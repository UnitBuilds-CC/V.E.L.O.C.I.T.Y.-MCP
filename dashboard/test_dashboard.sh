#!/usr/bin/env bash
# Headless E2E test for the V.E.L.O.C.I.T.Y. MCP dashboard.
#
# Renders dashboard/index.html in headless Chrome with the auto-demo flow
# (connect -> echo -> js_calculator -> 50-ping burst) and asserts on the
# serialized DOM. Also checks the unreachable-server error path.
#
# Usage:
#   1. Start the server:  target/release/velocity_mcp --mode http --config dashboard/demo.toml
#   2. Run:               bash dashboard/test_dashboard.sh
#
# Step 1 is not optional in spirit: demo.toml sets [http] cors_origins = ["*"], and this
# page is opened as a file:// document, so its Origin is "null" and the server's default
# predicate (src/transport/http.rs:1508, localhost-only) omits access-control-allow-origin
# entirely. Measured 2026-10-11 on one build: bare --mode http gives 6/14 with eight
# "missing:" DOM failures, the demo.toml server gives 14/0. Those eight are CORS, not
# regressions -- check the config before bisecting them.
#
# Env: CHROME=/path/to/chrome  (default: Chrome on Program Files)
#      DASH_URL=http://127.0.0.1:3900  (server the dashboard should connect to)
set -u
cd "$(dirname "$0")/.."
CHROME="${CHROME:-/c/Program Files/Google/Chrome/Application/chrome.exe}"
DASH_URL="${DASH_URL:-http://127.0.0.1:3900}"
PAGE="dashboard/index.html"
TMP="$(mktemp -d)"
PROFILE="$TMP/profile"
PASS=0; FAIL=0

ok()   { PASS=$((PASS+1)); echo "  PASS  $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL  $1"; }

render() { # render <out-file> <query>
  "$CHROME" --headless=new --disable-gpu --no-first-run \
    --virtual-time-budget=20000 --user-data-dir="$PROFILE" \
    --dump-dom "file://$(cygpath -w "$PWD/$PAGE")?$2" > "$1" 2>/dev/null
}

assert_grep() { # assert_grep <file> <pattern> <label>
  grep -q "$2" "$1" && ok "$3" || bad "$3  (missing: $2)"
}

echo "== preflight =="
curl -s -m 3 "$DASH_URL/health" | grep -q '"status":"healthy"' \
  && ok "server healthy at $DASH_URL" \
  || { bad "server not running — start: target/release/velocity_mcp --mode http --config dashboard/demo.toml"; exit 1; }
[ -x "$CHROME" ] && ok "headless Chrome found" || { bad "Chrome not found at '$CHROME' (set CHROME=)"; exit 1; }

echo "== golden path (?demo=1&capture=1) =="
DOM="$TMP/dom.html"
render "$DOM" "demo=1&capture=1&api=$DASH_URL"
assert_grep "$DOM" 'class="pill ok" id="connPill">healthy'  "connected pill shows healthy"
assert_grep "$DOM" 'id="versionPill"[^>]*>v3\.2\.0'      "version pill shows v3.2.0"
assert_grep "$DOM" 'id="toolCount"[^>]*>(40'             "tools/list rendered 40 tools"
assert_grep "$DOM" '1066'                                  "js_calculator (QuickJS WASM) result 1066 shown"
assert_grep "$DOM" 'id="lastRtt"[^>]*>[0-9.]* ms'        "round-trip latency rendered in ms (not bogus us)"
assert_grep "$DOM" 'over 50 pings'                       "50-call ping burst completed"
assert_grep "$DOM" 'compare echo'                        "echo call logged (both-mode comparison)"
assert_grep "$DOM" 'id="sTotal"[^>]*>[0-9]'              "server total-requests stat populated"
assert_grep "$DOM" 'id="sRate"[^>]*>[0-9.]*'             "instantaneous req/s stat populated"
grep -q 'id="lastRtt"[^>]*>0\.0 µs' "$DOM" && bad "latency underflow: sub-10us HTTP is impossible" || ok "no impossible sub-10us HTTP latency"
# NOTE: latency-distribution checks can't live here — --virtual-time-budget
# quantizes every clock (incl. Resource Timing) to 10 ms ticks. Real-time
# assertions are in dashboard/test_cdp.mjs.

echo "== error path (unreachable server) =="
DOM2="$TMP/dom2.html"
render "$DOM2" "capture=1&api=http://127.0.0.1:1"
assert_grep "$DOM2" 'class="pill bad" id="connPill">unreachable' "unreachable pill on dead port"
assert_grep "$DOM2" 'connect failed'                                  "failure logged to event feed"

rm -rf "$TMP"
echo "== result: $PASS passed, $FAIL failed =="
[ "$FAIL" -eq 0 ]
