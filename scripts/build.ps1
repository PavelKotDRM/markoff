[CmdletBinding()]
param(
    [ValidateSet("All", "Windows", "Linux")]
    [string]$Platform = "All"
)

$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$artifactsDirectory = Join-Path $repositoryRoot "artifacts"
$windowsArtifactsDirectory = Join-Path $artifactsDirectory "windows-x86_64"
$linuxArtifactsDirectory = Join-Path $artifactsDirectory "linux-x86_64"
$dockerImage = "markoff-linux-builder"
$binaries = @("markoff_cli", "markoff_gui")

Set-Location $repositoryRoot

function Build-Windows {
    New-Item -ItemType Directory -Force -Path $windowsArtifactsDirectory | Out-Null
    cargo build --release -p markoff_cli -p markoff_gui
    if ($LASTEXITCODE -ne 0) {
        throw "Windows release build failed."
    }

    foreach ($binary in $binaries) {
        Copy-Item -Force (Join-Path $repositoryRoot "target\release\$binary.exe") `
            (Join-Path $windowsArtifactsDirectory "$binary.exe")
    }
}

function Build-Linux {
    Get-Command docker | Out-Null
    New-Item -ItemType Directory -Force -Path $linuxArtifactsDirectory | Out-Null

    docker build --tag $dockerImage --file (Join-Path $repositoryRoot "docker\linux-build.Dockerfile") $repositoryRoot
    if ($LASTEXITCODE -ne 0) {
        throw "Could not create the Linux build image."
    }

    $repositoryMount = "$($repositoryRoot.Replace('\', '/')):/workspace"
    $artifactsMount = "$($artifactsDirectory.Replace('\', '/')):/artifacts"
    docker run --rm `
        --volume $repositoryMount `
        --volume $artifactsMount `
        --workdir /workspace `
        --env CARGO_TARGET_DIR=/tmp/markoff-target `
        $dockerImage `
        bash -c 'cargo build --release -p markoff_cli -p markoff_gui && cp /tmp/markoff-target/release/markoff_cli /tmp/markoff-target/release/markoff_gui /artifacts/linux-x86_64/'
    if ($LASTEXITCODE -ne 0) {
        throw "Linux release build failed."
    }
}

if ($Platform -in @("All", "Windows")) {
    Build-Windows
}

if ($Platform -in @("All", "Linux")) {
    Build-Linux
}