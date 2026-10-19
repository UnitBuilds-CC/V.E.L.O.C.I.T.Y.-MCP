// NMCP/NDA frame codec, shared by dashboard/probe_nda.mjs and dashboard/diff_nda.mjs.
// Ported from client/src/transport/nda_codec.rs. Importing this module performs no I/O.
export const NDA_HDR = 36;
export const METHODS = { initialize: 0x01, 'tools/list': 0x02, 'tools/call': 0x03, ping: 0x04 };

export const u32be = n => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
export function tlvEnc(v, out) {
  if (typeof v === 'string') {
    const b = new TextEncoder().encode(v);
    out.push(0x01, ...u32be(b.length), ...b);
  } else if (typeof v === 'number') {
    const d = new DataView(new ArrayBuffer(8));
    if (Number.isSafeInteger(v)) { out.push(0x02); d.setBigInt64(0, BigInt(v)); }
    else { out.push(0x07); d.setFloat64(0, v); }
    for (let i = 0; i < 8; i++) out.push(d.getUint8(i));
  } else if (typeof v === 'boolean') out.push(0x03, v ? 1 : 0);
  else if (v === null || v === undefined) out.push(0x04);
  else if (Array.isArray(v)) { out.push(0x05, ...u32be(v.length)); for (const it of v) tlvEnc(it, out); }
  else if (typeof v === 'object') {
    const ks = Object.keys(v);
    out.push(0x06, ...u32be(ks.length));
    for (const k of ks) {
      const kb = new TextEncoder().encode(k);
      if (kb.length > 0xFFFF) throw new Error('TLV object key length ' + kb.length + ' exceeds u16 max');
      out.push((kb.length >> 8) & 255, kb.length & 255, ...kb);
      tlvEnc(v[k], out);
    }
  } else throw new Error('TLV: unsupported ' + typeof v);
}
export function tlvDec(b, off) {
  const tag = b[off];
  if (tag === undefined) throw new Error('TLV: empty value');
  const u32 = o => (b[o] << 24 | b[o + 1] << 16 | b[o + 2] << 8 | b[o + 3]) >>> 0;
  const str = (o, len) => {
    if (b.length < o + len) throw new Error('TLV string: truncated body');
    try { return new TextDecoder('utf-8', { fatal: true }).decode(b.subarray(o, o + len)); }
    catch { throw new Error('TLV string: invalid UTF-8'); }
  };
  const need = n => { if (b.length < off + n) throw new Error('TLV value: truncated'); };
  if (tag === 0x01) {
    need(5);
    const len = u32(off + 1);
    return [str(off + 5, len), 5 + len];
  }
  if (tag === 0x03) { need(2); return [b[off + 1] !== 0, 2]; }
  if (tag === 0x04) return [null, 1];
  if (tag === 0x05 || tag === 0x06) {
    const count = u32(off + 1);
    if (count > b.length - (off + 5)) throw new Error('TLV ' + (tag === 0x05 ? 'array' : 'object') + ': truncated count');
    const a = tag === 0x05 ? [] : {}; let o = off + 5;
    for (let i = 0; i < count; i++) {
      if (tag === 0x06) {
        if (b.length < o + 2) throw new Error('TLV object: truncated key length');
        const kl = (b[o] << 8) | b[o + 1]; o += 2;
        const key = str(o, kl); o += kl;
        const [v, n] = tlvDec(b, o); a[key] = v; o += n;
      } else {
        const [v, n] = tlvDec(b, o); a.push(v); o += n;
      }
    }
    return [a, o - off];
  }
  need(9);
  const d = new DataView(b.buffer, b.byteOffset, b.byteLength);
  if (tag === 0x02) return [Number(d.getBigInt64(off + 1)), 9];
  if (tag === 0x07) return [d.getFloat64(off + 1), 9];
  throw new Error('TLV: unknown tag 0x' + tag.toString(16).padStart(2, '0'));
}
export const sha256 = async bytes => new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));

export async function buildRequest(code, id, data) {
  const payload = [code];
  tlvEnc(id, payload);
  if (data !== null && data !== undefined) tlvEnc(data, payload);
  const p = Uint8Array.from(payload);
  return new Uint8Array([...new TextEncoder().encode('NMCP'), ...await sha256(p), ...p]);
}
export async function parseResponse(frame) {
  if (frame.length < NDA_HDR + 1) throw new Error(`response too small (${frame.length} B)`);
  if (new TextDecoder().decode(frame.subarray(0, 4)) !== 'NMCP') throw new Error('bad magic');
  const payload = frame.subarray(NDA_HDR);
  const root = frame.subarray(4, 36);
  let rootOk = 'skipped (all-zero)';
  if (root.some(x => x !== 0)) {
    const computed = await sha256(payload);
    if (computed.every((x, i) => x === root[i])) rootOk = 'verified';
    else throw new Error('Merkle root mismatch');
  }
  const [id, n1] = tlvDec(payload, 1);
  let result = {};
  if (1 + n1 < payload.length) [result] = tlvDec(payload, 1 + n1);
  return { status: payload[0], id, result, rootOk };
}
