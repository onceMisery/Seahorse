# Rebuild And Repair Stability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent rebuild jobs and repair tasks from remaining stuck in `running`, while making the repair worker self-heal after panic, runtime error, or watchdog-detected stall.

**Architecture:** Keep liveness management in-process. `seahorse-server` gains runtime watchdog/supervisor state, rebuild heartbeat updates, rebuild panic containment, and repair worker restart/backoff. `seahorse-core` storage guards ensure stale workers cannot overwrite terminal task/job states after watchdog intervention.

**Tech Stack:** Rust, standard library threads/synchronization, Axum, rusqlite, existing `tracing` crate.

---

### Task 1: Add Guarded Storage Transitions

**Files:**
- Modify: `crates/seahorse-core/src/storage/repository.rs`
- Test: `crates/seahorse-core/src/storage/repository.rs`

- [ ] **Step 1: Write failing repository tests for guarded updates**

Add tests that prove:
- `finish_maintenance_job` does not overwrite a job already marked `failed`, `succeeded`, or `cancelled`
- `succeed_repair_task` does not overwrite a task already recovered to `failed`
- `fail_repair_task` does not increment or overwrite a task no longer in `running`

Suggested skeleton:

```rust
#[test]
fn guarded_repair_task_transitions_ignore_non_running_rows() {
    let mut repository = repository_with_schema();
    let task_id = repository
        .enqueue_repair_task("default", "index_insert", "file", Some(1), Some("{\"error\":\"x\"}"))
        .expect("enqueue");
    repository
        .claim_next_repair_task("default", 3)
        .expect("claim")
        .expect("task");
    repository
        .recover_running_repair_tasks("default", 3, "watchdog")
        .expect("recover");

    repository.succeed_repair_task(task_id).expect("late success");

    let task = repository.get_repair_task(task_id).expect("load").expect("task");
    assert_eq!(task.status, "failed");
}
```

- [ ] **Step 2: Run targeted repository tests and confirm failure**

Run: `cargo test -p seahorse-core guarded_ -- --nocapture`

Expected: tests fail because current updates are unconditional.

- [ ] **Step 3: Guard state transitions in repository methods**

Change SQL so these updates apply only when the row is still active:

```sql
UPDATE repair_queue
SET status = 'succeeded', last_error = NULL
WHERE id = ?1 AND status = 'running'
```

```sql
UPDATE maintenance_jobs
SET status = ?2, progress = ?3, result_summary = ?4, error_message = ?5, finished_at = CURRENT_TIMESTAMP
WHERE id = ?1 AND status IN ('queued', 'running')
```
```

Do the same for `fail_repair_task` and keep `mark_maintenance_job_running` limited to active rows.

- [ ] **Step 4: Re-run repository tests**

Run: `cargo test -p seahorse-core guarded_ -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/seahorse-core/src/storage/repository.rs
git commit -m "fix: guard stale repair and maintenance updates"
```

### Task 2: Add Rebuild Runtime Controls And Watchdog

**Files:**
- Modify: `crates/seahorse-server/src/state/mod.rs`
- Test: `crates/seahorse-server/src/state/mod.rs`

- [ ] **Step 1: Write failing rebuild stability tests**

Add tests that cover:
- rebuild panic is converted to terminal `failed`
- rebuild timeout auto-fails a job instead of leaving it `running`
- `rebuild()` does not return a stale or orphaned active job to the caller

Use a test-only runtime config with short watchdog/timeout values. Add `#[cfg(test)]` runtime hooks inside `state/mod.rs` so tests can:
- force a rebuild panic during worker execution
- delay heartbeat progress long enough to trigger timeout

Suggested assertions:

```rust
assert_eq!(final_body["data"]["status"], Value::String("failed".to_owned()));
assert!(final_body["data"]["error_message"].as_str().unwrap_or_default().contains("watchdog"));
```

- [ ] **Step 2: Run targeted server rebuild tests and confirm failure**

Run: `cargo test -p seahorse-server rebuild_ -- --nocapture`

Expected: new tests fail because rebuild currently has no panic containment or watchdog timeout.

- [ ] **Step 3: Add internal runtime config and worker heartbeat state**

In `crates/seahorse-server/src/state/mod.rs`, add internal structs like:

```rust
#[derive(Debug, Clone)]
struct RuntimeConfig {
    rebuild_watchdog_interval: Duration,
    rebuild_stale_after: Duration,
    rebuild_progress_interval: usize,
    rebuild_progress_min_age: Duration,
    repair_watchdog_interval: Duration,
    repair_stale_after: Duration,
    repair_restart_backoff_initial: Duration,
    repair_restart_backoff_max: Duration,
}
```

Add process-local runtime state for active rebuild worker heartbeats and a single repair worker registration.

- [ ] **Step 4: Add rebuild panic containment and heartbeat-aware progress updates**

Wrap the rebuild thread entrypoint in `catch_unwind`.

Replace the bulk builder with a callback-based builder:

```rust
fn build_rebuild_entries_with_progress<F>(
    embedding_provider: &StubEmbeddingProvider,
    chunks: &[RebuildChunkRecord],
    mut on_progress: F,
) -> Result<Vec<IndexEntry>, RebuildError>
where
    F: FnMut(usize, usize) -> Result<(), RebuildError>,
```

`on_progress` should:
- update runtime heartbeat
- periodically persist `progress` to the maintenance job row

- [ ] **Step 5: Add rebuild watchdog loop**

