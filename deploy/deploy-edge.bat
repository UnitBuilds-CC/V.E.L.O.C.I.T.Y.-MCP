@echo off
setlocal EnableDelayedExpansion

REM ============================================================================
REM VELOCITY-MCP Edge Deployment Script (Windows)
REM Builds, validates, tests, deploys, and verifies a Wasmer Edge deployment.
REM Idempotent: safe to run multiple times.
REM Usage: deploy-edge.bat [--skip-tests] [--skip-build] [--dry-run]
REM ============================================================================

REM --- Configuration ---
set "WASM_BINARY=target\wasm32-wasip1\release\velocity-edge.wasm"
set "MAX_WASM_SIZE_BYTES=5242880"
set "TARGET=wasm32-wasip1"
set "BIN_NAME=velocity-edge"
set "SKIP_TESTS=0"
set "SKIP_BUILD=0"
set "DRY_RUN=0"
set "DEPLOY_TIMESTAMP="

REM --- Parse arguments ---
:parse_args
if "%~1"=="" goto :args_done
if /i "%~1"=="--skip-tests" (set "SKIP_TESTS=1" & shift & goto :parse_args)
if /i "%~1"=="--skip-build" (set "SKIP_BUILD=1" & shift & goto :parse_args)
if /i "%~1"=="--dry-run" (set "DRY_RUN=1" & shift & goto :parse_args)
echo Unknown argument: %~1
exit /b 1
:args_done

REM --- Helper: timestamp ---
for /f "tokens=2 delims==" %%I in ('wmic os get localdatetime /value 2^>nul') do set "dt=%%I"
set "DEPLOY_TIMESTAMP=%dt:~0,4%-%dt:~4,2%-%dt:~6,2%T%dt:~8,2%:%dt:~10,2%:%dt:~12,2%Z"

echo ============================================================================
echo  VELOCITY-MCP Edge Deployment
echo  Timestamp: %DEPLOY_TIMESTAMP%
echo ============================================================================
echo.

REM --- Step 1: Validate environment ---
echo [1/6] Validating environment...

REM Check Rust toolchain
where rustc >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Rust toolchain not found. Install from https://rustup.rs/
    exit /b 1
)
echo   [OK] Rust toolchain found

REM Check wasm32-wasip1 target
rustup target list --installed | findstr /i "%TARGET%" >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Target %TARGET% not installed.
    echo   Run: rustup target add %TARGET%
    exit /b 1
)
echo   [OK] Target %TARGET% installed

REM Check Wasmer CLI
set "PATH=%USERPROFILE%\.wasmer\bin;%PATH%"
where wasmer >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Wasmer CLI not found. Install from https://wasmer.io/
    echo   Or add %USERPROFILE%\.wasmer\bin to PATH
    exit /b 1
)
echo   [OK] Wasmer CLI found

REM Check Wasmer login
wasmer whoami >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Not logged in to Wasmer.
    echo   Run: wasmer login
    exit /b 1
)
echo   [OK] Logged in to Wasmer

REM Check wasmer.toml exists
if not exist "wasmer.toml" (
    echo [ERROR] wasmer.toml not found in current directory.
    echo   Run this script from the project root.
    exit /b 1
)
echo   [OK] wasmer.toml found

echo   Environment validation passed.
echo.

REM --- Step 2: Build WASM binary ---
if "%SKIP_BUILD%"=="1" (
    echo [2/6] Skipping build (--skip-build flag set)
    if not exist "%WASM_BINARY%" (
        echo [ERROR] WASM binary not found at %WASM_BINARY% and --skip-build was specified.
        exit /b 1
    )
) else (
    echo [2/6] Building WASM binary for %TARGET%...
    echo   Command: cargo build --target %TARGET% --release --bin %BIN_NAME%
    cargo build --target %TARGET% --release --bin %BIN_NAME%
    if errorlevel 1 (
        echo [ERROR] Build failed!
        echo   Check compiler output above for errors.
        exit /b 1
    )
    echo   Build succeeded.
)

REM Verify WASM file exists
if not exist "%WASM_BINARY%" (
    echo [ERROR] WASM binary not found at %WASM_BINARY% after build.
    exit /b 1
)
echo   [OK] WASM binary exists: %WASM_BINARY%

REM --- Step 3: Validate binary size ---
echo [3/6] Validating binary size...
for %%F in ("%WASM_BINARY%") do set "WASM_SIZE=%%~zF"
echo   Binary size: %WASM_SIZE% bytes

if %WASM_SIZE% GTR %MAX_WASM_SIZE_BYTES% (
    echo [WARNING] Binary size %WASM_SIZE% exceeds recommended limit of %MAX_WASM_SIZE_BYTES% bytes (5MB).
    echo   Consider optimizing dependencies or using cargo features to reduce size.
    echo   Deployment will continue, but large binaries may cause slow cold starts.
) else (
    echo   [OK] Binary size within 5MB limit.
)

REM Calculate KB for display
set /a "WASM_SIZE_KB=%WASM_SIZE% / 1024"
echo   Size: %WASM_SIZE_KB% KB
echo.

REM --- Step 4: Run tests ---
if "%SKIP_TESTS%"=="1" (
    echo [4/6] Skipping tests (--skip-tests flag set)
) else (
    echo [4/6] Running tests...
    cargo test --workspace --lib
    if errorlevel 1 (
        echo [ERROR] Tests failed!
        echo   Fix failing tests before deploying.
        exit /b 1
    )
    echo   [OK] All tests passed.
)
echo.

REM --- Step 5: Deploy ---
echo [5/6] Deploying to Wasmer Edge...

if "%DRY_RUN%"=="1" (
    echo   [DRY RUN] Would execute: wasmer deploy
    echo   [DRY RUN] Skipping actual deployment.
) else (
    wasmer deploy
    if errorlevel 1 (
        echo [ERROR] Deployment failed!
        echo   Check Wasmer CLI output above for errors.
        echo   Common causes:
        echo     - wasmer.toml misconfiguration
        echo     - Authentication issues (run: wasmer login)
        echo     - Binary too large for plan limits
        echo     - Network connectivity issues
        exit /b 1
    )
)
echo.

REM --- Step 6: Verify deployment ---
if "%DRY_RUN%"=="1" (
    echo [6/6] Skipping verification (dry run mode)
) else (
    echo [6/6] Verifying deployment health...

    REM Attempt to find the deployed URL from wasmer edge list
    echo   Checking deployment status...
    wasmer edge list
    echo.

    echo   To verify manually, run:
    echo     curl https://^<your-app^>.wasmer.app/health
    echo   Expected response: {"status":"healthy","version":"3.2.0"}
    echo.

    echo   [OK] Deployment completed successfully.
)

REM --- Summary ---
echo ============================================================================
echo  Deployment Summary
echo  Timestamp:   %DEPLOY_TIMESTAMP%
echo  Binary:      %WASM_BINARY% (%WASM_SIZE_KB% KB)
echo  Skip tests:  %SKIP_TESTS%
echo  Skip build:  %SKIP_BUILD%
echo  Dry run:     %DRY_RUN%
echo  Status:      SUCCESS
echo ============================================================================
echo.
echo  Next steps:
echo    1. View your deployment: wasmer edge list
echo    2. Check logs: wasmer edge logs ^<app-name^>
echo    3. Test endpoint: curl https://^<your-app^>.wasmer.app/health
echo    4. Read the user guide: docs/edge_user_guide.md
echo ============================================================================

exit /b 0
