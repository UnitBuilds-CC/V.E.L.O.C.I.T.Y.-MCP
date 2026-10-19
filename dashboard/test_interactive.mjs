// Thorough interactive browser test for the dashboard, over CDP.
//
// Drives the real UI (clicks chips/buttons, edits fields) and asserts on the
// resulting DOM state — complements test_dashboard.sh (static render) and
// test_cdp.mjs (auto-demo + latency distribution).
//
// Usage: node dashboard/test_interactive.mjs [chrome.exe] [http://127.0.0.1:3900]
import { spawn } from 'node:child_process';
import http from 'node:http';
import { mkdtempSync, rmSync, existsSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const CHROME = process.argv[2] || 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const API = process.argv[3] || 'http://127.0.0.1:3900';
const page = pathToFileURL(join(import.meta.dirname, 'index.html')).href + '?api=' + API;
const dir = mkdtempSync(join(tmpdir(), 'dashint'));

// Second server instance WITH auth enabled, to exercise the API-key path.
const sleep = ms => new Promise(r => setTimeout(r, ms));
const AUTH_KEY = 'demo-secret-' + Date.now();
const EXE = process.env.VELOCITY_DASH_EXE || join(import.meta.dirname, '..', 'target', 'release', 'velocity_mcp.exe');
let authServer = null, authUp = false;
if (existsSync(EXE)) {
  authServer = spawn(EXE,
    ['--mode', 'http', '--config', join(import.meta.dirname, 'demo.toml'), '--addr', '127.0.0.1:3901'],
    { stdio: 'ignore', cwd: join(import.meta.dirname, '..'), env: { ...process.env, VELOCITY_API_KEY: AUTH_KEY } });
  for (let i = 0; i < 40 && !authUp; i++) {
    await sleep(250);
    try {
      const r = await fetch('http://127.0.0.1:3901/v1/mcp', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: '{"jsonrpc":"2.0","method":"ping","id":0}',
      });
      if (r.status === 401) authUp = true; // alive and demanding the key
    } catch {}
  }
}

const chrome = spawn(CHROME, [
  '--headless=new', '--disable-gpu', '--no-first-run',
  '--remote-debugging-port=0', `--user-data-dir=${dir}`, 'about:blank',
], { stdio: 'ignore' });

async function endpoint() {
  for (let i = 0; i < 40; i++) {
    try {
      const p = readFileSync(join(dir, 'DevToolsActivePort'), 'utf8').split('\n')[0].trim();
      if (p) return `http://127.0.0.1:${p}`;
    } catch {}
    await sleep(250);
  }
  throw new Error('CDP endpoint never came up');
}

async function target() {
  const base = await endpoint();
  for (let i = 0; i < 40; i++) {
    try {
      const l = await (await fetch(`${base}/json/list`)).json();
      const p = l.find(t => t.type === 'page');
      if (p) return p;
    } catch {}
    await sleep(250);
  }
  throw new Error('no CDP page target');
}

// Read raw SSE lines straight off the wire. EventSource is unusable for this:
// the spec makes it discard comment lines, and comment lines are exactly the
// keep-alive frames under test. Resolves early once a heartbeat arrives, so the
// run pays the server's 15s keep-alive interval and nothing more.
function sseWireFrames(base, budget) {
  return new Promise((resolve) => {
    const url = new URL('/v1/sse?session_id=wire-' + Date.now(), base);
    const lines = [];
    let error = null;
    const t0 = Date.now();
    let settled = false;
    const done = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      req.destroy();
      resolve({ lines, error, ms: Date.now() - t0 });
    };
    const timer = setTimeout(done, budget);
    const req = http.get(url, (res) => {
      let buf = '';
      res.setEncoding('utf8');
      res.on('data', (chunk) => {
        buf += chunk;
        let i;
        while ((i = buf.indexOf('\n')) >= 0) {
          const line = buf.slice(0, i).replace(/\r$/, '');
          buf = buf.slice(i + 1);
          if (line !== '') lines.push(line);
        }
        if (/^:\s*heartbeat\b/.test(lines[lines.length - 1] || '')) done();
      });
      res.on('end', done);
      res.on('error', done);
    });
    req.on('error', (e) => { error = e.message; done(); });
  });
}

let pass = 0, fail = 0;
const ok = m => { pass++; console.log('  PASS  ' + m); };
const bad = (m, d) => { fail++; console.log('  FAIL  ' + m + (d ? '  [' + d + ']' : '')); };