Spawn a watchdog thread during `AppState` initialization. It should:
- scan active rebuild jobs
- mark orphaned or stale ones failed
- restore `index_state` from current repository state only after the watchdog fails a rebuild and only when no other rebuild remains active
- skip jobs whose worker heartbeat is current

Staleness must be based on heartbeat age, not unchanged progress text.

- [ ] **Step 6: Add rebuild submission-path stale worker recovery**

Before `rebuild()` reuses an active rebuild job, inspect its runtime heartbeat:
- if the worker heartbeat exists and is fresh, return the active job as today
- if the heartbeat is missing or stale, fail that active job immediately and create a fresh one in the same request path

Add a focused test for this exact path so implementation cannot satisfy the plan with watchdog-only eventual recovery.

- [ ] **Step 7: Guard rebuild result application**

In `apply_rebuild_result`, reload the job and proceed only when status is still `running`. If status is terminal, return without mutating vector index or chunk/file DB state.

- [ ] **Step 8: Re-run rebuild-focused tests**

Run: `cargo test -p seahorse-server rebuild_ -- --nocapture`

Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add crates/seahorse-server/src/state/mod.rs
git commit -m "fix: watchdog rebuild worker liveness"
```

### Task 3: Add Repair Supervisor, Backoff, And Runtime Recovery

**Files:**
- Modify: `crates/seahorse-server/src/state/mod.rs`
- Modify: `crates/seahorse-core/src/jobs/mod.rs` (only if a small helper is needed; prefer not to expand scope)
- Test: `crates/seahorse-server/src/state/mod.rs`

- [ ] **Step 1: Write failing repair stability tests**

Add tests that prove:
- repair worker panic or runtime error causes automatic restart
- consecutive repair failures do not hot-loop
- runtime watchdog recovers `running` repair tasks even after startup
- a healthy idle repair supervisor is not treated as stale or spuriously restarted

Suggested test shape:
- seed a repair task with a `#[cfg(test)]` executor/runtime hook that triggers deterministic panic/error
- use a short runtime config for repair timeout/backoff
- assert the task leaves `running`
- assert a later healthy task can still be processed after restart
- assert an idle worker keeps heartbeating and survives beyond the stale threshold without recovery firing

- [ ] **Step 2: Run targeted repair tests and confirm failure**

Run: `cargo test -p seahorse-server repair_ -- --nocapture`

Expected: new tests fail because repair currently exits, retries immediately, and only recovers `running` tasks at startup.

- [ ] **Step 3: Refactor repair loop into supervisor + heartbeat model**

Implement a repair runtime handle with:
- heartbeat timestamp
- current backoff level
- terminal/stale marker
- idle loop heartbeat updates so a healthy but idle worker is distinguishable from a dead one
- preserve the existing startup `recover_running_repair_tasks` path; the supervisor/watchdog adds runtime recovery, not a replacement for startup crash recovery

Run the repair worker under a restart loop:

```rust
loop {
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| run_repair_worker_once(...)));
    match outcome {
        Ok(Ok(())) => reset_backoff(),
        Ok(Err(error)) | Err(_) => {
            recover_running_repair_tasks(...);
            sleep(next_backoff());
        }
    }
}
```

- [ ] **Step 4: Add repair watchdog recovery**

Extend the watchdog so that when the registered repair worker heartbeat is stale or missing:
- recover `running` repair tasks
- log the recovery
- spawn a fresh repair supervisor generation

Also add an explicit negative case in tests proving the watchdog does not recover or restart a worker whose only state is healthy idle polling.

- [ ] **Step 5: Re-run repair-focused tests**

Run: `cargo test -p seahorse-server repair_ -- --nocapture`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/seahorse-server/src/state/mod.rs crates/seahorse-core/src/jobs/mod.rs
git commit -m "fix: supervise repair worker recovery"
```

### Task 4: Full Verification And Warning Sweep

**Files:**
- Modify if needed: `crates/seahorse-server/src/main.rs`
- Modify if needed: `crates/seahorse-server/src/api/mod.rs`
- Modify if needed: `crates/seahorse-server/src/api/observability.rs`
- Modify if needed: `crates/seahorse-server/src/state/mod.rs`

- [ ] **Step 1: Run formatting and diff checks**

Run: `cargo fmt --all`

Run: `git diff --check`

Expected: no formatting or whitespace issues

- [ ] **Step 2: Run full test suite**

Run: `cargo test`

Expected: PASS

- [ ] **Step 3: Fix residual warnings introduced or exposed by this work**

If warnings remain in touched files, clear them now. Prefer:
- `#[cfg(test)]` around test-only helpers like `build_app`
- removing or renaming unused fields/locals
- avoiding dead helper functions

- [ ] **Step 4: Check required watchdog/restart logs exist**

Verify the implementation logs all required events from the spec:
- rebuild worker spawn
- rebuild heartbeat timeout or orphan recovery
- rebuild panic recovery
- repair worker failure
- repair worker restart with backoff
- repair runtime recovery of stuck `running` tasks

- [ ] **Step 5: Re-run full verification**

Run: `cargo test`

Expected: PASS and warning count not worse than baseline

- [ ] **Step 6: Commit**

```bash
git add crates/seahorse-server/src/main.rs crates/seahorse-server/src/api/mod.rs crates/seahorse-server/src/api/observability.rs crates/seahorse-server/src/state/mod.rs
git commit -m "chore: clean runtime stability warnings"
```
