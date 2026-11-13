@echo off
setlocal enabledelayedexpansion
title MutexFlow Enterprise Launcher

echo ===================================================
echo          MutexFlow Enterprise Launcher
echo ===================================================
echo.

:: 1. Verify Rust/Cargo
where cargo >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo [ERROR] 'cargo' is not found in PATH.
    echo Please install Rust or ensure Cargo is added to PATH.
    pause
    exit /b 1
)

:: 2. Verify .NET SDK
where dotnet >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo [ERROR] 'dotnet' is not found in PATH.
    echo Please install .NET 9 SDK or ensure dotnet is added to PATH.
    pause
    exit /b 1
)

echo [*] Starting MutexFlow Rust Backend on port 3001...
start "MutexFlow Backend (Rust)" /d "%~dp0backend-rust" cargo run

echo [*] Waiting for backend and configured Orchestral runtime...
powershell -NoProfile -Command "$deadline=(Get-Date).AddMinutes(4); while ((Get-Date) -lt $deadline) { try { $r=Invoke-WebRequest -UseBasicParsing -TimeoutSec 2 http://127.0.0.1:3001/health; if ($r.StatusCode -eq 200) { exit 0 } } catch {}; Start-Sleep -Seconds 1 }; exit 1"
if %ERRORLEVEL% neq 0 (
    echo [WARNING] Backend did not become ready within four minutes.
    echo The desktop will still launch; backend-dependent features may be unavailable.
)

echo [*] Launching MutexFlow Avalonia Desktop Client (.NET 9)...
cd /d "%~dp0desktop-avalonia"
dotnet run

echo.
echo [*] Application closed.
