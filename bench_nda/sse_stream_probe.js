// Does POST /v1/mcp/stream reach plain GET /v1/sse subscribers? Two observers, one poster.
const http = require('http');

const port = Number(process.argv[2] || 3988);
const t0 = Date.now();

function observe(name) {
  const seen = [];
  http.get({ host: '127.0.0.1', port, path: `/v1/sse?session_id=${name}` }, (res) => {
    let buf = '';
    res.setEncoding('utf8');
    res.on('data', (c) => {
      buf += c;
      const NL2 = String.fromCharCode(10, 10);
      let i;
      while ((i = buf.indexOf(NL2)) >= 0) {
        seen.push({ t: Date.now() - t0, frame: buf.slice(0, i) });
        buf = buf.slice(i + 2);
      }
    });
  });
  return seen;
}

const a = observe('obsA');
const b = observe('obsB');

setTimeout(() => {
  const body = JSON.stringify({ jsonrpc: '2.0', method: 'tools/list', id: 7 });
  const req = http.request(
    {
      host: '127.0.0.1',
      port,
      path: '/v1/mcp/stream',
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'text/event-stream',
        'Content-Length': Buffer.byteLength(body),
      },
    },
    (res) => {
      let s = '';
      res.setEncoding('utf8');
      res.on('data', (c) => (s += c));
      res.on('end', () => console.log(`poster_http_status=${res.statusCode} poster_bytes=${s.length}`));
    }
  );
  req.on('error', (e) => console.log('poster_err=' + e.message));
  req.end(body);
}, 500);

setTimeout(() => {
  const NL = String.fromCharCode(10);
  for (const [n, arr] of [
    ['obsA', a],
    ['obsB', b],
  ]) {
    console.log(`--- ${n}: ${arr.length} frames (heartbeat comments omitted) ---`);
    for (const e of arr) {
      if (e.frame.startsWith(':')) continue;
      console.log(`t=${e.t}ms ${e.frame.split(NL).join(' <NL> ')}`);
    }
  }
  process.exit(0);
}, 4000);
