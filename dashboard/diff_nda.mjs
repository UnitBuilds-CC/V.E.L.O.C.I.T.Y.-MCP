// NDA-vs-JSON payload differential + in-guest decode discriminator, over all WASM tools.
// Usage: node dashboard/diff_nda.mjs [baseUrl] [--poison]   (default http://127.0.0.1:3900)
// Exit 0 when every tool's payload matches its JSON twin.
// --poison additionally sends a malformed-arguments frame per tool. It is advisory about
// where the TLV is decoded (see poisonCall), and it is off by default because it is not
// read-only: the frames make some guests trap, and a trapped runtime is discarded and its
// tool downgraded in the capability report, so the run mutates the state it then re-reads.
// The default pass is not passive either, and measuring it is what showed that: a
// well-formed tools/call for a guest whose registration fails on the host (TinyGo absent
// for analyze_logs_go, measured 2026-10-13) records the decoded-JSON fallback the same way
// and for the same lifetime -- the first pass moved that tool true->false, the second read
// 21/1 as its starting state. So read `capabilities ... before -> ... after` as a mutation
// report for any pass, not as a poison-mode-only one.
import { METHODS, u32be, tlvEnc, buildRequest, parseResponse } from './nda_codec.mjs';

const POISON = process.argv.includes('--poison');
const base = process.argv.slice(2).find(a => !a.startsWith('--')) ?? 'http://127.0.0.1:3900';
const LOGS = ['2026-01-01 ERROR alpha failed', '2026-01-02 WARN beta slow',
  '2026-01-03 INFO gamma ok', '2026-01-04 ERROR alpha again'];
const TEXT = 'the quick brown fox jumps over the lazy dog again and again';
const FIXTURES = {
  analyze_logs: { log_lines: LOGS }, analyze_logs_lua: { log_lines: LOGS },
  analyze_logs_js: { log_lines: LOGS }, analyze_logs_go: { log_lines: LOGS },
  csharp_greet: { name: 'world' }, java_greet: { name: 'world' }, julia_greet: { name: 'world' },
  perl_greet: { name: 'world' }, php_greet: { name: 'world' }, r_greet: { name: 'world' },
  go_text_stats: { text: TEXT }, rb_text_stats: { text: TEXT }, rust_text_stats: { text: TEXT },
  ts_text_stats: { text: TEXT }, py_text_analyzer: { text: TEXT },
  js_string_transform: { action: 'slug', input: 'Hello Wide World' },
  js_calculator: { expression: '2+3*4' },
  js_data_flatten: { data: { a: { b: 1 }, c: [2, 3] } },
  lua_string_utils: { action: 'upper', input: 'hello world', count: 2 },
  lua_math_tools: { operation: 'power', value: 2, exponent: 10 },
  py_unit_converter: { value: 5, from_unit: 'km', to_unit: 'mi' },
  py_list_operations: { items: [3, 1, 2, 1], operation: 'sort' },
};
// Tools whose *guest* decodes TLV; the rest are advertised binary but served by the
// host's decode-then-JSON fall-through.
const IN_GUEST = new Set(['analyze_logs', 'py_text_analyzer', 'py_unit_converter', 'py_list_operations',
  'analyze_logs_lua', 'lua_string_utils', 'lua_math_tools', 'csharp_greet', 'java_greet',
  'julia_greet', 'perl_greet', 'php_greet', 'r_greet']);

