// Cold vs warm tools/list comparison: Node.js vs Rust
const { spawn } = require('child_process');
const path = require('path');

const NODE_SERVER = path.join(__dirname, 'server.js');
const RUST_SERVER = path.join(__dirname, '..', 'target', 'release', 'velocity_mcp.exe');

function benchServer(name, cmd, args) {
  return new Promise((resolve) => {
    const proc = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'pipe'] });
    let buffer = '';
    const pendingResolve = new Map();

    proc.stdout.on('data', (chunk) => {
      buffer += chunk.toString();
      let newlineIdx;
      while ((newlineIdx = buffer.indexOf('\n')) !== -1) {
        const line = buffer.slice(0, newlineIdx).trim();
        buffer = buffer.slice(newlineIdx + 1);
        if (line) {
          try {
            const resp = JSON.parse(line);
            if (resp.id !== undefined && pendingResolve.has(resp.id)) {
              const { resolve: res, sentAt } = pendingResolve.get(resp.id);
              const receivedAt = process.hrtime.bigint();
              pendingResolve.delete(resp.id);
              res({ resp, elapsed: Number(receivedAt - sentAt) / 1e6 });
            }
          } catch (e) {}
        }
      }
    });

    function sendAndWait(id, obj) {
      return new Promise((res) => {
        const sentAt = process.hrtime.bigint();
        pendingResolve.set(id, { resolve: res, sentAt });
        proc.stdin.write(JSON.stringify(obj) + '\n');
      });
    }

    async function run() {
      await sendAndWait(1, { jsonrpc: '2.0', method: 'initialize', params: { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'bench', version: '1.0' } }, id: 1 });
      proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized', params: {} }) + '\n');

      const timings = [];

      // Cold call #1
      const r1 = await sendAndWait(2, { jsonrpc: '2.0', method: 'tools/list', params: {}, id: 2 });
      timings.push({ label: 'cold', ms: r1.elapsed });

      // Warm call #2
      const r2 = await sendAndWait(3, { jsonrpc: '2.0', method: 'tools/list', params: {}, id: 3 });
      timings.push({ label: 'warm', ms: r2.elapsed });

      // 298 more
      for (let i = 4; i <= 301; i++) {
        const r = await sendAndWait(i, { jsonrpc: '2.0', method: 'tools/list', params: {}, id: i });
        timings.push({ label: 'cache', ms: r.elapsed });
      }

      proc.kill();

      const cold = timings[0].ms;
      const warm = timings[1].ms;
      const cacheOnly = timings.slice(2).map(t => t.ms);
      const sorted = [...cacheOnly].sort((a, b) => a - b);
      const avg = cacheOnly.reduce((a, b) => a + b, 0) / cacheOnly.length;

      resolve({
        name,
        cold,
        warm,
        coldWarmRatio: cold / warm,
        cacheAvg: avg,
        cacheP50: sorted[Math.floor(sorted.length * 0.50)],
        cacheP95: sorted[Math.floor(sorted.length * 0.95)],
        cacheP99: sorted[Math.floor(sorted.length * 0.99)],
        cacheMin: sorted[0],
        cacheMax: sorted[sorted.length - 1]
      });
    }

    run().then(resolve).catch(e => { proc.kill(); resolve({ name, error: e.message }); });
  });
}

async function main() {
  console.log('Cold vs warm tools/list: Node.js vs Rust (300 calls each)\n');

  const nodeResult = await benchServer('Node.js', 'node', [NODE_SERVER]);
  const rustResult = await benchServer('Rust', RUST_SERVER, ['--mode', 'stdio']);

  if (nodeResult.error) console.error('Node.js error:', nodeResult.error);
  if (rustResult.error) console.error('Rust error:', rustResult.error);

  console.log('  ' + ''.padEnd(24) + 'Node.js'.padStart(14) + 'Rust'.padStart(14) + 'Speedup'.padStart(10));
  console.log('  ' + '─'.repeat(62));

  const row = (label, n, r) => {
    const speedup = n / r;
    console.log(`  ${label.padEnd(24)} ${(n).toFixed(3).padStart(11)} ms ${(r).toFixed(3).padStart(11)} ms ${speedup.toFixed(1).padStart(8)}x`);
  };

  row('Call #1 (cold)', nodeResult.cold, rustResult.cold);
  row('Call #2 (warm)', nodeResult.warm, rustResult.warm);
  console.log('');
  row('Cache hits avg', nodeResult.cacheAvg, rustResult.cacheAvg);
  row('Cache hits p50', nodeResult.cacheP50, rustResult.cacheP50);
  row('Cache hits p95', nodeResult.cacheP95, rustResult.cacheP95);
  row('Cache hits p99', nodeResult.cacheP99, rustResult.cacheP99);
  console.log('');

  console.log(`  Cold penalty (cold/warm):`);
  console.log(`    Node.js: ${nodeResult.coldWarmRatio.toFixed(1)}x slower on first call`);
  console.log(`    Rust:    ${rustResult.coldWarmRatio.toFixed(1)}x slower on first call`);
  console.log('');
  console.log(`  Node.js: static const array (zero assembly)`);
  console.log(`  Rust:    dynamic assembly (4 registries, hashset dedup, merge 5 sources)`);
}

main();
