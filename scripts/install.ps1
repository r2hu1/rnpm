# Installation script for rnpm (Windows)

param(
    [switch]$Help
)

if ($Help) {
    Write-Host "Installation script for rnpm on Windows"
    Write-Host ""
    Write-Host "Usage:"
    Write-Host "  .\install.ps1              Install rnpm to user's local bin"
    Write-Host "  .\install.ps1 -Help        Show this help message"
    exit
}

Write-Host "Installing rnpm..."

# Check if Rust is installed
$rustInstalled = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $rustInstalled) {
    Write-Host "Error: Rust/Cargo is not installed. Please install from https://rustup.rs/" -ForegroundColor Red
    exit 1
}

# Build the project
Write-Host "Building rnpm..."
try {
    cargo build --release
} catch {
    Write-Host "Error building rnpm: $_" -ForegroundColor Red
    exit 1
}

# Determine installation directory
$installDir = Join-Path $env:USERPROFILE ".local\bin"

# Create installation directory if it doesn't exist
if (-not (Test-Path $installDir)) {
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
}

# Copy binary
$binaryPath = "target\release\rnpm.exe"
if (Test-Path $binaryPath) {
    Copy-Item $binaryPath (Join-Path $installDir "rnpm.exe") -Force
    Write-Host "rnpm installed successfully to $installDir\rnpm.exe" -ForegroundColor Green
} else {
    Write-Host "Error: Binary not found at $binaryPath" -ForegroundColor Red
    exit 1
}

# Check if installation directory is in PATH
$currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($currentPath -notlike "*$installDir*") {
    Write-Host ""
    Write-Host "Note: $installDir is not in your PATH" -ForegroundColor Yellow
    Write-Host "To add it permanently, run this in an Administrator PowerShell:"
    Write-Host "  `$oldPath = [Environment]::GetEnvironmentVariable('Path', 'User')"
    Write-Host "  `$newPath = `$oldPath + ';$installDir'"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', `$newPath, 'User')"
    Write-Host ""
    Write-Host "Or for current session only:"
    Write-Host "  `$env:Path += `";$installDir`""
} else {
    Write-Host ""
    Write-Host "To verify the installation, run:" -ForegroundColor Green
    Write-Host "  rnpm --version" -ForegroundColor Cyan
}
