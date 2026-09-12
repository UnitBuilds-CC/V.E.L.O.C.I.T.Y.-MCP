@echo off
REM Deploy VELOCITY-MCP to Wasmer Edge (free tier optimized)
REM Usage: deploy-edge.bat

setlocal

REM Add Wasmer to PATH
set PATH=%USERPROFILE%\.wasmer\bin;%PATH%

REM Check if logged in
wasmer whoami >nul 2>&1
if errorlevel 1 (
    echo Not logged in to Wasmer. Please login first:
    echo   wasmer login
    echo.
    exit /b 1
)

REM Build WASM target
echo Building WASM binary for wasm32-wasip1...
cargo build --target wasm32-wasip1 --release --bin velocity-edge
if errorlevel 1 (
    echo Build failed!
    exit /b 1
)

REM Verify WASM file exists
if not exist "target\wasm32-wasip1\release\velocity_edge.wasm" (
    echo WASM binary not found after build!
    exit /b 1
)

REM Show WASM file size
for %%F in ("target\wasm32-wasip1\release\velocity_edge.wasm") do (
    set WASM_SIZE=%%~zF
)
echo WASM binary size: %WASM_SIZE% bytes

REM Deploy to Wasmer Edge
echo.
echo Deploying to Wasmer Edge (free tier)...
wasmer deploy

echo.
echo Deployment complete! Check your Wasmer dashboard for the endpoint URL.
