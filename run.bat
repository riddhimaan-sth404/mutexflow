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

echo [*] Waiting for Backend initialization...
timeout /t 2 /nobreak >nul

echo [*] Launching MutexFlow Avalonia Desktop Client (.NET 9)...
cd /d "%~dp0desktop-avalonia"
dotnet run

echo.
echo [*] Application closed.
