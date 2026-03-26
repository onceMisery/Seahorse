$ErrorActionPreference = 'Stop'

function Read-Utf8File {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    return [System.IO.File]::ReadAllText($Path, [System.Text.UTF8Encoding]::new($false))
}

function Get-OpenApiPathBlock {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Content,
        [Parameter(Mandatory = $true)]
        [string]$PathName
    )

    $pattern = '^(  {0}:\r?\n.*?)(?=^  /|\z)' -f [System.Text.RegularExpressions.Regex]::Escape($PathName)
    $match = [System.Text.RegularExpressions.Regex]::Match(
        $Content,
        $pattern,
        [System.Text.RegularExpressions.RegexOptions]::Multiline -bor [System.Text.RegularExpressions.RegexOptions]::Singleline
    )

    if (-not $match.Success) {
        return $null
    }

    return $match.Groups[1].Value
}

function Get-MarkdownSectionBody {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Content,
        [Parameter(Mandatory = $true)]
        [string]$HeadingFragment
    )

    $pattern = '(?ms)^##\s+[^\r\n]*{0}[^\r\n]*\r?\n(.*?)(?=^##\s|\z)' -f [System.Text.RegularExpressions.Regex]::Escape($HeadingFragment)
    $match = [System.Text.RegularExpressions.Regex]::Match($Content, $pattern)
    if (-not $match.Success) {
        return $null
    }

    return $match.Groups[1].Value
}

function Get-BlockAfterMarker {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Content,
        [Parameter(Mandatory = $true)]
        [string]$StartMarker,
        [string]$EndPattern = '^##\s'
    )

    $pattern = '(?ms){0}[^\r\n]*\r?\n(.*?)(?={1}|\z)' -f [System.Text.RegularExpressions.Regex]::Escape($StartMarker), $EndPattern
    $match = [System.Text.RegularExpressions.Regex]::Match($Content, $pattern)
    if (-not $match.Success) {
        return $null
    }

    return $match.Groups[1].Value
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$docsRoot = Join-Path $repoRoot 'docs'
$failures = [System.Collections.Generic.List[string]]::new()

$designDocPath = Join-Path $repoRoot 'docs\design-all.md'
$legacyDesignDocPath = Join-Path $repoRoot 'docs\design-al.md'
$readmePath = Join-Path $repoRoot 'docs\README.md'
$roadmapPath = Join-Path $repoRoot 'docs\mvp-design-and-roadmap.md'
$openApiPath = Join-Path $repoRoot 'docs\mvp-openapi.yaml'
$checklistPath = Join-Path $repoRoot 'docs\mvp-release-checklist.md'
$runbookPath = Join-Path $repoRoot 'docs\runbooks\mvp-deploy-backup-rollback.md'

$readme = Read-Utf8File -Path $readmePath
$roadmap = Read-Utf8File -Path $roadmapPath
$openApi = Read-Utf8File -Path $openApiPath
$checklist = Read-Utf8File -Path $checklistPath
$runbook = Read-Utf8File -Path $runbookPath

$faultInjectionTerm = ([string]([char]0x6545)) + ([char]0x969C) + ([char]0x6CE8) + ([char]0x5165)
$implementedHeading = ([string]([char]0x5F53)) + ([char]0x524D) + ([char]0x4EE3) + ([char]0x7801) + ([char]0x5DF2) + ([char]0x5B9E) + ([char]0x73B0)
$evidenceGapHeading = ([string]([char]0x5F53)) + ([char]0x524D) + ([char]0x4ECD) + ([char]0x7F3A) + ([char]0x8BC1) + ([char]0x636E) + ' / ' + ([char]0x7F3A) + ([char]0x9A8C) + ([char]0x8BC1)
$runbookImplementedMarker = ([string]([char]0x5F53)) + ([char]0x524D) + ([char]0x4EE3) + ([char]0x7801) + ([char]0x5DF2) + ([char]0x5B9E) + ([char]0x73B0) + ([char]0x5E76) + ([char]0x53EF) + ([char]0x6309) + ([char]0x672C) + ([char]0x624B) + ([char]0x518C) + ([char]0x6267) + ([char]0x884C) + ([char]0x7684) + ([char]0x80FD) + ([char]0x529B)
$runbookEvidenceMarker = ([string]([char]0x4EE5)) + ([char]0x4E0B) + ([char]0x4E8B) + ([char]0x9879) + ([char]0x5F53) + ([char]0x524D) + ([char]0x4ECD) + ([char]0x7F3A) + ([char]0x8BC1) + ([char]0x636E) + ' / ' + ([char]0x7F3A) + ([char]0x9A8C) + ([char]0x8BC1)

if (-not (Test-Path $designDocPath)) {
    $null = $failures.Add('docs/design-all.md is missing')
}
if (Test-Path $legacyDesignDocPath) {
    $null = $failures.Add('docs/design-al.md should be removed after renaming to docs/design-all.md')
}

$legacyDesignRefs = [System.Collections.Generic.List[string]]::new()
foreach ($file in Get-ChildItem -Path $docsRoot -Recurse -File) {
    $content = Read-Utf8File -Path $file.FullName
    if ($content -match 'design-al\.md') {
        $relativePath = [System.IO.Path]::GetRelativePath($repoRoot, $file.FullName).Replace('\\', '/')
        $null = $legacyDesignRefs.Add($relativePath)
    }
}
if ($legacyDesignRefs.Count -gt 0) {
    $null = $failures.Add(('legacy design-al.md references remain in: {0}' -f ($legacyDesignRefs -join ', ')))
}
if ($readme -notmatch 'design-all\.md') {
    $null = $failures.Add('docs/README.md does not reference design-all.md')
}
if ($roadmap -notmatch 'design-all\.md') {
    $null = $failures.Add('docs/mvp-design-and-roadmap.md does not reference design-all.md')
}

