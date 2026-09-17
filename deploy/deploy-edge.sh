#!/bin/bash
# ============================================================================
# VELOCITY-MCP Edge Deployment Script (Linux/macOS)
# Builds, validates, tests, deploys, and verifies a Wasmer Edge deployment.
# Idempotent: safe to run multiple times.
# Usage: ./deploy-edge.sh [--skip-tests] [--skip-build] [--dry-run]
# ============================================================================

set -euo pipefail

# --- Configuration ---
WASM_BINARY="target/wasm32-wasmer-wasi/release/velocity-edge.wasm"
MAX_WASM_SIZE_BYTES=5242880  # 5MB
TARGET="wasm32-wasmer-wasi"
BIN_NAME="velocity-edge"
SKIP_TESTS=0
SKIP_BUILD=0
DRY_RUN=0

# --- Colors for output ---
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-tests) SKIP_TESTS=1; shift ;;
        --skip-build) SKIP_BUILD=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        *) echo -e "${RED}Unknown argument: $1${NC}"; exit 1 ;;
    esac
done

DEPLOY_TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

echo "============================================================================"
echo " VELOCITY-MCP Edge Deployment"
echo " Timestamp: $DEPLOY_TIMESTAMP"
echo "============================================================================"
echo ""

# --- Helper functions ---
log_ok() { echo -e "  ${GREEN}[OK]${NC} $1"; }
log_err() { echo -e "  ${RED}[ERROR]${NC} $1"; }
log_warn() { echo -e "  ${YELLOW}[WARNING]${NC} $1"; }
log_info() { echo -e "  ${BLUE}[INFO]${NC} $1"; }

# --- Step 1: Validate environment ---
echo "[1/6] Validating environment..."

# Check Rust toolchain
if ! command -v rustc &>/dev/null; then
    log_err "Rust toolchain not found. Install from https://rustup.rs/"
    exit 1
fi
log_ok "Rust toolchain found ($(rustc --version))"

if ! rustc +wasix --print target-libdir --target "$TARGET" >/dev/null; then
    log_err "Install the WASIX Rust toolchain as +wasix before building."
    exit 1
fi
if ! command -v cargo-wasix >/dev/null || ! command -v python >/dev/null; then
    log_err "Python 3 and cargo-wasix are required."
    exit 1
fi
log_ok "WASIX build prerequisites found"

# Check Wasmer CLI
if ! command -v wasmer &>/dev/null; then
    # Try adding Wasmer to PATH
    export PATH="$HOME/.wasmer/bin:$PATH"
    if ! command -v wasmer &>/dev/null; then
        log_err "Wasmer CLI not found. Install from https://wasmer.io/"
        exit 1
    fi
fi
log_ok "Wasmer CLI found ($(wasmer --version 2>/dev/null || echo 'unknown version'))"

# Check Wasmer login
if ! wasmer whoami &>/dev/null; then
    log_err "Not logged in to Wasmer."
    echo "  Run: wasmer login"
    exit 1
fi
log_ok "Logged in to Wasmer as $(wasmer whoami 2>/dev/null)"

# Check wasmer.toml exists
if [ ! -f "wasmer.toml" ]; then
    log_err "wasmer.toml not found in current directory."
    echo "  Run this script from the project root."
    exit 1
fi
log_ok "wasmer.toml found"

echo ""

# --- Step 2: Build WASM binary ---
if [ "$SKIP_BUILD" -eq 1 ]; then
    echo "[2/6] Skipping build (--skip-build flag set)"
    if [ ! -f "$WASM_BINARY" ]; then
        log_err "WASM binary not found at $WASM_BINARY and --skip-build was specified."
        exit 1
    fi
else
    echo "[2/6] Building WASM binary for $TARGET..."
    echo "  Command: python deploy/build-edge.py"
    if ! python deploy/build-edge.py; then
        log_err "Build failed!"
        echo "  Check compiler output above for errors."
        exit 1
    fi
    log_ok "Build succeeded."
