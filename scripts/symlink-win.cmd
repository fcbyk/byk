@echo off
rem Install byk for local development: symlink target\debug\byk.exe into %USERPROFILE%\.byk\bin.
rem Run from anywhere. Requires Developer Mode or an elevated shell for mklink.
setlocal

set "BIN=%USERPROFILE%\.byk\bin"
set "TARGET="

rem Resolve the build output to an absolute path (%~dp0 is this script's dir, with trailing slash).
for %%I in ("%~dp0..\target\debug\byk.exe") do set "TARGET=%%~fI"

if not exist "%TARGET%" (
    echo [error] Build output not found: %TARGET%
    echo [error] Run "cargo build" first.
    exit /b 1
)

if not exist "%BIN%" mkdir "%BIN%"

if exist "%BIN%\byk.exe" del "%BIN%\byk.exe"

mklink "%BIN%\byk.exe" "%TARGET%"
if errorlevel 1 (
    echo [error] Failed to create the symlink.
    echo [error] Enable Developer Mode or run from an elevated shell.
    exit /b 1
)

echo Installed: %BIN%\byk.exe -^> %TARGET%
endlocal
