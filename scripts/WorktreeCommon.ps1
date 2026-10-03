# Dot-sourced by the worktree scripts.

# Claude Code keeps per-directory data (memory, transcripts) under ~/.claude/projects in a folder
# named after the path with every character other than letters, digits and dashes replaced by a
# dash.
function Get-ClaudeProjectDir([string] $Directory) {
    $key = (Resolve-Path $Directory).Path.TrimEnd('\', '/') -replace '[^A-Za-z0-9-]', '-'
    return Join-Path -Path $HOME -ChildPath '.claude' -AdditionalChildPath 'projects', $key
}

# The main checkout's root, from anywhere inside it or one of its worktrees.
function Get-MainRoot {
    $commonDir = (git rev-parse --git-common-dir).Trim()
    if (-not $commonDir) { throw 'Not inside a git repository.' }
    return Split-Path (Resolve-Path $commonDir).Path
}

# The default worktree folder for a branch: ..\<repo>.wt\<branch with slashes as dashes>.
function Get-WorktreePath([string] $Root, [string] $Branch) {
    $name = $Branch -replace '[/\\]', '-'
    return Join-Path -Path (Split-Path $Root) -ChildPath "$(Split-Path -Leaf $Root).wt" -AdditionalChildPath $name
}