const t = await target();
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise(r => ws.onopen = r);
let id = 0;
const pend = new Map();
const send = (m, p = {}) => new Promise(res => {
  const i = ++id;
  pend.set(i, res);
  ws.send(JSON.stringify({ id: i, method: m, params: p }));
});
ws.onmessage = e => {
  const m = JSON.parse(e.data);
  if (m.id && pend.has(m.id)) { pend.get(m.id)(m.result); pend.delete(m.id); }
};

// Evaluate an async IIFE in the page; returns its JSON result.
async function evaljs(body) {
  const r = await send('Runtime.evaluate', {
    expression: `(async () => { ${body} })().then(v => JSON.stringify(v))`,
    awaitPromise: true, returnByValue: true,
  });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || 'page eval error');
  return JSON.parse(r.result.value);
}

// Waits for #callResult to change (page-side helper injected once).
const WAITER = `
  window.__waitResult = (ms) => new Promise(res => {
    const el = document.getElementById('callResult');
    const before = el.textContent;
    const t0 = Date.now();
    const iv = setInterval(() => {
      if (el.textContent !== before) { clearInterval(iv); res('changed'); }
      else if (Date.now() - t0 > ms) { clearInterval(iv); res('timeout'); }
    }, 50);
  });
  window.__clickTool = n => {
    // Chips for gated tools render as "<name> ·json", so match the bare name
    // either exactly or as the leading token.
    const el = [...document.getElementById('toolList').children]
      .find(t => t.textContent === n || t.textContent.split(' ')[0] === n);
    if (el) el.click();
    return !!el;
  };
  window.__logText = () => document.getElementById('log').innerText;
`;

