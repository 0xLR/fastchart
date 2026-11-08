[CmdletBinding()]
param(
    [string]$Remote = "origin",
    [string]$Branch = "gh-pages"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Invoke-Git {
    param(
        [Parameter(Mandatory)]
        [string[]]$Arguments,
        [string]$WorkingDirectory = $repoRoot
    )

    & git -C $WorkingDirectory @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Git failed: git $($Arguments -join ' ')"
    }
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$webRoot = Join-Path $repoRoot "apps/web"
$distRoot = Join-Path $webRoot "dist"

$status = & git -C $repoRoot status --porcelain
if ($LASTEXITCODE -ne 0) {
    throw "FastChart is not inside a readable Git worktree."
}
if ($status) {
    throw "Commit or stash local changes before publishing."
}

Push-Location $webRoot
try {
    & npm run build
    if ($LASTEXITCODE -ne 0) {
        throw "The web build failed."
    }
    & npm run test:wasm
    if ($LASTEXITCODE -ne 0) {
        throw "The WASM smoke test failed."
    }
}
finally {
    Pop-Location
}

$postBuildStatus = & git -C $repoRoot status --porcelain
if ($LASTEXITCODE -ne 0) {
    throw "Could not verify the worktree after building."
}
if ($postBuildStatus) {
    throw "The build changed tracked files. Review and commit them before publishing."
}

if (-not (Test-Path -LiteralPath (Join-Path $distRoot "index.html"))) {
    throw "The build did not produce apps/web/dist/index.html."
}

$remoteUrl = (& git -C $repoRoot remote get-url $Remote).Trim()
if ($LASTEXITCODE -ne 0 -or -not $remoteUrl) {
    throw "Git remote '$Remote' does not exist."
}

$remoteHead = & git -C $repoRoot ls-remote --heads $Remote $Branch
if ($LASTEXITCODE -ne 0) {
    throw "Could not inspect '$Remote/$Branch'."
}

$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$publishRoot = Join-Path $tempRoot ("fastchart-pages-" + [guid]::NewGuid().ToString("N"))
$publishRoot = [System.IO.Path]::GetFullPath($publishRoot)
if (-not $publishRoot.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to use a publishing directory outside the system temp directory."
}

New-Item -ItemType Directory -Path $publishRoot | Out-Null
try {
    Invoke-Git -WorkingDirectory $publishRoot -Arguments @("init", "--initial-branch=$Branch")
    Invoke-Git -WorkingDirectory $publishRoot -Arguments @("remote", "add", "origin", $remoteUrl)

    if ($remoteHead) {
        Invoke-Git -WorkingDirectory $publishRoot -Arguments @("fetch", "--depth=1", "origin", $Branch)
        Invoke-Git -WorkingDirectory $publishRoot -Arguments @("checkout", "-B", $Branch, "FETCH_HEAD")
    }

    Get-ChildItem -LiteralPath $publishRoot -Force |
        Where-Object Name -ne ".git" |
        Remove-Item -Recurse -Force
    Copy-Item -Path (Join-Path $distRoot "*") -Destination $publishRoot -Recurse -Force
    New-Item -ItemType File -Path (Join-Path $publishRoot ".nojekyll") -Force | Out-Null

    $authorName = (& git -C $repoRoot config user.name).Trim()
    $authorEmail = (& git -C $repoRoot config user.email).Trim()
    if (-not $authorName -or -not $authorEmail) {
        throw "Configure Git user.name and user.email before publishing."
    }
    Invoke-Git -WorkingDirectory $publishRoot -Arguments @("config", "user.name", $authorName)
    Invoke-Git -WorkingDirectory $publishRoot -Arguments @("config", "user.email", $authorEmail)
    Invoke-Git -WorkingDirectory $publishRoot -Arguments @("config", "core.autocrlf", "false")
    Invoke-Git -WorkingDirectory $publishRoot -Arguments @("add", "--all")

    $changes = & git -C $publishRoot status --porcelain
    if (-not $changes) {
        Write-Host "GitHub Pages is already up to date."
        return
    }

    $sourceCommit = (& git -C $repoRoot rev-parse --short HEAD).Trim()
    Invoke-Git -WorkingDirectory $publishRoot -Arguments @("commit", "-m", "Deploy FastChart $sourceCommit")
    Invoke-Git -WorkingDirectory $publishRoot -Arguments @("push", "origin", "HEAD:refs/heads/$Branch")
    Write-Host "Published FastChart to $Remote/$Branch."
}
finally {
    if (Test-Path -LiteralPath $publishRoot) {
        $resolvedPublishRoot = [System.IO.Path]::GetFullPath($publishRoot)
        if ($resolvedPublishRoot.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedPublishRoot -Recurse -Force
        }
    }
}
