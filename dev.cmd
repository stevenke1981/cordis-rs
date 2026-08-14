@echo off
setlocal
if "%1"=="" goto help
if "%1"=="check" goto check
if "%1"=="build" goto build
if "%1"=="test" goto test
if "%1"=="audit" goto audit
goto help
:check
powershell -ExecutionPolicy Bypass -File scripts\check.ps1
exit /b %errorlevel%
:build
cargo build --workspace --release
exit /b %errorlevel%
:test
cargo test --workspace
exit /b %errorlevel%
:audit
python scripts\static_audit.py
exit /b %errorlevel%
:help
echo Usage: dev.cmd check^|build^|test^|audit
exit /b 1
