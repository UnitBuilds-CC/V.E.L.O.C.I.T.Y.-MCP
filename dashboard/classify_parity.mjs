// Compare error classification between the JSON-RPC path and the NDA binary path.
// Usage: node classify_parity.mjs <baseUrl>
import { METHODS, buildRequest, parseResponse } from './nda_codec.mjs';

const base = process.argv[2] ?? 'http://127.0.0.1:3905';

const CASES = [
  ['analyze_logs_go (missing TinyGo)', { name: 'analyze_logs_go', arguments: { log_lines: ['ERROR boom'] } }],
  ['file_read (missing absolute path)', { name: 'file_read', arguments: { path: 'C:/definitely/not/here.txt' } }],
  ['file_read (relative path)', { name: 'file_read', arguments: { path: 'relative.txt' } }],
  ['no_such_tool_xyz (unknown tool)', { name: 'no_such_tool_xyz', arguments: {} }],
  ['convert_to_nda_document (bad args)', { name: 'convert_to_nda_document', arguments: {} }],
];

function classify(text) {
  const m = /Error type: ([A-Z_]+)/.exec(text);
  const h = /Hint: ([^\n]*)/.exec(text);
  return { kind: m ? m[1] : '(none)', hint: h ? h[1].slice(0, 42) : '(none)' };
}

async function viaJson(params) {
  const res = await fetch(`${base}/v1/mcp`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', method: 'tools/call', id: 1, params }),
  });
  const body = await res.text();
  let json;
  try { json = JSON.parse(body); } catch { return { text: '<non-json ' + res.status + '>' }; }
  const t = json?.result?.content?.[0]?.text ?? JSON.stringify(json).slice(0, 90);
  return { text: t, isError: json?.result?.isError };
}

async function viaNda(params) {
  const frame = await buildRequest(METHODS['tools/call'], 1, params);
  const res = await fetch(`${base}/v1/mcp/nda`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/octet-stream' },
    body: frame,
  });
  const buf = new Uint8Array(await res.arrayBuffer());
  if (buf.length <= 36) return { text: '<short frame ' + res.status + ' len ' + buf.length + '>' };
  const parsed = await parseResponse(buf);
  const payload = parsed?.result ?? parsed?.data ?? {};
  // The NDA error arm carries the message as a bare string (`json!(error_msg)`), while a
  // tool result is a {content:[{text}]} object -- reading only the second shape made the
  // probe fall back to a truncated JSON dump and invent a mismatch.
  const t = typeof payload === 'string' ? payload : (payload?.content?.[0]?.text ?? JSON.stringify(payload));
  return { text: t, isError: typeof payload === 'object' ? payload?.isError : true, status: parsed?.status };
}

let mismatch = 0;
for (const [label, params] of CASES) {
  const j = await viaJson(params);
  const n = await viaNda(params);
  const cj = classify(j.text);
  const cn = classify(n.text);
  const same = cj.kind === cn.kind;
  if (!same) mismatch++;
  console.log(`${label}`);
  console.log(`   json: isError=${j.isError} kind=${cj.kind} hint="${cj.hint}"`);
  console.log(`   nda : status=${n.status} isError=${n.isError} kind=${cn.kind} hint="${cn.hint}" ${same ? '' : '\n          raw=' + n.text.slice(0, 150).replace(/\n/g, ' | ')}`);
  console.log(`   => ${same ? 'same classification' : 'CLASSIFICATION MISMATCH'}`);
  if (!same) {
    // Slice for display only: classifying a truncated string would invent a mismatch,
    // because the `Error type:` line is the last thing in the payload.
    console.log(`      json raw: ${j.text.replace(/\n/g, ' | ').slice(0, 220)}`);
    console.log(`      nda  raw: ${n.text.replace(/\n/g, ' | ').slice(0, 220)}`);
  }
}
console.log(`\nparity: ${CASES.length - mismatch}/${CASES.length} cases classified identically on both paths`);
// Not process.exit(): same trap probe_nda.mjs documents — the keep-alive sockets from
// the checks above are still closing, and forcing the exit aborts inside libuv on
// Windows (assertion !(handle->flags & UV_HANDLE_CLOSING), src/win/async.c) *after* a
// 5/5 green run, which measured as exit 127 (2026-10-14).
process.exitCode = mismatch ? 1 : 0;