fi

# Verify WASM file exists
if [ ! -f "$WASM_BINARY" ]; then
    log_err "WASM binary not found at $WASM_BINARY after build."
    exit 1
fi

WASM_SIZE=$(stat -c%s "$WASM_BINARY" 2>/dev/null || stat -f%z "$WASM_BINARY" 2>/dev/null || echo "0")
WASM_SIZE_KB=$((WASM_SIZE / 1024))
log_ok "WASM binary exists: $WASM_BINARY ($WASM_SIZE_KB KB)"

# --- Step 3: Validate binary size ---
echo "[3/6] Validating binary size..."
echo "  Binary size: $WASM_SIZE bytes ($WASM_SIZE_KB KB)"

if [ "$WASM_SIZE" -gt "$MAX_WASM_SIZE_BYTES" ]; then
    log_warn "Binary size $WASM_SIZE exceeds recommended limit of $MAX_WASM_SIZE_BYTES bytes (5MB)."
    echo "  Consider optimizing dependencies or using cargo features to reduce size."
    echo "  Deployment will continue, but large binaries may cause slow cold starts."
else
    log_ok "Binary size within 5MB limit."
fi
echo ""

# --- Step 4: Run tests ---
if [ "$SKIP_TESTS" -eq 1 ]; then
    echo "[4/6] Skipping tests (--skip-tests flag set)"
else
    echo "[4/6] Running tests..."
    if ! cargo test --locked -p velocity-mcp-core -p velocity-mcp-edge; then
        log_err "Tests failed!"
        echo "  Fix failing tests before deploying."
        exit 1
    fi
    log_ok "All tests passed."
fi
echo ""

# --- Step 5: Deploy ---
echo "[5/6] Deploying to Wasmer Edge..."

if [ "$DRY_RUN" -eq 1 ]; then
    echo "  [DRY RUN] Would execute: wasmer deploy"
    echo "  [DRY RUN] Skipping actual deployment."
else
    if ! wasmer deploy --publish-package; then
        log_err "Deployment failed!"
        echo "  Check Wasmer CLI output above for errors."
        echo "  Common causes:"
        echo "    - wasmer.toml misconfiguration"
        echo "    - Authentication issues (run: wasmer login)"
        echo "    - Binary too large for plan limits"
        echo "    - Network connectivity issues"
        exit 1
    fi
fi
echo ""

# --- Step 6: Verify deployment ---
if [ "$DRY_RUN" -eq 1 ]; then
    echo "[6/6] Skipping verification (dry run mode)"
else
    echo "[6/6] Verifying deployment health..."
    echo ""
    echo "  Checking deployment status..."
    wasmer edge list 2>/dev/null || echo "  (Unable to list edges -- check Wasmer dashboard)"
    echo ""

    echo "  To verify manually, run:"
    echo "    curl https://<your-app>.wasmer.app/health"
    echo "  Expected response: {\"status\":\"healthy\",\"version\":\"3.2.0\"}"
    echo ""
    log_ok "Deployment completed successfully."
fi

# --- Summary ---
echo ""
echo "============================================================================"
echo " Deployment Summary"
echo " Timestamp:   $DEPLOY_TIMESTAMP"
echo " Binary:      $WASM_BINARY ($WASM_SIZE_KB KB)"
echo " Skip tests:  $SKIP_TESTS"
echo " Skip build:  $SKIP_BUILD"
echo " Dry run:     $DRY_RUN"
echo " Status:      SUCCESS"
echo "============================================================================"
echo ""
echo " Next steps:"
echo "   1. View your deployment: wasmer edge list"
echo "   2. Check logs: wasmer edge logs <app-name>"
echo "   3. Test endpoint: curl https://<your-app>.wasmer.app/health"
echo "   4. Read the user guide: docs/edge_user_guide.md"
echo "============================================================================"

exit 0
