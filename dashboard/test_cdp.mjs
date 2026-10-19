// Real-browser-session test for the dashboard, over Chrome DevTools Protocol.
//
// The --dump-dom suite can't verify latency accuracy: --virtual-time-budget
// quantizes every clock (including Resource Timing) to 10 ms ticks, and an
// open EventSource freezes virtual time entirely. This test uses real wall
// time: it runs the auto-demo, then asserts the SSE feed is live and the ping
// burst shows a genuine latency distribution (not timer-quantized flat 10 ms).
//
// Usage: node dashboard/test_cdp.mjs [chrome.exe] [http://127.0.0.1:3900]
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const CHROME = process.argv[2] || 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const API = process.argv[3] || 'http://127.0.0.1:3900';
const page = pathToFileURL(join(import.meta.dirname, 'index.html')).href
  + '?demo=1&api=' + API;
const dir = mkdtempSync(join(tmpdir(), 'dashcdp'));

const chrome = spawn(CHROME, [
  '--headless=new', '--disable-gpu', '--no-first-run',
  '--remote-debugging-port=0', `--user-data-dir=${dir}`, 'about:blank',
], { stdio: 'ignore' });

const sleep = ms => new Promise(r => setTimeout(r, ms));

// Port 0 makes Chrome choose a free port and publish it in the profile dir. A
// fixed port let any debug port already open on this machine — e.g. the
// operator's own Chrome started with --remote-debugging-port — answer /json/list
// instead, and this suite would then have driven that real browser.
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
      const list = await (await fetch(`${base}/json/list`)).json();
      const p = list.find(t => t.type === 'page');
      if (p) return p;
    } catch {}
    await sleep(250);
  }
  throw new Error('no CDP page target');
}

let pass = 0, fail = 0;
const ok = m => { pass++; console.log('  PASS  ' + m); };
const bad = m => { fail++; console.log('  FAIL  ' + m); };

try {
  const t = await target();
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise(r => ws.onopen = r);
  let id = 0;
  const pending = new Map();
  const send = (method, params = {}) => new Promise((res, rej) => {
    const mid = ++id;
    pending.set(mid, { res, rej });
    ws.send(JSON.stringify({ id: mid, method, params }));
  });
  ws.onmessage = e => {
    const m = JSON.parse(e.data);
    if (m.id && pending.has(m.id)) { pending.get(m.id).res(m.result); pending.delete(m.id); }
  };

  await send('Page.enable');
  await send('Page.navigate', { url: page });
  await sleep(12000); // connect + tools + echo + calculator + 50-ping burst + SSE

  const ev = await send('Runtime.evaluate', {
    expression: 'JSON.stringify({pill: document.getElementById("connPill").textContent, ' +
      'log: document.getElementById("log").innerText, burst: document.getElementById("burstOut").innerText})',
    returnByValue: true,
  });
  const { pill, log, burst } = JSON.parse(ev.result.value);

  pill === 'healthy' ? ok('connected pill shows healthy') : bad('connected pill (got: ' + pill + ')');
  (log.includes('SSE stream open') || log.includes('event:'))
    ? ok('SSE event feed active in log') : bad('SSE event feed active in log');
  burst.includes('over 50 pings')
    ? ok('50-call ping burst completed') : bad('burst did not complete (got: ' + burst + ')');

  const nums = [...burst.matchAll(/(\d+\.\d+) ms/g)].map(m => parseFloat(m[1]));
  if (nums.length >= 3) {
    const [avg, p50, p95] = nums;
    (avg === 10 && p50 === 10)
      ? bad(`latency pinned to 10.00 ms (avg ${avg}, p50 ${p50}) — timer quantization, not real measurement`)
      : ok(`burst shows real latency distribution (avg ${avg} ms, p50 ${p50} ms, p95 ${p95} ms)`);
    (avg > 0.05 && avg < 50)
      ? ok(`avg round-trip plausible for localhost HTTP (${avg} ms)`)
      : bad(`avg round-trip implausible (${avg} ms)`);
  } else bad('could not parse burst percentiles: ' + burst);
} finally {
  chrome.kill();
  await sleep(500);
  // A renderer can still hold profile files a moment after the kill; a throw here
  // pre-empts the verdict line below, making the exit code describe the cleanup.
  try {
    rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  } catch (e) {
    console.log(`  NOTE  Chrome profile dir kept (${e.code}) - verdict unaffected`);
  }
}
console.log(`== cdp test: ${pass} passed, ${fail} failed ==`);
process.exit(fail ? 1 : 0);
