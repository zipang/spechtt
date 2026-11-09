<#
.SYNOPSIS
    Verify (and optionally install) the runtime dependencies SpeechTT needs.

.DESCRIPTION
    Runs scripts/setup.ts through Bun. When `bun` itself is missing this script
    installs it from the official installer first, then refreshes PATH from the
    user and machine environment and runs the dependency check.

    Flags, in PowerShell spelling:
        -Check      probe only; exits 1 when something is missing
        -DryRun     print the plan and the exact commands, change nothing
        -Yes        skip the confirmation prompt

    The Unix spelling (--check, --dry-run, --yes) is accepted too and is passed
    to scripts/setup.ts unchanged, as is every other argument.

.EXAMPLE
    .\scripts\setup.ps1 -Check
#>
param(
    [switch]$Check,
    [switch]$DryRun,
    [switch]$Yes,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Rest
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Push-Location $repoRoot
try {
    if (-not (Get-Command bun -ErrorAction SilentlyContinue)) {
        Write-Host "bun is not installed. Installing it with:"
        Write-Host "  irm bun.sh/install.ps1 | iex"
        irm bun.sh/install.ps1 | iex

        # The installer updates the user environment; this process needs it now.
        # Join only the scopes that are set, so an unset one does not leave a
        # stray leading or trailing separator.
        $userPath = [System.Environment]::GetEnvironmentVariable("PATH", "User")
        $machinePath = [System.Environment]::GetEnvironmentVariable("PATH", "Machine")
        $env:PATH = (@($userPath, $machinePath) | Where-Object { $_ }) -join ";"
    }

    $forward = @()
    if ($Check) { $forward += "--check" }
    if ($DryRun) { $forward += "--dry-run" }
    if ($Yes) { $forward += "--yes" }
    # `$Rest` is $null when no remaining argument was given; splatting that would
    # pass one empty argument to a native command, which `runCli` rejects.
    if ($null -ne $Rest -and $Rest.Count -gt 0) { $forward += $Rest }

    & bun run scripts/setup.ts @forward
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