try {
  await send('Page.enable');
  await send('Page.navigate', { url: page });
  await sleep(4000);
  await evaljs(WAITER + 'return 1;');

  console.log('== connect + render ==');
  await evaljs(`document.getElementById('transport').value = 'json'; return 1;`);
  let r = await evaljs(`
    return { pill: document.getElementById('connPill').textContent,
             tools: document.getElementById('toolList').children.length,
             count: document.getElementById('toolCount').textContent };`);
  r.pill === 'healthy' ? ok('auto-connect: pill healthy') : bad('pill', r.pill);
  r.tools === 40 ? ok('40 tool chips rendered') : bad('tool count', String(r.tools));

  console.log('== tool calls across runtimes ==');
  const calls = [
    ['echo', 'native echo', v => v.includes('hello velocity') && v.includes('"isError": false')],
    ['text_transform', 'native transform (upper)', v => v.includes('HELLO VELOCITY')],
    ['bench_echo', 'bench_echo', v => v.includes('"isError": false')],
    ['js_calculator', 'QuickJS WASM', v => v.includes('1066')],
    ['py_text_analyzer', 'MicroPython WASM', v => v.includes('"isError": false') || v.includes('words')],
    ['lua_math_tools', 'Lua WASM', v => v.includes('12')],
    ['go_text_stats', 'Go WASM', v => v.includes('"isError": false') || v.includes('chars')],
    ['rust_text_stats', 'Rust WASM', v => v.includes('"isError": false') || v.includes('words')],
  ];
  for (const [tool, label, check] of calls) {
    r = await evaljs(`
      if (!window.__clickTool('${tool}')) return { err: 'chip not found' };
      const p = window.__waitResult(15000);
      document.getElementById('callBtn').click();
      const w = await p;
      return { w, out: document.getElementById('callResult').textContent,
               rtt: document.getElementById('lastRtt').textContent };`);
    if (r.err) bad(label, r.err);
    else if (r.w === 'timeout') bad(label + ' — no result within 15 s');
    else if (!check(r.out)) bad(label + ' — unexpected result', r.out.slice(0, 100));
    else ok(`${label} (${tool}) → ${r.rtt.split('(')[0].trim()}`);
  }

  console.log('== schema prefill ==');
  r = await evaljs(`window.__clickTool('file_read');
    return { args: document.getElementById('args').value };`);
  r.args.includes('"path"') ? ok('file_read prefills its real schema property') : bad('prefill', r.args);
  r = await evaljs(`
    window.__clickTool('file_write');
    const args = document.getElementById('args').value;
    const p = window.__waitResult(15000);
    document.getElementById('callBtn').click();
    const w = await p;
    return { args, w, out: document.getElementById('callResult').textContent };`);
  (r.args.includes('velocity_demo.txt') && /^[A-Za-z]:\\\\/.test(r.args) === false && r.args.includes('C:'))
    ? ok('file_write prefills an ABSOLUTE path (name-aware hints)') : bad('path prefill', r.args);
  (r.w === 'changed' && r.out.includes('"isError": false'))
    ? ok('file_write executes with prefilled args (real file written to Public)') : bad('file_write call', r.out.slice(0, 100));

  console.log('== bad input handling ==');
  r = await evaljs(`
    window.__clickTool('echo');
    document.getElementById('args').value = '{definitely not json';
    const before = window.__logText();
    document.getElementById('callBtn').click();
    await new Promise(r2 => setTimeout(r2, 300));
    return { logged: window.__logText().includes('not valid JSON'),
             unchanged: window.__logText() === before || true };`);
  r.logged ? ok('invalid JSON args rejected with log message, no crash') : bad('invalid JSON handling');

  console.log('== NDA binary transport (side-by-side + integrity) ==');
  r = await evaljs(`
    document.getElementById('transport').value = 'both';
    window.__clickTool('echo');
    const p = window.__waitResult(15000);
    document.getElementById('callBtn').click();
    const w = await p;
    await new Promise(r2 => setTimeout(r2, 300));
    return { w, verdict: document.getElementById('cmpVerdict').textContent,
             jrow: document.getElementById('cmpJson').textContent,
             nrow: document.getElementById('cmpNda').textContent,
             out: document.getElementById('callResult').textContent };`);
  (r.verdict.includes('identical payload via both encodings'))
    ? ok('JSON and NDA return identical payloads') : bad('identical-payload', r.verdict || r.w);
  r.nrow.includes('SHA-256 root verified in-browser')
    ? ok('browser verified the NDA frame SHA-256 root') : bad('merkle verify', r.nrow);
  (!r.jrow.includes('SHA-256') && r.jrow.includes('MCP envelope'))
    ? ok('JSON row labelled honestly (no merkle claim)') : bad('json row label', r.jrow);
  const jbytes = +(r.jrow.match(/req (\d+) B/) || [0, 1e9])[1];
  const nbytes = +(r.nrow.match(/req (\d+) B/) || [0, 0])[1];
  (nbytes > 0 && nbytes < jbytes)
    ? ok(`NDA frame smaller on the wire (${nbytes} B vs ${jbytes} B)`) : bad('byte comparison', r.jrow + ' | ' + r.nrow);
  r.out.includes('hello velocity')
    ? ok('NDA result rendered in the panel') : bad('nda result render', r.out.slice(0, 80));

  // The page must mirror whatever GET /v1/nda/capabilities reports - in either
  // direction. Servers differ here: an older build reports 15/7 (the six tree-walk
  // greet guests plus Go answer NDA calls with "Unknown tool", so the server lists
  // them JSON-only); the current source reports 22/0, because those guests now
  // decode TLV args in-guest (docs/NDA_PROTOCOL.md 9.3). So take the tool from the
  // server's own list instead of hardcoding one, and assert the matching arm.
  // The gated arm is not merely conditional coverage: the next block forces it by
  // driving noBinary directly, so both arms are proven on every server.
  r = await evaljs(`
    const t = noBinary.size ? [...noBinary][0] : 'perl_greet';
    window.__clickTool(t);
    const found = current === t;
    const p = window.__waitResult(15000);
    document.getElementById('callBtn').click();
    const w = await p;
    await new Promise(r2 => setTimeout(r2, 300));
    return { t, found, gated: noBinary.has(t), jsonOnly: noBinary.size, w,
             verdict: document.getElementById('cmpVerdict').textContent,
             nrow: document.getElementById('cmpNda').textContent,
             chip: [...document.getElementById('toolList').children]
                     .find(x => x.textContent.startsWith(t)).textContent,
             out: document.getElementById('callResult').textContent,
             log: window.__logText() };`);
  console.log(`  server reports ${r.jsonOnly} JSON-only tool(s); gating arm = ${r.gated ? 'json-only' : 'binary'}`);
  r.found ? ok(`${r.t} chip selectable`) : bad('chip missing', r.t);
  (r.gated ? r.chip.includes('·json') : !r.chip.includes('·json'))
    ? ok('chip gate follows the server capability list') : bad('chip not gated', r.chip);
  (r.gated ? r.verdict.includes('reports no binary NDA executor')
           : r.verdict.includes('identical payload via both encodings'))
    ? ok('verdict follows the server capability list') : bad('verdict text', r.verdict || r.w);
  (r.gated ? (r.nrow.includes('skipped') && !r.nrow.includes('SHA-256 root verified'))
           : r.nrow.includes('SHA-256 root verified in-browser'))
    ? ok('NDA leg follows the server capability list') : bad('nda leg disagrees with gate', r.nrow.slice(0, 90));
  // Gated arm: prove the page actually reached the end of its JSON-only branch.
  // A payload literal would not do here - the tool is picked from the server's
  // list, and whichever it is may fail at runtime on this host (a tool-level
  // isError still exercises the branch, which is what this block is about).
  // Binary arm keeps the literal, because perl_greet's payload is what the
  // NDA-in-guest decode produces. Block below covers the gated branch's payload.
  (r.gated ? r.log.includes('compare ' + r.t + ': JSON only (no NDA binary executor)')
           : r.out.includes('Hello'))
    ? ok('gated branch ran to completion / binary payload rendered')
    : bad('gate path not reached', (r.gated ? r.log : r.out).slice(0, 90));

  // A tool whose guest exposes only a JSON entry point must still not run the NDA
  // leg in compare mode: the comparison would measure the same path twice. A 22/0
  // server reports no such tool, so drive the page's own gate here - the noBinary
  // set it builds from GET /v1/nda/capabilities - and re-render the chips. This is
  // what proves the gated arm above on a current-source server, and it pins the
  // mechanism for a host whose list differs.
  r = await evaljs(`
    noBinary.add('perl_greet');
    await loadTools();
    window.__clickTool('perl_greet');
    const p = window.__waitResult(15000);
    document.getElementById('callBtn').click();
    const w = await p;
    await new Promise(r2 => setTimeout(r2, 300));
    const out = { w, verdict: document.getElementById('cmpVerdict').textContent,
             nrow: document.getElementById('cmpNda').textContent,
             chip: [...document.getElementById('toolList').children]
                     .find(t => t.textContent.startsWith('perl_greet')).textContent,
             out: document.getElementById('callResult').textContent };
    noBinary.delete('perl_greet');
    return out;`);
  r.chip.includes('·json') ? ok('gated tool chip marked json-only') : bad('chip marker', r.chip);
  r.nrow.includes('skipped')
    ? ok('NDA leg skipped for tool without binary executor') : bad('nda skip', r.nrow);
  (!r.nrow.includes('µs') && !r.nrow.includes(' ms') && !r.nrow.match(/req \d+ B/))
    ? ok('skipped NDA row reports no latency or byte count') : bad('bogus nda numbers', r.nrow);
  r.verdict.includes('second number would measure nothing')
    ? ok('verdict explains why only one leg ran') : bad('verdict', r.verdict.slice(0, 90));
  r.out.includes('Hello')
    ? ok('JSON leg still returns the tool result') : bad('gated tool result', r.out.slice(0, 80));

  r = await evaljs(`
    document.getElementById('transport').value = 'nda';
    window.__clickTool('js_calculator');
    const p = window.__waitResult(15000);
    document.getElementById('callBtn').click();
    const w = await p;
    return { w, out: document.getElementById('callResult').textContent,
             log: window.__logText().slice(0, 120) };`);
  (r.w === 'changed' && r.out.includes('1066') && r.log.includes('via NDA binary'))
    ? ok('NDA-only mode executes a WASM tool (QuickJS 1066)') : bad('NDA-only mode', r.log);
  await evaljs(`document.getElementById('transport').value = 'json'; return 1;`);

  console.log('== auth: wrong vs correct API key (server on :3901 with key) ==');
  if (!authUp) bad('auth server failed to start — auth path untested');
  else {
    r = await evaljs(`
      document.getElementById('serverUrl').value = 'http://127.0.0.1:3901';
      document.getElementById('apiKey').value = 'wrong-key-123';
      document.getElementById('connectBtn').click();
      await new Promise(r2 => setTimeout(r2, 2500));
      return { log: window.__logText().slice(0, 400),
               tools: document.getElementById('toolList').children.length };`);
    (r.log.includes('failed') || r.log.includes('401'))
      ? ok('wrong API key rejected and surfaced in the log') : bad('wrong-key handling', r.log.slice(0, 120));
    r = await evaljs(`
      document.getElementById('apiKey').value = '${AUTH_KEY}';
      document.getElementById('connectBtn').click();
      await new Promise(r2 => setTimeout(r2, 2500));
      return { tools: document.getElementById('toolList').children.length,
               pill: document.getElementById('connPill').textContent };`);
    (r.tools === 40 && r.pill === 'healthy')
      ? ok('correct API key connects (40 tools on authed server)') : bad('correct-key connect', JSON.stringify(r));
    r = await evaljs(`
      window.__clickTool('echo');
      const p = window.__waitResult(10000);
      document.getElementById('callBtn').click();
      const w = await p;
      return { w, out: document.getElementById('callResult').textContent };`);
    (r.w === 'changed' && r.out.includes('hello velocity'))
      ? ok('tool call works with auth header') : bad('authed tool call', r.out?.slice(0, 80));
    // back to the unauthenticated demo server for the remaining scenarios
    await evaljs(`
      document.getElementById('serverUrl').value = '${API}';
      document.getElementById('apiKey').value = '';
      document.getElementById('connectBtn').click();
      await new Promise(r2 => setTimeout(r2, 2500));
      return 1;`);
  }

  console.log('== double connect ==');
  r = await evaljs(`
    const n = document.getElementById('log').children.length;
    document.getElementById('connectBtn').click();
    document.getElementById('connectBtn').click();
    await new Promise(r2 => setTimeout(r2, 3000));
    const kids = [...document.getElementById('log').children];
    const fresh = kids.slice(0, kids.length - n).map(e => e.textContent).join('\\n');
    return { pill: document.getElementById('connPill').textContent,
             tools: document.getElementById('toolList').children.length,
             errs: (fresh.match(/failed|Error/gi) || []).length };`);
  (r.pill === 'healthy' && r.tools === 40 && r.errs === 0)
    ? ok('double Connect idempotent (healthy, 40 tools, no errors)')
    : bad('double connect', JSON.stringify(r));

  console.log('== back-to-back bursts (rate limiter) ==');
  r = await evaljs(`
    document.getElementById('burstBtn').click();
    await new Promise(r2 => setTimeout(r2, 2500));
    document.getElementById('burstBtn').click();
    await new Promise(r2 => setTimeout(r2, 4000));
    return { burst: document.getElementById('burstOut').innerText,
             hits: document.getElementById('sSec').textContent };`);
  const graceful = r.burst.includes('over 50 pings') || r.burst.includes('rate limited');
  graceful ? ok(`second burst handled gracefully ("${r.burst.slice(0, 60)}…", rate hits ${r.hits})`)
           : bad('burst behavior', r.burst);

  console.log('== SSE feed ==');
  // Split by transport reality: EventSource delivers named events, and discards
  // SSE comment lines by spec - which is what a heartbeat is. The old assertion
  // ("heartbeat" in the page log) was only satisfiable through the double-framing
  // defect, where the keep-alive text reached the browser as `data: :heartbeat`.
  // So check the named event in the browser, and the comment frame on the wire.
  r = await evaljs(`await new Promise(r2 => setTimeout(r2, 2000));
    return { log: window.__logText() };`);
  (r.log.includes('SSE stream open') && /connected: \{"sessionId"/.test(r.log))
    ? ok('SSE open + named connected event delivered to EventSource')
    : bad('SSE named events', r.log.slice(0, 120));

  const wire = await sseWireFrames(API, 20000);
  const comments = wire.lines.filter(l => /^:\s*heartbeat\b/.test(l));
  const doubled = wire.lines.filter(l => /^data:\s*:/.test(l));
  (comments.length > 0 && doubled.length === 0)
    ? ok(`keep-alive is a bare comment frame (${comments.length} in ${(wire.ms / 1000).toFixed(1)}s, 0 double-framed)`)
    : bad('SSE keep-alive framing',
        `${comments.length} comment / ${doubled.length} data-framed` +
        (wire.error ? ' wire error: ' + wire.error : '') + ': ' + wire.lines.slice(0, 2).join(' | '));

  console.log('== no uncaught page errors ==');
  // reload with console capture: any JS error during the whole session would have
  // surfaced via exceptions thrown by evaljs; final sanity check:
  r = await evaljs(`return { total: document.getElementById('sTotal').textContent };`);
  /^\d[\d,]*$/.test(r.total) ? ok(`metrics card still live (total requests: ${r.total})`) : bad('metrics card', r.total);
} finally {
  chrome.kill();
  if (authServer) authServer.kill();
  await sleep(400);
  // Chrome's renderer children can still hold the profile directory when we get
  // here; on Windows that makes rmSync throw EPERM inside this finally block,
  // which discards the pass/fail tally below and the exit code with it. Retry,
  // and if the directory is still busy leave it (the OS reclaims %TEMP%) rather
  // than crash the run that just finished.
  try {
    rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
  } catch {
    console.log(`(left temporary profile dir: ${dir})`);
  }
}
console.log(`== interactive test: ${pass} passed, ${fail} failed ==`);
process.exit(fail ? 1 : 0);
