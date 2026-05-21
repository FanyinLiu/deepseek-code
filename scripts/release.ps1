Param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^(v?\d+\.\d+\.\d+)(-[0-9A-Za-z\.\-]+)?$')]
    [string]$Version,

    [string]$Remote = "origin",
    [string]$Branch = "main",
    [switch]$SkipChecks,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

function Run-Or-Throw {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Command,
        [Parameter()]
        [string[]]$Arguments = @()
    )

    $commandLine = if ($Arguments.Count -eq 0) { $Command } else { "$Command $Arguments" }
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed: $commandLine"
    }
}

function Assert-CleanWorkingTree {
    if (git status --porcelain) {
        throw "Working tree is not clean. Commit or stash changes before running release."
    }
}

function Get-CurrentVersion {
    $toml = Get-Content -Raw "Cargo.toml"
    $match = [regex]::Match($toml, '^(?m)\s*version\s*=\s*"(.*?)"\s*$')
    if (-not $match.Success) {
        throw "Could not find version in Cargo.toml."
    }
    return $match.Groups[1].Value
}

function Set-NpmPackageVersion {
    param(
        [Parameter(Mandatory = $true)]
        [string]$VersionNoV
    )

    foreach ($path in @("package.json", "package-lock.json")) {
        if (-not (Test-Path $path)) {
            continue
        }
        $json = Get-Content $path -Raw | ConvertFrom-Json
        $json.version = $VersionNoV
        if ($json.packages -and $json.packages."") {
            $json.packages."".version = $VersionNoV
        }
        $json | ConvertTo-Json -Depth 32 | Set-Content $path -Encoding utf8
    }
}

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location (Resolve-Path (Join-Path $scriptRoot ".."))

$tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
$versionNoV = $Version -replace '^v', ''

Write-Host "=== Octo Release Helper ==="
Write-Host "Release target: $tag"
if ($DryRun) {
    Write-Host "Mode: dry-run (no commit/tag/push)."
}

if ((git rev-parse --abbrev-ref HEAD) -ne $Branch) {
    throw "Please run release from branch '$Branch'."
}

Assert-CleanWorkingTree

if (-not $SkipChecks) {
    Write-Host "Running project checks..."
    Run-Or-Throw -Command "cargo" -Arguments @("fmt", "--all", "--check")
    Run-Or-Throw -Command "cargo" -Arguments @("check", "--all-targets", "--all-features")
    Run-Or-Throw -Command "cargo" -Arguments @("clippy", "--all-targets", "--all-features", "--", "-D", "warnings")
    Run-Or-Throw -Command "cargo" -Arguments @("test", "--all-features")
    Run-Or-Throw -Command "cargo" -Arguments @("run", "--", "--help")
    Run-Or-Throw -Command "cargo" -Arguments @("run", "--bin", "octo", "--", "--help")
    Run-Or-Throw -Command "cargo" -Arguments @("run", "--bin", "octo", "--", "preview-tui", "--api", "ready", "--scenario", "welcome", "--width", "80", "--height", "24")
    Run-Or-Throw -Command "node" -Arguments @("scripts/npm-bootstrap.js", "--dry-run")
    Run-Or-Throw -Command "npm" -Arguments @("pack", "--dry-run")
}

$changelogPath = "CHANGELOG.md"
if (-not (Test-Path $changelogPath)) {
    throw "CHANGELOG.md missing. Create it before release."
}

$changelog = Get-Content $changelogPath -Raw
if ($changelog -notmatch "(?m)^## \[Unreleased\]") {
    throw "CHANGELOG.md must contain '## [Unreleased]' section."
}
$unreleased = [regex]::Match($changelog, "(?ms)^## \[Unreleased\]\r?\n(.*?)(?=^## \[|$)")
if (-not $unreleased.Success -or [string]::IsNullOrWhiteSpace($unreleased.Groups[1].Value)) {
    throw "CHANGELOG.md '[Unreleased]' section cannot be empty."
}

$currentVersion = Get-CurrentVersion
if ($currentVersion -eq $versionNoV) {
    throw "Cargo.toml already at version $versionNoV. No version bump needed."
}
$cargoToml = Get-Content "Cargo.toml" -Raw
$updatedToml = [regex]::Replace(
    $cargoToml,
    '(?ms)^(\s*version\s*=\s*)".*?"(\s*)$',
    "`$1`"$versionNoV`"$2",
    1
)
if ($cargoToml -eq $updatedToml) {
    throw "Failed to update Cargo.toml version."
}
Set-Content "Cargo.toml" -Value $updatedToml -Encoding utf8
Set-NpmPackageVersion -VersionNoV $versionNoV

Run-Or-Throw -Command "cargo" -Arguments @("generate-lockfile")

if ($DryRun) {
    Write-Host "Dry-run mode: release files updated locally; no commit/tag/push."
    Write-Host "If this is correct, rerun without -DryRun:"
    Write-Host "  .\scripts\release.ps1 -Version $tag"
    return
}

if ((git tag --list $tag)) {
    throw "Tag $tag already exists."
}

git add Cargo.toml Cargo.lock CHANGELOG.md package.json package-lock.json
if (git diff --cached --quiet) {
    throw "No changes staged after version bump. Aborting."
}

$commitMsg = "chore(release): $tag"
Run-Or-Throw -Command "git" -Arguments @("commit", "-m", $commitMsg)

Run-Or-Throw -Command "git" -Arguments @("tag", "-a", $tag, "-m", $tag)
Run-Or-Throw -Command "git" -Arguments @("push", $Remote, $Branch)
Run-Or-Throw -Command "git" -Arguments @("push", $Remote, $tag)

Write-Host "Release prepared. Tag $tag pushed to $Remote, wait for GitHub Actions release assets."
