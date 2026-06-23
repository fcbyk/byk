$ErrorActionPreference = "Stop"

$InstallDir = "$env:USERPROFILE\.byk\bin"
$Url = "https://github.com/fcbyk/byk/releases/latest/download/byk-windows-x64.zip"
$TempDir = Join-Path $env:TEMP "byk-install"

Write-Host "Installing byk..."
Write-Host "Downloading from $Url..."

Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $TempDir | Out-Null
Invoke-WebRequest -Uri $Url -OutFile "$TempDir\byk.zip"

Write-Host "Extracting..."
Expand-Archive -Path "$TempDir\byk.zip" -DestinationPath $TempDir -Force

Write-Host "Installing to $InstallDir\byk.exe..."
New-Item -ItemType Directory -Force $InstallDir | Out-Null
Copy-Item -Path "$TempDir\byk.exe" -Destination "$InstallDir\byk.exe" -Force

# Add to user PATH if not already present
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User") ?? ""
if ($UserPath -notlike "*\.byk\bin*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    Write-Host "Added $InstallDir to user PATH"
}

Write-Host ""
Write-Host "byk installed successfully!"
Write-Host "Restart your terminal, then run: byk --version"

Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue