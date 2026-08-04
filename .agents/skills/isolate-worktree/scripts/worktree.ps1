[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet("create", "integrate", "cleanup", "status")]
    [string]$Action,

    [string]$TaskSlug,
    [string]$Branch,
    [string]$MergeMessage,
    [string]$RepoPath = ".",
    [string]$BaseBranch = "main",
    [string]$BranchPrefix = "codex"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Invoke-Git {
    param(
        [Parameter(Mandatory = $true)]
        [string]$WorkingDirectory,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    $previousErrorAction = $ErrorActionPreference
    try {
        # Windows PowerShell turns native stderr into ErrorRecord objects. Git
        # writes normal progress there, so judge success only by its exit code.
        $ErrorActionPreference = "Continue"
        $output = @(& git -C $WorkingDirectory @Arguments 2>&1)
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorAction
    }

    [pscustomobject]@{
        ExitCode = $exitCode
        Output   = @($output | ForEach-Object { $_.ToString() })
    }
}

function Assert-GitSucceeded {
    param(
        [Parameter(Mandatory = $true)]
        [pscustomobject]$Result,

        [Parameter(Mandatory = $true)]
        [string]$Context
    )

    if ($Result.ExitCode -eq 0) {
        return
    }

    $details = ($Result.Output -join [Environment]::NewLine).Trim()
    if ([string]::IsNullOrWhiteSpace($details)) {
        $details = "Git exited with code $($Result.ExitCode)."
    }

    throw "$Context failed.`n$details"
}

function Get-RepositoryRoot {
    param([string]$Path)

    $result = Invoke-Git -WorkingDirectory $Path -Arguments @("rev-parse", "--show-toplevel")
    Assert-GitSucceeded -Result $result -Context "Resolve repository root"
    return [IO.Path]::GetFullPath(($result.Output | Select-Object -Last 1).Trim())
}

function Get-WorktreeRecords {
    param([string]$RepositoryRoot)

    $result = Invoke-Git -WorkingDirectory $RepositoryRoot -Arguments @("worktree", "list", "--porcelain")
    Assert-GitSucceeded -Result $result -Context "List worktrees"

    $records = [Collections.Generic.List[object]]::new()
    $current = $null

    foreach ($line in @($result.Output) + "") {
        if ([string]::IsNullOrWhiteSpace($line)) {
            if ($null -ne $current) {
                $records.Add([pscustomobject]$current)
                $current = $null
            }
            continue
        }

        if ($line.StartsWith("worktree ")) {
            if ($null -ne $current) {
                $records.Add([pscustomobject]$current)
            }
            $current = [ordered]@{
                Path     = [IO.Path]::GetFullPath($line.Substring(9))
                Branch   = $null
                Detached = $false
            }
            continue
        }

        if ($null -eq $current) {
            continue
        }

        if ($line.StartsWith("branch refs/heads/")) {
            $current.Branch = $line.Substring(18)
        }
        elseif ($line -eq "detached") {
            $current.Detached = $true
        }
    }

    return @($records)
}

function Get-PrimaryWorktree {
    param(
        [string]$RepositoryRoot,
        [string]$ExpectedBranch
    )

    $matches = @(
        Get-WorktreeRecords -RepositoryRoot $RepositoryRoot |
            Where-Object { $_.Branch -eq $ExpectedBranch }
    )

    if ($matches.Count -ne 1) {
        throw "Expected exactly one worktree with branch '$ExpectedBranch'; found $($matches.Count)."
    }

    return $matches[0]
}

function Assert-CleanWorktree {
    param(
        [string]$Path,
        [string]$Label
    )

    $result = Invoke-Git -WorkingDirectory $Path -Arguments @("status", "--porcelain=v1", "--untracked-files=normal")
    Assert-GitSucceeded -Result $result -Context "Inspect $Label"

    if ($result.Output.Count -gt 0) {
        $details = $result.Output -join [Environment]::NewLine
        throw "$Label is not clean. Refusing to continue.`n$details"
    }
}

function Test-LocalBranch {
    param(
        [string]$RepositoryRoot,
        [string]$Name
    )

    $result = Invoke-Git -WorkingDirectory $RepositoryRoot -Arguments @(
        "show-ref", "--verify", "--quiet", "refs/heads/$Name"
    )
    return $result.ExitCode -eq 0
}

function Assert-TaskBranchName {
    param(
        [string]$Name,
        [string]$ExpectedPrefix
    )

    if ([string]::IsNullOrWhiteSpace($Name)) {
        throw "-Branch is required for this action."
    }

    if (-not $Name.StartsWith("$ExpectedPrefix/", [StringComparison]::Ordinal)) {
        throw "Task branch '$Name' must use the '$ExpectedPrefix/' prefix."
    }
}

function Write-Result {
    param([Collections.IDictionary]$Value)

    [pscustomobject]$Value | ConvertTo-Json -Depth 5
}

if ($null -eq (Get-Command git -ErrorAction SilentlyContinue)) {
    throw "Git is required but was not found on PATH."
}

if ($BaseBranch -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._/-]*$') {
    throw "-BaseBranch is not a valid simple branch name."
}

