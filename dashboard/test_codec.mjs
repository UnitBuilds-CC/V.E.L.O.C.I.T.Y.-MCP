// Offline parity gate for the dashboard's two NDA TLV codec copies.
// Usage: node dashboard/test_codec.mjs        (no server, no network)
//
// The codec exists twice: dashboard/nda_codec.mjs (imported by probe_nda.mjs and
// diff_nda.mjs) and an inline copy in dashboard/index.html. Both decode frames whose
// payloads diff_nda.mjs then compares for payload equality, so a decoder that *invents*
// a value for a corrupt frame can underwrite a false payload-equality PASS — truncation
// is a documented production condition (256 KiB guest output clamp, result-frame
// overflow), not a hypothetical.
// This gate pins three things: corrupt input must throw, it must throw a clean
// "TLV ..." message rather than an internal one, and the two copies must return
// identical verdicts for identical bytes. It cannot check parity with the Rust SDK or
// the server parser — those have their own unit tests, and the remaining gaps to them
// (depth / element / string-length caps, big-integer precision, the zero-root Merkle
// skip) are open owner calls, so nothing here asserts those behaviours.
import { readFileSync } from 'node:fs';
import { u32be, tlvEnc, tlvDec } from './nda_codec.mjs';

// Slice a top-level `function name(...) { ... }` out of the dashboard HTML by brace depth.
function sliceFn(src, decl) {
  const start = src.indexOf(decl);
  if (start < 0) throw new Error(`inline codec: '${decl}' not found in index.html`);
  const open = src.indexOf('{', start);
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    if (src[i] === '{') depth++;
    else if (src[i] === '}') { depth--; if (depth === 0) return src.slice(start, i + 1); }
  }
  throw new Error(`inline codec: unbalanced braces after '${decl}'`);
}
const html = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const inline = new Function(
  `${sliceFn(html, 'function u32be(n)')}\n${sliceFn(html, 'function tlvEnc(v, out)')}\n` +
  `${sliceFn(html, 'function tlvDec(b, off)')}\nreturn { u32be, tlvEnc, tlvDec };`)();

const bytes = (...b) => Uint8Array.from(b.flat());
const f64 = v => { const d = new DataView(new ArrayBuffer(8)); d.setFloat64(0, v); return [...new Uint8Array(d.buffer)]; };
const i64 = v => { const d = new DataView(new ArrayBuffer(8)); d.setBigInt64(0, BigInt(v)); return [...new Uint8Array(d.buffer)]; };
const strBody = s => [...new TextEncoder().encode(s)];
const key = (k) => { const b = strBody(k); return [(b.length >> 8) & 255, b.length & 255, ...b]; };

// Well-formed frames and the value + consumed-length each decoder must report.
const CORRECT = [
  ['OK_INT', bytes(0x02, i64(7)), '7 +9'],
  ['OK_F64', bytes(0x07, f64(1.5)), '1.5 +9'],
  ['OK_TRUE', bytes(0x03, 1), 'true +2'],
  ['OK_FALSE', bytes(0x03, 0), 'false +2'],
  ['OK_NULL', bytes(0x04), 'null +1'],
  ['OK_STRING', bytes(0x01, 0, 0, 0, 2, strBody('hi')), '"hi" +7'],
  ['OK_EMPTY_STRING', bytes(0x01, 0, 0, 0, 0), '"" +5'],
  ['OK_ARRAY', bytes(0x05, 0, 0, 0, 2, 0x04, 0x04), '[null,null] +7'],
  ['OK_EMPTY_ARRAY', bytes(0x05, 0, 0, 0, 0), '[] +5'],
  ['OK_EMPTY_OBJECT', bytes(0x06, 0, 0, 0, 0), '{} +5'],
  ['OK_OBJECT', bytes(0x06, 0, 0, 0, 1, key('msg'), 0x04), '{"msg":null} +11'],
];
// Corrupt frames: every one of these was measured decoding to a fabricated value
// (true / "" / [] / {}) or an internal TypeError before the bounds checks landed.
const CORRUPT = [
  ['EMPTY', bytes()],
  ['TRUNC_BOOL', bytes(0x03)],
  ['TRUNC_INT_BODY', bytes(0x02, 1, 2, 3)],
  ['TRUNC_F64_BODY', bytes(0x07, ...i64(1).slice(0, 4))],
  ['TRUNC_STRING_LEN', bytes(0x01, 0, 0)],
  ['TRUNC_STRING_BODY', bytes(0x01, 0, 0, 0, 5, strBody('hi'))],
  ['INVALID_UTF8', bytes(0x01, 0, 0, 0, 2, 0xff, 0xfe)],
  ['TRUNC_ARRAY_COUNT', bytes(0x05, 0, 0, 0, 1)],
  ['TRUNC_OBJ_COUNT', bytes(0x06, 0, 0, 0, 1)],
  ['HUGE_ARRAY_COUNT', bytes(0x05, 0xff, 0xff, 0xff, 0xff, 0x04)],
  ['HUGE_OBJ_COUNT', bytes(0x06, 0xff, 0xff, 0xff, 0xff, 0x04)],
  ['TRUNC_OBJ_KEY_LEN', bytes(0x06, 0, 0, 0, 1, 0x00)],
  ['TRUNC_OBJ_KEY_BODY', bytes(0x06, 0, 0, 0, 1, 0x00, 5, strBody('h'), 0x04)],
  ['TRUNC_OBJ_VALUE', bytes(0x06, 0, 0, 0, 1, key('k'), 0x02, 1, 2)],
  ['TRUNC_UNKNOWN_TAG', bytes(0x99)],
  ['UNKNOWN_TAG', bytes(0x99, ...i64(0))],
];

