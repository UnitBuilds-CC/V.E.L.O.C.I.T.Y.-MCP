// Benchmark: Node.js MCP server vs Rust VELOCITY-MCP server
// Sends identical JSON-RPC requests over stdio to both servers.

const { spawn } = require('child_process');
const path = require('path');
const readline = require('readline');

const RUST_SERVER = path.join(__dirname, '..', 'target', 'release', 'velocity_mcp.exe');
const NODE_SERVER = path.join(__dirname, 'server.js');

const REQUESTS = {
  initialize: '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"1.0"}},"id":1}',
  ping: '{"jsonrpc":"2.0","method":"ping","id":2}',
  toolsList: '{"jsonrpc":"2.0","method":"tools/list","id":3}',
  toolsCall: '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_file","arguments":{"path":"C:/test/file.txt"}},"id":4}',
  healthCheck: '{"jsonrpc":"2.0","method":"health/check","id":5}',
};

function benchServer(name, command, args, iterations) {
  return new Promise((resolve, reject) => {
    const proc = spawn(command, args, { stdio: ['pipe', 'pipe', 'pipe'] });
    let responseBuffer = '';
    let responseCount = 0;
    let pendingResolve = null;
    let startTime = 0;

    proc.stderr.on('data', () => {}); // suppress

    proc.stdout.on('data', (data) => {
      responseBuffer += data.toString();
      const lines = responseBuffer.split('\n');
      responseBuffer = lines.pop(); // keep incomplete line
      for (const line of lines) {
        if (line.trim()) {
          responseCount++;
          if (pendingResolve) {
            const r = pendingResolve;
            pendingResolve = null;
            r();
          }
        }
      }
    });

    function sendRequest(json) {
      return new Promise((res) => {
        pendingResolve = res;
        proc.stdin.write(json + '\n');
      });
    }

    async function run() {
      // Warm up: send initialize
      await sendRequest(REQUESTS.initialize);

      // Benchmark each request type
      const results = {};
      for (const [reqName, reqJson] of Object.entries(REQUESTS)) {
        if (reqName === 'initialize') continue;

        // Warm up
        for (let i = 0; i < 10; i++) {
          await sendRequest(reqJson);
        }

        // Timed run
        const latencies = [];
        const batchStart = process.hrtime.bigint();
        for (let i = 0; i < iterations; i++) {
          const reqStart = process.hrtime.bigint();
          await sendRequest(reqJson);
          const reqEnd = process.hrtime.bigint();
          latencies.push(Number(reqEnd - reqStart) / 1e6); // ms
        }
        const batchEnd = process.hrtime.bigint();
        const totalMs = Number(batchEnd - batchStart) / 1e6;

        latencies.sort((a, b) => a - b);
        const avg = latencies.reduce((s, v) => s + v, 0) / latencies.length;
        const p50 = latencies[Math.floor(latencies.length * 0.5)];
        const p95 = latencies[Math.floor(latencies.length * 0.95)];
        const p99 = latencies[Math.floor(latencies.length * 0.99)];
        const min = latencies[0];
        const max = latencies[latencies.length - 1];
        const throughput = iterations / (totalMs / 1000);

        results[reqName] = { avg, p50, p95, p99, min, max, throughput, totalMs };
      }

      proc.stdin.end();
      proc.kill();
      resolve({ name, results });
    }

    proc.on('error', reject);
    run().catch(reject);
  });
}

async function main() {
  const iterations = parseInt(process.argv[2] || '200');
  console.log('='.repeat(72));
  console.log('  MCP Server Benchmark: Node.js vs Rust (VELOCITY-MCP v3.0.0)');
  console.log(`  ${iterations} requests per method per server`);
  console.log('='.repeat(72));

  console.log('\nRunning Node.js MCP server benchmark...');
  const nodeResults = await benchServer('Node.js', 'node', [NODE_SERVER], iterations);

  console.log('Running Rust VELOCITY-MCP server benchmark...');
  const rustResults = await benchServer('Rust', RUST_SERVER, ['--mode', 'stdio'], iterations);

  // Print comparison table
  console.log('\n' + '='.repeat(72));
  console.log('  RESULTS');
  console.log('='.repeat(72));

  for (const reqName of Object.keys(REQUESTS)) {
    if (reqName === 'initialize') continue;

    const node = nodeResults.results[reqName];
    const rust = rustResults.results[reqName];
    if (!node || !rust) continue;

    const speedup = node.avg / rust.avg;
    const label = reqName.replace(/([A-Z])/g, ' $1').replace(/^./, s => s.toUpperCase());

    console.log(`\n─── ${label} ──────────────────────────────────────────`);
    console.log(`  ${''.padEnd(20)} ${'Node.js'.padStart(12)} ${'Rust'.padStart(12)} ${'Speedup'.padStart(10)}`);
    console.log(`  ${'Avg latency'.padEnd(20)} ${(node.avg).toFixed(3).padStart(10)} ms ${(rust.avg).toFixed(3).padStart(10)} ms ${speedup.toFixed(1).padStart(8)}x`);
    console.log(`  ${'p50'.padEnd(20)} ${(node.p50).toFixed(3).padStart(10)} ms ${(rust.p50).toFixed(3).padStart(10)} ms`);
    console.log(`  ${'p95'.padEnd(20)} ${(node.p95).toFixed(3).padStart(10)} ms ${(rust.p95).toFixed(3).padStart(10)} ms`);
    console.log(`  ${'p99'.padEnd(20)} ${(node.p99).toFixed(3).padStart(10)} ms ${(rust.p99).toFixed(3).padStart(10)} ms`);
    console.log(`  ${'Throughput'.padEnd(20)} ${(node.throughput).toFixed(0).padStart(10)} r/s ${(rust.throughput).toFixed(0).padStart(10)} r/s`);
  }

  console.log('\n' + '='.repeat(72));
  console.log('  SUMMARY');
  console.log('='.repeat(72));

  let totalNodeAvg = 0, totalRustAvg = 0, count = 0;
  for (const reqName of Object.keys(REQUESTS)) {
    if (reqName === 'initialize') continue;
    const node = nodeResults.results[reqName];
    const rust = rustResults.results[reqName];
    if (node && rust) {
      totalNodeAvg += node.avg;
      totalRustAvg += rust.avg;
      count++;
    }
  }
  const overallSpeedup = totalNodeAvg / totalRustAvg;
  console.log(`  Overall avg latency:  Node.js ${(totalNodeAvg / count).toFixed(3)} ms  vs  Rust ${(totalRustAvg / count).toFixed(3)} ms`);
  console.log(`  Overall speedup:      ${overallSpeedup.toFixed(1)}x faster (Rust vs Node.js)`);
  console.log('='.repeat(72));
}

main().catch(e => { console.error(e); process.exit(1); });
