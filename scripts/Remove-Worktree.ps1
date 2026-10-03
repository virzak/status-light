<#
.SYNOPSIS
    Removes a worktree made by New-Worktree.ps1, and its link to the shared Claude Code memory.

.DESCRIPTION
    Deletes the worktree's memory junction first, so the shared memory it points to is never
    touched, then runs `git worktree remove`. Refuses a worktree with uncommitted changes unless
    -Force is given, as git does. The branch itself is kept; delete it with git once it is merged.

.EXAMPLE
    .\scripts\Remove-Worktree.ps1 feature/wisp-flame
#>
[CmdletBinding()]
param(
    # Branch whose worktree to remove, as given to New-Worktree.ps1.
    [Parameter(Mandatory, Position = 0)]
    [string] $Branch,

    # Worktree folder, when it is not the default ..\<repo>.wt\<branch>.
    [string] $Path,

    # Remove even with uncommitted changes (passed to git worktree remove).
    [switch] $Force
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'WorktreeCommon.ps1')

$root = Get-MainRoot
if (-not $Path) { $Path = Get-WorktreePath $root $Branch }
if (-not (Test-Path $Path)) { throw "No worktree at $Path." }
$target = (Resolve-Path $Path).Path

$link = Join-Path (Get-ClaudeProjectDir $target) 'memory'
if ((Test-Path $link) -and (Get-Item $link).LinkType -eq 'Junction') {
    # Deletes the junction only; the shared memory it points to stays.
    [System.IO.Directory]::Delete($link)
    Write-Information "Unlinked Claude memory" -InformationAction Continue
}

$gitArgs = @('worktree', 'remove') + @(if ($Force) { '--force' }) + $target
& git @gitArgs
if ($LASTEXITCODE -ne 0) { throw "git worktree remove failed ($LASTEXITCODE)." }

$parent = Split-Path $target
if ((Test-Path $parent) -and -not (Get-ChildItem $parent -Force)) { Remove-Item $parent }
Write-Information "Removed $target" -InformationAction Continue
