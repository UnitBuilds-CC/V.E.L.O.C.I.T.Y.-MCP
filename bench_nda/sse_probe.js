// One-shot probe: record SSE line arrival times and event names from a live server.
const http = require('http');

const port = Number(process.argv[2] || 3988);
const session = process.argv[3] || 'probe';
const windowMs = Number(process.argv[4] || 40000);
const triggerPost = process.argv[5] === 'post';

const t0 = Date.now();
const events = [];

http.get({ host: '127.0.0.1', port, path: `/v1/sse?session_id=${session}` }, (res) => {
  let buf = '';
  res.setEncoding('utf8');
  res.on('data', (chunk) => {
    buf += chunk;
    let idx;
    while ((idx = buf.indexOf('\n\n')) >= 0) {
      const frame = buf.slice(0, idx);
      buf = buf.slice(idx + 2);
      events.push({ t: Date.now() - t0, frame });
    }
  });
});

if (triggerPost) {
  setTimeout(() => {
    const body = JSON.stringify({
      jsonrpc: '2.0',
      method: 'tools/list',
      id: 1000 + Math.floor(Math.random() * 100),
    });
    const req = http.request(
      {
        host: '127.0.0.1',
        port,
        path: '/v1/mcp',
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(body) },
      },
      () => {}
    );
    req.on('error', () => {});
    req.end(body);
  }, 5000);
}

setTimeout(() => {
  console.log(`total_frames=${events.length}`);
  let prev = 0;
  const NL = String.fromCharCode(10);
  for (const e of events) {
    console.log(`t=${e.t}ms gap=${e.t - prev}ms frame=${e.frame.split(NL).join(' <NL> ')}`);
    prev = e.t;
  }
  process.exit(0);
}, windowMs);
