# Standalone installation script for rnpm (Windows)
# Downloads latest release and installs without requiring git clone

param(
    [switch]$Help
)

if ($Help) {
    Write-Host "Standalone installation script for rnpm on Windows"
    Write-Host ""
    Write-Host "Usage:"
    Write-Host "  .\install-standalone.ps1              Install latest rnpm"
    Write-Host "  .\install-standalone.ps1 -Help        Show this help message"
    exit
}

Write-Host "Installing rnpm..."

# Determine architecture
$arch = $env:PROCESSOR_ARCHITECTURE
switch ($arch) {
    "AMD64" { $archName = "x86_64" }
    "ARM64" { $archName = "aarch64" }
    default {
        Write-Host "Error: Unsupported architecture: $arch" -ForegroundColor Red
        exit 1
    }
}

Write-Host "Detected architecture: $archName"

# GitHub repository info
$repo = "r2hu1/rnpm"

# Get latest release
try {
    $latestRelease = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -ErrorAction Stop
    $version = $latestRelease.tag_name
    Write-Host "Latest version: $version"
} catch {
    Write-Host "Warning: Could not fetch latest release information" -ForegroundColor Yellow
    $version = "latest"
}

# Determine binary name
$binaryName = "rnpm-windows-$archName.exe"
$downloadUrl = "https://github.com/$repo/releases/download/$version/$binaryName"

# Create temporary directory
$tempDir = Join-Path $env:TEMP "rnpm-install-$([Guid]::NewGuid())"
New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

try {
    Write-Host "Downloading rnpm..."

    # Try to download pre-built binary
    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile (Join-Path $tempDir "rnpm.exe") -ErrorAction Stop
        Write-Host "Download complete" -ForegroundColor Green
    } catch {
        Write-Host "Pre-built binary not found. Building from source..." -ForegroundColor Yellow

        # Check if Rust is installed
        $rustInstalled = Get-Command cargo -ErrorAction SilentlyContinue
        if (-not $rustInstalled) {
            Write-Host "Error: Rust/Cargo is not installed. Please install from https://rustup.rs/" -ForegroundColor Red
            exit 1
        }

        # Clone and build
        Write-Host "Cloning repository..."
        git clone --depth 1 "https://github.com/$repo.git" (Join-Path $tempDir "src") 2>$null
        if ($LASTEXITCODE -ne 0) {
            Write-Host "Error: Could not clone repository" -ForegroundColor Red
            exit 1
        }

        Set-Location (Join-Path $tempDir "src")
        Write-Host "Building..."
        cargo build --release

        Copy-Item "target\release\rnpm.exe" (Join-Path $tempDir "rnpm.exe")
    }

    # Determine installation directory
    $installDir = Join-Path $env:USERPROFILE ".local\bin"

    if (-not (Test-Path $installDir)) {
        New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    }

    # Copy binary
    Copy-Item (Join-Path $tempDir "rnpm.exe") (Join-Path $installDir "rnpm.exe") -Force

    Write-Host ""
    Write-Host "✓ rnpm installed successfully to $installDir\rnpm.exe" -ForegroundColor Green
    Write-Host ""
    Write-Host "To verify the installation, run:" -ForegroundColor Cyan
    Write-Host "  rnpm --version" -ForegroundColor Cyan
    Write-Host ""

    # Check if in PATH
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($currentPath -notlike "*$installDir*") {
        Write-Host "Note: $installDir is not in your PATH" -ForegroundColor Yellow
        Write-Host "To add it permanently, run this in an Administrator PowerShell:"
        Write-Host "  `$oldPath = [Environment]::GetEnvironmentVariable('Path', 'User')"
        Write-Host "  `$newPath = `$oldPath + ';$installDir'"
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', `$newPath, 'User')"
        Write-Host ""
        Write-Host "Or for current session only:"
        Write-Host "  `$env:Path += `";$installDir`""
    }

} finally {
    # Cleanup
    if (Test-Path $tempDir) {
        Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
    }
}