$recallBlock = Get-OpenApiPathBlock -Content $openApi -PathName '/recall'
$forgetBlock = Get-OpenApiPathBlock -Content $openApi -PathName '/forget'
$metricsBlock = Get-OpenApiPathBlock -Content $openApi -PathName '/metrics'
$openApiWithoutRecall = if ($null -eq $recallBlock) { $openApi } else { $openApi.Replace($recallBlock, '') }

if ($null -eq $metricsBlock) {
    $null = $failures.Add('docs/mvp-openapi.yaml is missing the formal /metrics contract')
}
elseif ($metricsBlock -notmatch 'text/plain; version=0.0.4' -or $metricsBlock -notmatch 'enable_metrics=true' -or $metricsBlock -notmatch 'observability\.metrics_path') {
    $null = $failures.Add('docs/mvp-openapi.yaml /metrics block must describe Prometheus format, enable_metrics gating, and metrics_path override')
}

if ($null -eq $recallBlock -or $recallBlock -notmatch 'enum: \[basic\]' -or $recallBlock -notmatch 'default: basic') {
    $null = $failures.Add('docs/mvp-openapi.yaml does not lock /recall mode to basic')
}
if ($openApiWithoutRecall -match 'enum: \[basic\]' -or $openApiWithoutRecall -match 'default: basic') {
    $null = $failures.Add('docs/mvp-openapi.yaml allows mode=basic outside /recall')
}
if ($null -eq $forgetBlock -or $forgetBlock -notmatch 'enum: \[soft\]' -or $forgetBlock -notmatch 'default: soft') {
    $null = $failures.Add('docs/mvp-openapi.yaml does not lock /forget mode to soft')
}
if ($forgetBlock -match '\bhard\b' -or $openApi -match '\[soft,\s*hard\]') {
    $null = $failures.Add('docs/mvp-openapi.yaml still exposes hard delete in the formal contract')
}

$checklistImplementedSection = Get-MarkdownSectionBody -Content $checklist -HeadingFragment $implementedHeading
$checklistEvidenceSection = Get-MarkdownSectionBody -Content $checklist -HeadingFragment $evidenceGapHeading
if ($null -eq $checklistImplementedSection) {
    $null = $failures.Add('docs/mvp-release-checklist.md is missing the implemented section')
}
else {
    foreach ($term in @('/metrics', 'mode=soft', 'enable_metrics')) {
        if ($checklistImplementedSection -notmatch [System.Text.RegularExpressions.Regex]::Escape($term)) {
            $null = $failures.Add(('docs/mvp-release-checklist.md implemented section is missing: {0}' -f $term))
        }
    }
}
if ($null -eq $checklistEvidenceSection) {
    $null = $failures.Add('docs/mvp-release-checklist.md is missing the evidence-gap section')
}
else {
    foreach ($term in @('contract', 'E2E', $faultInjectionTerm)) {
        if ($checklistEvidenceSection -notmatch [System.Text.RegularExpressions.Regex]::Escape($term)) {
            $null = $failures.Add(('docs/mvp-release-checklist.md evidence-gap section is missing: {0}' -f $term))
        }
    }
}
if ($checklist -notmatch 'release blocker') {
    $null = $failures.Add('docs/mvp-release-checklist.md must label the missing-evidence section as release blocker')
}

$runbookImplementedSection = Get-BlockAfterMarker -Content $runbook -StartMarker $runbookImplementedMarker -EndPattern ([System.Text.RegularExpressions.Regex]::Escape($runbookEvidenceMarker))
$runbookEvidenceSection = Get-MarkdownSectionBody -Content $runbook -HeadingFragment $evidenceGapHeading
if ($null -eq $runbookImplementedSection) {
    $null = $failures.Add('docs/runbooks/mvp-deploy-backup-rollback.md is missing the implemented capability block')
}
else {
    foreach ($term in @('/metrics', 'mode=soft', 'enable_metrics')) {
        if ($runbookImplementedSection -notmatch [System.Text.RegularExpressions.Regex]::Escape($term)) {
            $null = $failures.Add(('docs/runbooks/mvp-deploy-backup-rollback.md implemented block is missing: {0}' -f $term))
        }
    }
}
if ($null -eq $runbookEvidenceSection) {
    $null = $failures.Add('docs/runbooks/mvp-deploy-backup-rollback.md is missing the evidence-gap block')
}
else {
    foreach ($term in @('contract', 'E2E', $faultInjectionTerm)) {
        if ($runbookEvidenceSection -notmatch [System.Text.RegularExpressions.Regex]::Escape($term)) {
            $null = $failures.Add(('docs/runbooks/mvp-deploy-backup-rollback.md evidence-gap block is missing: {0}' -f $term))
        }
    }
}
if ($runbook -notmatch 'release blocker') {
    $null = $failures.Add('docs/runbooks/mvp-deploy-backup-rollback.md must label missing evidence as release blocker')
}

if ($failures.Count -gt 0) {
    Write-Host 'FAIL'
    foreach ($failure in $failures) {
        Write-Host ('- {0}' -f $failure)
    }
    exit 1
}

Write-Host 'PASS'