if ($BranchPrefix -cnotmatch '^[a-z0-9][a-z0-9-]*$') {
    throw "-BranchPrefix must contain lowercase ASCII letters, digits, or hyphens."
}

$repositoryRoot = Get-RepositoryRoot -Path $RepoPath

if (-not (Test-LocalBranch -RepositoryRoot $repositoryRoot -Name $BaseBranch)) {
    throw "Local base branch '$BaseBranch' does not exist."
}

$primary = Get-PrimaryWorktree -RepositoryRoot $repositoryRoot -ExpectedBranch $BaseBranch

switch ($Action) {
    "status" {
        $worktrees = @(
            Get-WorktreeRecords -RepositoryRoot $repositoryRoot |
                Where-Object {
                    $_.Branch -eq $BaseBranch -or
                    ($null -ne $_.Branch -and $_.Branch.StartsWith("$BranchPrefix/"))
                }
        )

        Write-Result -Value ([ordered]@{
            action     = "status"
            baseBranch = $BaseBranch
            primary    = $primary.Path
            worktrees  = $worktrees
        })
        break
    }

    "create" {
        if ($TaskSlug -cnotmatch '^[a-z0-9](?:[a-z0-9-]{0,46}[a-z0-9])?$') {
            throw "-TaskSlug must be 1-48 lowercase ASCII letters, digits, or hyphens, without a trailing hyphen."
        }

        Assert-CleanWorktree -Path $primary.Path -Label "Primary '$BaseBranch' worktree"

        $suffix = ([guid]::NewGuid().ToString("N")).Substring(0, 8)
        $branchName = "$BranchPrefix/$TaskSlug-$suffix"
        $primaryInfo = [IO.DirectoryInfo]::new($primary.Path)
        $worktreeRoot = Join-Path $primaryInfo.Parent.FullName "$($primaryInfo.Name)-worktrees"
        $worktreePath = Join-Path $worktreeRoot "$TaskSlug-$suffix"

        New-Item -ItemType Directory -Force -Path $worktreeRoot | Out-Null

        $result = Invoke-Git -WorkingDirectory $primary.Path -Arguments @(
            "worktree", "add", "-b", $branchName, $worktreePath, $BaseBranch
        )
        Assert-GitSucceeded -Result $result -Context "Create task worktree"

        Write-Result -Value ([ordered]@{
            action     = "create"
            baseBranch = $BaseBranch
            branch     = $branchName
            worktree   = $worktreePath
            primary    = $primary.Path
        })
        break
    }

    "integrate" {
        Assert-TaskBranchName -Name $Branch -ExpectedPrefix $BranchPrefix
        if ([string]::IsNullOrWhiteSpace($MergeMessage)) {
            throw "-MergeMessage is required for integration."
        }
        if (-not (Test-LocalBranch -RepositoryRoot $repositoryRoot -Name $Branch)) {
            throw "Local task branch '$Branch' does not exist."
        }

        Assert-CleanWorktree -Path $primary.Path -Label "Primary '$BaseBranch' worktree"

        $taskWorktrees = @(
            Get-WorktreeRecords -RepositoryRoot $repositoryRoot |
                Where-Object { $_.Branch -eq $Branch }
        )
        foreach ($taskWorktree in $taskWorktrees) {
            Assert-CleanWorktree -Path $taskWorktree.Path -Label "Task '$Branch' worktree"
        }

        $ancestor = Invoke-Git -WorkingDirectory $primary.Path -Arguments @(
            "merge-base", "--is-ancestor", "refs/heads/$Branch", "refs/heads/$BaseBranch"
        )
        if ($ancestor.ExitCode -eq 0) {
            Write-Result -Value ([ordered]@{
                action        = "integrate"
                branch        = $Branch
                baseBranch    = $BaseBranch
                alreadyMerged = $true
                primary       = $primary.Path
            })
            break
        }
        if ($ancestor.ExitCode -ne 1) {
            Assert-GitSucceeded -Result $ancestor -Context "Check integration ancestry"
        }

        $result = Invoke-Git -WorkingDirectory $primary.Path -Arguments @(
            "merge", "--no-ff", "-m", $MergeMessage, $Branch
        )
        if ($result.ExitCode -ne 0) {
            $details = ($result.Output -join [Environment]::NewLine).Trim()
            throw "Merge failed. Preserve the task worktree and deliberately resolve or abort the merge in '$($primary.Path)'.`n$details"
        }

        $head = Invoke-Git -WorkingDirectory $primary.Path -Arguments @("rev-parse", "HEAD")
        Assert-GitSucceeded -Result $head -Context "Read integrated HEAD"

        Write-Result -Value ([ordered]@{
            action        = "integrate"
            branch        = $Branch
            baseBranch    = $BaseBranch
            alreadyMerged = $false
            commit        = ($head.Output | Select-Object -Last 1).Trim()
            primary       = $primary.Path
        })
        break
    }

    "cleanup" {
        Assert-TaskBranchName -Name $Branch -ExpectedPrefix $BranchPrefix
        if (-not (Test-LocalBranch -RepositoryRoot $repositoryRoot -Name $Branch)) {
            throw "Local task branch '$Branch' does not exist."
        }

        Assert-CleanWorktree -Path $primary.Path -Label "Primary '$BaseBranch' worktree"

        $ancestor = Invoke-Git -WorkingDirectory $primary.Path -Arguments @(
            "merge-base", "--is-ancestor", "refs/heads/$Branch", "refs/heads/$BaseBranch"
        )
        if ($ancestor.ExitCode -ne 0) {
            if ($ancestor.ExitCode -eq 1) {
                throw "Task branch '$Branch' is not fully contained in '$BaseBranch'. Refusing cleanup."
            }
            Assert-GitSucceeded -Result $ancestor -Context "Check cleanup ancestry"
        }

        $taskWorktrees = @(
            Get-WorktreeRecords -RepositoryRoot $repositoryRoot |
                Where-Object { $_.Branch -eq $Branch }
        )
        foreach ($taskWorktree in $taskWorktrees) {
            Assert-CleanWorktree -Path $taskWorktree.Path -Label "Task '$Branch' worktree"
            Set-Location $primary.Path
            $remove = Invoke-Git -WorkingDirectory $primary.Path -Arguments @(
                "worktree", "remove", $taskWorktree.Path
            )
            Assert-GitSucceeded -Result $remove -Context "Remove task worktree"
        }

        $delete = Invoke-Git -WorkingDirectory $primary.Path -Arguments @(
            "branch", "-d", "--", $Branch
        )
        Assert-GitSucceeded -Result $delete -Context "Delete merged task branch"

        $prune = Invoke-Git -WorkingDirectory $primary.Path -Arguments @("worktree", "prune")
        Assert-GitSucceeded -Result $prune -Context "Prune worktree metadata"

        Write-Result -Value ([ordered]@{
            action     = "cleanup"
            branch     = $Branch
            baseBranch = $BaseBranch
            removed    = $true
            primary    = $primary.Path
        })
        break
    }
}
