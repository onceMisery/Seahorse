$ErrorActionPreference = 'Stop'

param(
    [string]$BaseUrl = 'http://127.0.0.1:8080',
    [string]$OutputDir = '.\release-validation-artifacts',
    [int]$JobPollAttempts = 120,
    [int]$JobPollSleepSeconds = 2,
    [switch]$RunPerfGate
)

function Ensure-Directory {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) {
        New-Item -ItemType Directory -Path $Path | Out-Null
    }
}

function Write-Utf8File {
    param(
        [string]$Path,
        [string]$Content
    )

    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

function Invoke-JsonRequest {
    param(
        [string]$Method,
        [string]$Url,
        [object]$Body
    )

    $headers = @{
        'content-type' = 'application/json'
    }

    if ($null -eq $Body) {
        return Invoke-RestMethod -Method $Method -Uri $Url -Headers $headers
    }

    return Invoke-RestMethod -Method $Method -Uri $Url -Headers $headers -Body (($Body | ConvertTo-Json -Depth 10) -replace '\r?\n', '')
}

function Invoke-TextRequest {
    param([string]$Url)

    return Invoke-WebRequest -Method GET -Uri $Url
}

function Save-JsonArtifact {
    param(
        [string]$Path,
        [object]$Value
    )

    Write-Utf8File -Path $Path -Content ($Value | ConvertTo-Json -Depth 10)
}

function Poll-RebuildJob {
    param(
        [string]$BaseUrl,
        [string]$JobId,
        [int]$Attempts,
        [int]$SleepSeconds
    )

    for ($index = 0; $index -lt $Attempts; $index++) {
        $job = Invoke-JsonRequest -Method GET -Url "$BaseUrl/admin/jobs/$JobId" -Body $null
        $status = $job.data.status
        if ($status -in @('succeeded', 'failed', 'cancelled')) {
            return $job
        }

        Start-Sleep -Seconds $SleepSeconds
    }

    throw "rebuild job $JobId did not reach terminal state in time"
}

Ensure-Directory -Path $OutputDir

$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$runDir = Join-Path $OutputDir $timestamp
Ensure-Directory -Path $runDir

Write-Host "Artifacts directory: $runDir"

$live = Invoke-JsonRequest -Method GET -Url "$BaseUrl/live" -Body $null
$ready = Invoke-JsonRequest -Method GET -Url "$BaseUrl/ready" -Body $null
$health = Invoke-JsonRequest -Method GET -Url "$BaseUrl/health" -Body $null
$stats = Invoke-JsonRequest -Method GET -Url "$BaseUrl/stats" -Body $null
$metrics = Invoke-TextRequest -Url "$BaseUrl/metrics"

Save-JsonArtifact -Path (Join-Path $runDir 'live.json') -Value $live
Save-JsonArtifact -Path (Join-Path $runDir 'ready.json') -Value $ready
Save-JsonArtifact -Path (Join-Path $runDir 'health.json') -Value $health
Save-JsonArtifact -Path (Join-Path $runDir 'stats.json') -Value $stats
Write-Utf8File -Path (Join-Path $runDir 'metrics.txt') -Content $metrics.Content

if ($live.data.status -ne 'ok') {
    throw '/live did not return status=ok'
}

if ($ready.data.status -notin @('ok', 'degraded')) {
    throw '/ready did not return a serviceable status'
}

$ingestRequest = @{
    namespace = 'default'
    content = "release validation alpha beta gamma $timestamp"
    source = @{
        type = 'inline'
        filename = "release-validation-$timestamp.txt"
    }
    options = @{
        auto_tag = $true
    }
}
$ingest = Invoke-JsonRequest -Method POST -Url "$BaseUrl/ingest" -Body $ingestRequest
Save-JsonArtifact -Path (Join-Path $runDir 'ingest.json') -Value $ingest

$recallRequest = @{
    namespace = 'default'
    query = 'release validation alpha'
    mode = 'basic'
    top_k = 5
    timeout_ms = 5000
}
$recallBeforeForget = Invoke-JsonRequest -Method POST -Url "$BaseUrl/recall" -Body $recallRequest
Save-JsonArtifact -Path (Join-Path $runDir 'recall-before-forget.json') -Value $recallBeforeForget

$chunkIds = @($ingest.data.chunk_ids)
if ($chunkIds.Count -eq 0) {
    throw 'ingest did not return any chunk_ids'
}

$forgetRequest = @{
    namespace = 'default'
    chunk_ids = @($chunkIds[0])
    mode = 'soft'
}
$forget = Invoke-JsonRequest -Method POST -Url "$BaseUrl/forget" -Body $forgetRequest
Save-JsonArtifact -Path (Join-Path $runDir 'forget.json') -Value $forget

$recallAfterForget = Invoke-JsonRequest -Method POST -Url "$BaseUrl/recall" -Body $recallRequest
Save-JsonArtifact -Path (Join-Path $runDir 'recall-after-forget.json') -Value $recallAfterForget

$forgottenChunkPresent = @($recallAfterForget.data.results | Where-Object { $_.chunk_id -eq $chunkIds[0] }).Count -gt 0
if ($forgottenChunkPresent) {
    throw "forgotten chunk $($chunkIds[0]) still appears in recall results"
}

$rebuildRequest = @{
    namespace = 'default'
    scope = 'all'
    force = $false
}
$rebuild = Invoke-JsonRequest -Method POST -Url "$BaseUrl/admin/rebuild" -Body $rebuildRequest
Save-JsonArtifact -Path (Join-Path $runDir 'rebuild-submit.json') -Value $rebuild

$jobId = $rebuild.data.job_id
if ([string]::IsNullOrWhiteSpace($jobId)) {
    throw 'rebuild did not return job_id'
}

$job = Poll-RebuildJob -BaseUrl $BaseUrl -JobId $jobId -Attempts $JobPollAttempts -SleepSeconds $JobPollSleepSeconds
Save-JsonArtifact -Path (Join-Path $runDir 'rebuild-job-final.json') -Value $job

if ($job.data.status -ne 'succeeded') {
    throw "rebuild job $jobId ended with status=$($job.data.status)"
}

$postRebuildReady = Invoke-JsonRequest -Method GET -Url "$BaseUrl/ready" -Body $null
$postRebuildStats = Invoke-JsonRequest -Method GET -Url "$BaseUrl/stats" -Body $null
Save-JsonArtifact -Path (Join-Path $runDir 'ready-after-rebuild.json') -Value $postRebuildReady
Save-JsonArtifact -Path (Join-Path $runDir 'stats-after-rebuild.json') -Value $postRebuildStats

if ($RunPerfGate) {
    $perfOutput = cargo test -p seahorse-server perf_baseline_10k -- --ignored --nocapture 2>&1
    Write-Utf8File -Path (Join-Path $runDir 'perf-gate.txt') -Content ($perfOutput -join [Environment]::NewLine)
    if ($LASTEXITCODE -ne 0) {
        throw 'perf gate failed'
    }
}

$summary = [ordered]@{
    base_url = $BaseUrl
    artifact_dir = $runDir
    request_ids = @{
        live = $live.request_id
        ready = $ready.request_id
        health = $health.request_id
        ingest = $ingest.request_id
        recall_before_forget = $recallBeforeForget.request_id
        forget = $forget.request_id
        recall_after_forget = $recallAfterForget.request_id
        rebuild_submit = $rebuild.request_id
        rebuild_job_final = $job.request_id
    }
    rebuild_job_id = $jobId
    rebuild_final_status = $job.data.status
    perf_gate_ran = [bool]$RunPerfGate
}

Save-JsonArtifact -Path (Join-Path $runDir 'summary.json') -Value $summary

Write-Host 'Release validation completed successfully.'
Write-Host "Summary: $(Join-Path $runDir 'summary.json')"