let pass = 0, fail = 0;
function check(label, fn) {
  try { const msg = fn(); console.log(`PASS  ${label}${msg ? ' — ' + msg : ''}`); pass++; }
  catch (e) { console.log(`FAIL  ${label} — ${e.message ?? String(e)}`); fail++; }
}
const verdict = dec => b => {
  try { const [v, n] = dec(b, 0); return JSON.stringify(v) + ' +' + n; }
  catch (e) { return 'ERR ' + (e instanceof Error ? e.message : 'non-Error ' + String(e)); }
};
const throwsClean = dec => b => {
  let err;
  try { const [v] = dec(b, 0); throw new Error(`fabricated value: ${JSON.stringify(v)}`); }
  catch (e) { err = e instanceof Error ? e.message : 'non-Error ' + String(e); }
  if (err.startsWith('fabricated')) throw new Error(err);
  if (!/^TLV\b/.test(err)) throw new Error(`internal error text: ${err}`);
  return err.slice(4, 40);
};

console.log('NDA TLV codec parity gate (offline)\n');
for (const [name, frame, want] of CORRECT) {
  check(`${name} decodes`, () => {
    const got = verdict(tlvDec)(frame);
    if (got !== want) throw new Error(`want ${want}, got ${got}`);
    return want;
  });
}
for (const [name, frame] of CORRUPT) check(`${name} rejected`, () => throwsClean(tlvDec)(frame));

check(`inline index.html copy agrees on ${CORRECT.length + CORRUPT.length} frames`, () => {
  const all = [...CORRECT.map(f => f[1]), ...CORRUPT.map(f => f[1])];
  const diffs = all.filter((b, i) => verdict(tlvDec)(b) !== verdict(inline.tlvDec)(b));
  if (diffs.length) {
    const i = all.indexOf(diffs[0]);
    throw new Error(`${diffs.length} frames diverge, first: ${i === -1 ? '?' : (i < CORRECT.length ? CORRECT[i][0] : CORRUPT[i - CORRECT.length][0])}`);
  }
  return 'identical verdicts';
});
check('both copies reject a > u16::MAX object key on encode', () => {
  const big = 'k'.repeat(65536);
  const msgs = [tlvEnc, inline.tlvEnc].map(enc => {
    try { enc({ [big]: 1 }, []); return 'no error (key guard missing)'; }
    catch (e) { return e instanceof Error ? e.message : 'non-Error ' + String(e); }
  });
  if (msgs[0] !== msgs[1]) throw new Error(`copies diverge: ${msgs[0]} vs ${msgs[1]}`);
  if (!/exceeds u16 max/.test(msgs[0])) throw new Error(msgs[0]);
  return msgs[0];
});
check('both copies roundtrip a nested value', () => {
  const v = { msg: 'hi', n: 3, arr: [1, 2], o: { b: true, z: null } };
  const got = [[tlvEnc, tlvDec], [inline.tlvEnc, inline.tlvDec]].map(([enc, dec]) => {
    const out = []; enc(v, out);
    return JSON.stringify(dec(Uint8Array.from(out), 0)[0]);
  });
  if (got[0] !== JSON.stringify(v) || got[1] !== got[0]) throw new Error(got.join(' vs '));
  return 'stable';
});

console.log(`\n${pass} pass, ${fail} fail`);
// Not process.exit(): see probe_nda.mjs — forcing exit mid-cleanup aborts in libuv on
// Windows and turns an all-green run into exit code 127.
process.exitCode = fail ? 1 : 0;
