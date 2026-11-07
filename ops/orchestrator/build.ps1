# Build script for the RFS server supervisor (Windows PowerShell).
# Requires Go 1.22+. No external dependencies.

$ErrorActionPreference = "Stop"
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
Set-Location $scriptDir

function Build {
    Write-Host "Building rfs-orchestrator..." -ForegroundColor Cyan
    go build -o rfs-orchestrator.exe .
    Write-Host "Build complete: rfs-orchestrator.exe" -ForegroundColor Green
}

function Test {
    Write-Host "Running tests..." -ForegroundColor Cyan
    go test -v ./...
}

function Vet {
    Write-Host "Running go vet..." -ForegroundColor Cyan
    go vet ./...
}

function Clean {
    Write-Host "Cleaning build artifacts..." -ForegroundColor Cyan
    if (Test-Path "rfs-orchestrator.exe") { Remove-Item "rfs-orchestrator.exe" -Force }
    if (Test-Path "profiles") { Remove-Item "profiles" -Recurse -Force }
    Write-Host "Clean complete." -ForegroundColor Green
}

function Run {
    param(
        [Parameter(Mandatory=$true)]
        [string]$Server
    )
    Build
    Write-Host "Starting profile 'default' with server: $Server" -ForegroundColor Cyan
    .\rfs-orchestrator.exe --server $Server start default
}

switch ($args[0]) {
    "build"   { Build }
    "test"    { Test }
    "vet"     { Vet }
    "clean"   { Clean }
    "run"     { if ($args[1]) { Run -Server $args[1] } else { Write-Host "Usage: .\build.ps1 run <path-to-server>" -ForegroundColor Yellow } }
    default   {
        Write-Host "RFS Supervisor build script" -ForegroundColor Cyan
        Write-Host "Usage: .\build.ps1 <build|test|vet|clean|run [server-path]>" -ForegroundColor Yellow
    }
}
