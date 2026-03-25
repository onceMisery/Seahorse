# Rebuild And Repair Stability Design

**Date:** 2026-03-25

## Goal
Ensure rebuild and repair work do not remain stuck in `running` without intervention. Rebuild jobs must be auto-failed when the worker is gone or stops making progress. The repair worker must self-heal after panic or runtime errors and must not silently exit or spin on the same failure forever.

## Scope
- Add an in-process watchdog for rebuild worker liveness.
- Add panic containment around rebuild execution so unexpected panics still end in a terminal job state.
- Add heartbeat-driven progress updates during rebuild so long embedding phases do not appear frozen at `0/N`.
- Add a repair supervisor loop with restart and backoff.
- Add runtime recovery for `running` repair tasks when the repair worker is unhealthy after startup.
- Add targeted tests for timeout, panic recovery, restart, and runtime task recovery.

## Non-Goals
- No distributed lease or cross-process leader election.
- No force-kill of blocked threads at the OS level.
- No redesign of ingest/forget pipelines.
- No large storage or schema redesign unless implementation proves it is strictly necessary.

## Constraints
- Existing startup recovery remains the crash boundary for process restarts.
- The user requirement is operational: job rows and repair tasks must not stay in `running` indefinitely, even if a worker thread wedges.
- `Cargo.lock` must not be committed and `target/` remains ignored.

## Problem Summary
- `run_rebuild_job` can panic and leave a rebuild job in `queued` or `running`.
- Rebuild embedding work runs for a long time without heartbeats, so a stuck worker is indistinguishable from a slow one.
- `rebuild()` reuses an active rebuild job without checking whether the worker is actually alive.
- The repair worker loop returns on some initialization failures and only recovers `running` tasks during process startup.
- Repeated repair failures can be retried immediately, causing empty spin or failure storms.

## Options Considered

### Option 1: DB-Leased Workers
Store heartbeat/lease ownership in SQLite and let a DB watchdog recover stale work.

**Pros**
- Survives multi-process deployment.
- State is externally observable.

**Cons**
- More schema work.
- More transaction design and edge cases than this codebase currently needs.
- Overkill for the current single-process server.

### Option 2: In-Process Watchdog And Supervisor
Keep liveness state in memory, use existing DB terminal states for recovery, and add guarded finalization so stale workers cannot re-mark tasks successful after watchdog intervention.

**Pros**
- Smallest change that directly addresses the current failures.
- Fits the current single-process runtime model.
- Keeps tests local and deterministic.

**Cons**
- Does not coordinate multiple server processes.
- A wedged thread may still exist in memory even after its job/task is recovered.

### Option 3: Request-Time Recovery Only
Recover stale rebuild/repair state only when new API calls arrive.

**Pros**
- Lowest implementation cost.

**Cons**
- Violates the operational requirement that stuck `running` work must not persist.
- Does nothing for silent repair worker exit while the server is otherwise idle.

## Chosen Design
Use **Option 2**.

This iteration adds an in-process runtime watchdog plus guarded terminal-state transitions:
- Rebuild workers publish heartbeats and progress.
- A watchdog marks stale or orphaned rebuild jobs failed.
- Rebuild thread panics are caught and converted into failed jobs.
- The repair worker runs under a supervisor with restart backoff.
- The watchdog recovers `running` repair tasks when the repair worker exits or stops heartbeating.
- Repository state transitions become guarded so stale workers cannot overwrite a terminal state after watchdog recovery.

## Rebuild Design

### Runtime Liveness State
- Add a runtime registry in `AppState` for active rebuild workers.
- Each rebuild worker gets a shared heartbeat handle with:
  - last heartbeat timestamp
  - last progress string
  - terminal flag
- This registry is process-local and exists only to drive watchdog decisions.

### Worker Lifecycle
- `spawn_rebuild_worker` registers the worker heartbeat before spawning the thread.
- The thread entrypoint wraps `run_rebuild_job` in `catch_unwind`.
- On panic, the worker:
  - logs the panic
  - attempts to mark the job failed
  - marks the runtime handle terminal

### Heartbeats And Progress
- Replace the single bulk `build_rebuild_entries` call with a heartbeat-aware builder.
- The builder updates progress during embedding generation, not only at the end.
- Heartbeats are emitted on a bounded cadence:
  - every N chunks, or
  - when a minimum elapsed duration passes
- Progress format remains compatible with existing job payloads, e.g. `37/800`.
- Staleness is defined by heartbeat age, not by unchanged progress text alone. Progress is for observability and operator context.

### Watchdog Behavior
- A background watchdog thread runs on a short interval.
- For each active rebuild job:
  - if there is no registered worker heartbeat, mark the job failed as orphaned
  - if the heartbeat is older than the rebuild timeout, mark the job failed as timed out
- After failing the job, restore `index_state` from current repository state if no other rebuild remains active.

### Guarded Finalization
- `apply_rebuild_result` must re-read the job and continue only if it is still `running`.
- If the watchdog already marked it `failed` or the user cancelled it, the rebuild result is discarded.
- Repository job-finishing updates should only affect active jobs (`queued` or `running`) so stale workers cannot resurrect a completed job.

### Rebuild Submission Path
- `rebuild()` should not blindly reuse the latest active job.
- If the active job has no live worker heartbeat or its heartbeat is stale, recover it first and create a fresh job.

## Repair Design

### Supervisor Model
- Replace the current bare `run_repair_worker_loop` with a supervisor-oriented runtime:
  - worker loop execution
  - panic containment
  - restart logging
  - restart backoff
- The supervisor itself publishes heartbeats so the watchdog can distinguish a healthy idle worker from a dead or wedged one.

### Failure Backoff
- When worker initialization or `run_once` fails, sleep before retrying.
- Use bounded exponential backoff with a floor and cap.
- A successful task execution or healthy idle poll resets the backoff level.
- This prevents high-frequency empty retries when the same underlying error persists.

### Runtime Recovery Of Running Tasks
- Keep the existing startup `recover_running_repair_tasks`.
- Add watchdog-triggered runtime recovery:
  - if the repair worker heartbeat is stale, recover `running` tasks immediately
  - if the worker exits after panic or repeated runtime failure, recover `running` tasks before restart
- Recovery reuses the existing storage API and error message pattern so task history stays understandable.

### Guarded Task Completion
- `succeed_repair_task` and `fail_repair_task` should update only tasks still in `running`.
- This prevents a stale worker from overwriting a task that the watchdog already recovered and re-queued.

## Observability
- Add `tracing` logs for:
  - rebuild worker spawn
  - rebuild heartbeat timeout/orphan recovery
  - rebuild panic recovery
  - repair worker failure
  - repair worker restart with backoff
  - repair runtime recovery of stuck `running` tasks
- Keep observability minimal and log-focused for this iteration.

## Testing Strategy
- Add server/runtime tests for:
  - rebuild worker panic ends in terminal `failed`
  - rebuild heartbeat timeout auto-fails the job
  - a stale active rebuild job is not reused forever
  - repair worker panic or runtime error triggers restart
  - repair failures back off instead of hot-looping
  - runtime watchdog recovers `running` repair tasks after worker unhealthy state
- Add or update repository tests for guarded task/job state transitions where useful.

## Rollout Notes
- No API contract change is expected.
- No user configuration is required for this iteration.
- Default timeout/backoff values should be internal constants, with test-only hooks for shorter durations.
