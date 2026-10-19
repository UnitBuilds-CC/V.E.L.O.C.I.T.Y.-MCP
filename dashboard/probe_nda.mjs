// Raw NDA frame probe: builds NMCP frames, POSTs to /v1/mcp/nda, verifies SHA-256 root.
// Usage: node dashboard/probe_nda.mjs [baseUrl]   (default http://127.0.0.1:3900)
import { NDA_HDR, METHODS, buildRequest, parseResponse } from './nda_codec.mjs';

const base = process.argv[2] ?? 'http://127.0.0.1:3900';

let pass = 0, fail = 0;
async function check(label, fn) {
  try { const msg = await fn(); console.log(`PASS  ${label}${msg ? ' — ' + msg : ''}`); pass++; }
  catch (e) { console.log(`FAIL  ${label} — ${e.message}`); fail++; }
}
async function rpcNda(method, params, id) {
  const frame = await buildRequest(METHODS[method], id, params ?? null);
  const res = await fetch(`${base}/v1/mcp/nda`, {
    method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body: frame,
  });
  if (!res.ok) throw new Error(`HTTP ${res.status} from ${res.url}`);
  const parsed = await parseResponse(new Uint8Array(await res.arrayBuffer()));
  if (parsed.id !== id) throw new Error(`id mismatch ${parsed.id} != ${id}`);
  if (parsed.status === 1) throw new Error('NDA error: ' + (typeof parsed.result === 'string' ? parsed.result : JSON.stringify(parsed.result)));
  return parsed;
}

console.log(`NDA raw-frame probe against ${base}\n`);
let id = 1000;
await check('ping over NDA', async () => JSON.stringify((await rpcNda('ping', null, ++id)).result).slice(0, 60));
await check('tools/list returns >= 30 tools', async () => {
  const { result } = await rpcNda('tools/list', null, ++id);
  const tools = result.tools ?? result;
  const n = Array.isArray(tools) ? tools.length : 0;
  if (n < 30) throw new Error(`only ${n} tools`);
  return `${n} tools`;
});
await check('tools/call echo', async () => {
  const { result } = await rpcNda('tools/call', { name: 'echo', arguments: { message: 'nda probe' } }, ++id);
  const text = Array.isArray(result) ? result.join('') : JSON.stringify(result);
  if (!text.includes('nda probe')) throw new Error('payload missing: ' + text.slice(0, 80));
  return text.slice(0, 60);
});
await check('NDA error path is honest (bad tool name)', async () => {
  const frame = await buildRequest(METHODS['tools/call'], ++id, { name: 'no_such_tool_xyz', arguments: {} });
  const res = await fetch(`${base}/v1/mcp/nda`, { method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body: frame });
  const parsed = await parseResponse(new Uint8Array(await res.arrayBuffer()));
  if (parsed.status !== 1) throw new Error('expected status=1 error, got ' + parsed.status);
  return 'status=1: ' + (typeof parsed.result === 'string' ? parsed.result : JSON.stringify(parsed.result)).slice(0, 60);
});
await check('corrupted frame is rejected', async () => {
  const frame = await buildRequest(METHODS.ping, ++id, null);
  frame[frame.length - 1] ^= 0xff; // flip a payload byte -> root mismatch server-side
  const res = await fetch(`${base}/v1/mcp/nda`, { method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body: frame });
  const body = res.ok ? await res.arrayBuffer() : null;
  if (!res.ok) return `rejected with HTTP ${res.status}`;
  const parsed = await parseResponse(new Uint8Array(body));
  if (parsed.status !== 1) throw new Error('tampered frame accepted!');
  return 'rejected via status=1';
});
await check('garbage bytes get a clean 4xx/parse error', async () => {
  const res = await fetch(`${base}/v1/mcp/nda`, { method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body: Uint8Array.from([1, 2, 3, 4, 5]) });
  if (res.ok) { const b = new Uint8Array(await res.arrayBuffer()); if (b.length > NDA_HDR) throw new Error('nonsense accepted'); }
  return res.ok ? 'empty-ish response' : `HTTP ${res.status}`;
});

console.log(`\n${pass} pass, ${fail} fail`);
// Not process.exit(): the keep-alive sockets from the checks above are still
// closing when this line runs, and forcing the exit aborts inside libuv on
// Windows (assertion !(handle->flags & UV_HANDLE_CLOSING), src/win/async.c)
// *after* an all-green run — turning 6/6 passed into exit code 127.
process.exitCode = fail ? 1 : 0;
