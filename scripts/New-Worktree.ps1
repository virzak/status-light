<#
.SYNOPSIS
    Creates a git worktree beside this checkout, sharing local-only files and Claude Code memory.

.DESCRIPTION
    The worktree goes to a sibling folder named after the repo with a `.wt` suffix, in a subfolder
    named after the branch with slashes replaced by dashes: `<repo>.wt/<branch>`.

    Gitignored per-machine files listed in $sharedItems are symlinked from the main checkout, so all
    worktrees share one copy (symlinks need Developer Mode or an elevated shell; -Copy copies
    instead).

    Claude Code keys its per-project data on the working directory, so a session started in a
    worktree would not see memory saved from the main checkout. Unless -NoSharedMemory is given,
    the worktree's memory folder becomes a junction to the main checkout's, so both read and write
    the same files; transcripts stay per worktree. Remove-Worktree.ps1 undoes all of this.

.EXAMPLE
    .\scripts\New-Worktree.ps1 -NewBranch feature/wisp-flame -Base master
    Creates feature/wisp-flame from master in ..\status-light.wt\feature-wisp-flame.

.EXAMPLE
    .\scripts\New-Worktree.ps1 feature/strip -Open
    Checks out the existing branch and opens VS Code there.
#>
[CmdletBinding()]
param(
    # Existing branch to check out. Omit when using -NewBranch.
    [Parameter(Position = 0)]
    [string] $Branch,

    # Name of a new branch to create.
    [string] $NewBranch,

    # Commit or branch the new branch starts from. Defaults to the current commit.
    [string] $Base,

    # Where to create the worktree. Defaults to ..\<repo>.wt\<branch>.
    [string] $Path,

    # Copy the shared items instead of symlinking them.
    [switch] $Copy,

    # Open the new worktree in VS Code.
    [switch] $Open,

    # Do not share the main checkout's Claude Code memory with the worktree.
    [switch] $NoSharedMemory
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'WorktreeCommon.ps1')

# Gitignored items to share between worktrees; paths are relative to the repo root.
$sharedItems = @(
    '.vscode'
)

if (-not $Branch -and -not $NewBranch) { throw 'Specify a branch to check out or -NewBranch.' }
if ($Base -and -not $NewBranch) { throw '-Base only applies with -NewBranch.' }

$root = Get-MainRoot
if (-not $Path) { $Path = Get-WorktreePath $root ($NewBranch ? $NewBranch : $Branch) }

$gitArgs = @('worktree', 'add')
if ($NewBranch) { $gitArgs += @('-b', $NewBranch, $Path) + @($Base | Where-Object { $_ }) }
else { $gitArgs += @($Path, $Branch) }

& git @gitArgs
if ($LASTEXITCODE -ne 0) { throw "git worktree add failed ($LASTEXITCODE)." }

$target = (Resolve-Path $Path).Path

foreach ($rel in $sharedItems) {
    $source = Join-Path $root $rel
    if (-not (Test-Path $source)) {
        Write-Verbose "Skipping $rel (not present in $root)"
        continue
    }
    $dest = Join-Path $target $rel
    $destDir = Split-Path $dest
    if (-not (Test-Path $destDir)) { New-Item -ItemType Directory -Path $destDir | Out-Null }

    if ($Copy) {
        Copy-Item $source $dest -Recurse -Force
        Write-Information "Copied  $rel" -InformationAction Continue
    }
    else {
        New-Item -ItemType SymbolicLink -Path $dest -Target $source -Force | Out-Null
        Write-Information "Linked  $rel" -InformationAction Continue
    }
}

if (-not $NoSharedMemory) {
    $memory = Join-Path (Get-ClaudeProjectDir $root) 'memory'
    $link = Join-Path (Get-ClaudeProjectDir $target) 'memory'
    if (-not (Test-Path $memory)) { New-Item -ItemType Directory -Path $memory | Out-Null }
    if (Test-Path $link) {
        Write-Warning "$link already exists; leaving it."
    }
    else {
        New-Item -ItemType Directory -Path (Split-Path $link) -Force | Out-Null
        # A junction needs no elevation, unlike a directory symlink.
        New-Item -ItemType Junction -Path $link -Target $memory | Out-Null
        Write-Information "Shared Claude memory with the main checkout" -InformationAction Continue
    }
}

Write-Information "Worktree ready at $target" -InformationAction Continue
if ($Open) { code $target }
