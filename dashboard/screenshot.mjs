// Real-clock dashboard screenshot via Chrome DevTools Protocol.
//
// Preferred over `chrome --headless --screenshot --virtual-time-budget`,
// which quantizes every clock to 10 ms ticks and renders fake flat latencies.
//
// Usage: node dashboard/screenshot.mjs [out.png]
//        node dashboard/screenshot.mjs shot.png "file:///.../index.html?demo=1&api=http://127.0.0.1:3900"
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CHROME = process.env.CHROME || 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const OUT = process.argv[2] || 'dashboard.png';
const url = process.argv[3] || new URL('index.html?demo=1', import.meta.url).href;
const dir = mkdtempSync(join(tmpdir(), 'dashshotcdp'));

const chrome = spawn(CHROME, [
  '--headless=new', '--disable-gpu', '--no-first-run',
  '--remote-debugging-port=0', `--user-data-dir=${dir}`, 'about:blank',
], { stdio: 'ignore' });
const sleep = ms => new Promise(r => setTimeout(r, ms));

// See test_cdp.mjs: port 0 + the profile's DevToolsActivePort file, so a debug
// port already open on this machine can't be driven by mistake.
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

try {
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
  await send('Page.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 1500, deviceScaleFactor: 1, mobile: false });
  await send('Page.navigate', { url });
  await sleep(13000); // let the auto-demo (connect, tool calls, 50-ping burst) finish
  const shot = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync(OUT, Buffer.from(shot.data, 'base64'));
  console.log('saved ' + OUT);
} finally {
  chrome.kill();
  await sleep(400);
  rmSync(dir, { recursive: true, force: true });
}