const caps = async () => {
  const j = await (await fetch(`${base}/v1/nda/capabilities`)).json();
  return new Map(j.tools.map(t => [t.name, t]));
};
const ndaCall = async (name, args, id) => {
  const frame = await buildRequest(METHODS['tools/call'], id, { name, arguments: args });
  const res = await fetch(`${base}/v1/mcp/nda`, {
    method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body: frame });
  return { parsed: await parseResponse(new Uint8Array(await res.arrayBuffer())), bytes: frame.length };
};
// `arguments` is extracted as a raw TLV slice with no shape requirement, so an array here
// reaches whichever decoder reads it. This is an *advisory* probe, not a proof of decode
// site: measured 2026-10-09 on current source, `perl_greet` answered `{"error":"undefined
// variable 'name' at line 1"}` and `lua_math_tools` `{"error":"unknown operation: "}` —
// a guest that tolerates the absent field answers the same whether it decoded the TLV
// itself or received the host's re-serialized JSON `[]`. Only an explicit refusal
// ("TLV decode failed or not an object") or a trap distinguishes, and then only against
// the in-guest case. A bad *tag* (0x99) cannot be used at all: the host's skip walker
// refuses it before dispatch ("Malformed tools/call payload: TLV skip:").
const poisonCall = async (name, id) => {
  const key = k => { const b = new TextEncoder().encode(k); return [(b.length >> 8) & 255, b.length & 255, ...b]; };
  const p = [METHODS['tools/call'], ...tlv(id), 0x06, ...u32be(2), ...key('name'), ...str(name),
    ...key('arguments'), 0x05, 0, 0, 0, 0];
  const payload = Uint8Array.from(p);
  const frame = new Uint8Array([...new TextEncoder().encode('NMCP'),
    ...new Uint8Array(await crypto.subtle.digest('SHA-256', payload)), ...payload]);
  const res = await fetch(`${base}/v1/mcp/nda`, {
    method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body: frame });
  const parsed = await parseResponse(new Uint8Array(await res.arrayBuffer()));
  const text = typeof parsed.result === 'string' ? parsed.result : JSON.stringify(parsed.result);
  if (/TLV decode failed or not an object/i.test(text)) return 'guest';
  if (/exceeded resource limits or trapped/i.test(text)) return 'guest:trap';
  if (/Malformed tools\/call payload/i.test(text)) return 'host:frame';
  return 'silent';
};
const tlv = v => { const o = []; tlvEnc(v, o); return o; };
const str = v => tlv(v);
const jsonText = async (name, args) => {
  const r = await (await fetch(`${base}/v1/mcp`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', method: 'tools/call', id: 1, params: { name, arguments: args } }),
  })).json();
  if (r.error) return 'ERR ' + r.error.message;
  const c = r.result?.content;
  return Array.isArray(c) ? c.map(x => x.text ?? JSON.stringify(x)).join('') : JSON.stringify(r.result);
};
const norm = s => String(s).split('\n')
  .filter(l => !/^\s*(Error type|Hint):/i.test(l))
  .map(l => l.trim()).join('\n').trim();
const ndaText = p => typeof p.result === 'string' ? p.result : JSON.stringify(p.result);

const before = await caps();
const names = [...before.keys()].sort();
let identical = 0, diverged = 0, guestDecode = 0, trapDecode = 0, bothError = 0;
const erroring = [];
const rows = [];
let id = 5000;
for (const name of names) {
  const args = FIXTURES[name];
  if (!args) { rows.push(`${name.padEnd(21)} no fixture — SKIPPED`); diverged++; continue; }
  const json = norm(await jsonText(name, args));
  const { parsed, bytes } = await ndaCall(name, args, ++id);
  const nda = norm(ndaText(parsed));
  const site = POISON ? await poisonCall(name, ++id) : 'off';
  // The differential asks whether the two transports carry the *same payload*, which is true
  // of an identical error on both legs. Counting status here made the runner red for a tool
  // that is broken in both paths — a health fact, not a transport divergence — so it is
  // reported on its own line instead.
  const same = nda === json;
  if (same) identical++; else diverged++;
  if (parsed.status !== 0 || json.startsWith('ERR ')) { bothError++; erroring.push(name); }
  if (site === 'guest') guestDecode++; else if (site === 'guest:trap') trapDecode++;
  const predicts = before.get(name).binaryExecutor ? (IN_GUEST.has(name) ? 'guest' : 'host') : 'host';
  rows.push(`${name.padEnd(21)} ${String(before.get(name).binaryExecutor).padEnd(5)} ` +
    `nda=${String(parsed.status)} ${bytes}B ${same ? 'IDENTICAL' : 'DIFFERS    '} ` +
    `poison=${site.padEnd(11)} predicts=${POISON ? predicts : '—'}` +
    (same ? '' : `\n    nda : ${nda.slice(0, 110)}\n    json: ${json.slice(0, 110)}`));
}
const after = await caps();
const changed = names.filter(n => before.get(n).binaryExecutor !== after.get(n).binaryExecutor);
console.log(`WASM tool differential against ${base}\n`);
console.log(rows.join('\n'));
console.log(`\n${names.length} tools: payload identical ${identical}, diverged/skipped ${diverged}`);
console.log(`identical-but-erroring (tool broken in this build, not a transport split): ${bothError}` +
  (erroring.length ? ` — ${erroring.join(', ')}` : ''));
if (POISON) {
  console.log(`poison probe: explicit guest refusal ${guestDecode}, guest trap ${trapDecode}, silent ${names.length - guestDecode - trapDecode} (silent proves nothing — see the note above)`);
} else {
  console.log('decode site: not probed (pass --poison; it mutates server state)');
}
console.log(`capabilities ${[...before.values()].filter(t => t.binaryExecutor).length}/` +
  `${[...before.values()].filter(t => !t.binaryExecutor).length} before → ` +
  `${[...after.values()].filter(t => t.binaryExecutor).length}/` +
  `${[...after.values()].filter(t => !t.binaryExecutor).length} after`);
if (changed.length) console.log(`downgraded during the run: ${changed.join(', ')}`);
process.exitCode = diverged > 0 ? 1 : 0;
