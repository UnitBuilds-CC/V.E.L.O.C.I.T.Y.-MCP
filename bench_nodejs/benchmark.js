// DEPRECATED — superseded by bench_nda (unified 7-pipeline benchmark).
// This file is kept for reference only. Run bench_nda.exe instead.
// See bench_nda/src/main.rs for the authoritative benchmark harness.

// Benchmark: Node.js MCP server vs Rust VELOCITY-MCP server
// Sends identical JSON-RPC requests over stdio and HTTP to both servers.

const { spawn } = require('child_process');
const http = require('http');
const path = require('path');

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

function benchSingleMethod(name, command, args, requestJson, iterations) {
  return new Promise((resolve, reject) => {
    const proc = spawn(command, args, { stdio: ['pipe', 'pipe', 'pipe'] });
    let responseBuffer = '';
    let pendingResolve = null;

    proc.stderr.on('data', () => {});

    proc.stdout.on('data', (data) => {
      responseBuffer += data.toString();
      const lines = responseBuffer.split('\n');
      responseBuffer = lines.pop();
      for (const line of lines) {
        if (line.trim() && pendingResolve) {
          const r = pendingResolve;
          pendingResolve = null;
          r();
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
      await sendRequest(REQUESTS.initialize);
      for (let i = 0; i < 10; i++) await sendRequest(requestJson);

      const latencies = [];
      const batchStart = process.hrtime.bigint();
      for (let i = 0; i < iterations; i++) {
        const reqStart = process.hrtime.bigint();
        await sendRequest(requestJson);
        const reqEnd = process.hrtime.bigint();
        latencies.push(Number(reqEnd - reqStart) / 1e6);
      }
      const batchEnd = process.hrtime.bigint();
      const totalMs = Number(batchEnd - batchStart) / 1e6;

      latencies.sort((a, b) => a - b);
      const avg = latencies.reduce((s, v) => s + v, 0) / latencies.length;
      const p50 = latencies[Math.floor(latencies.length * 0.5)];
      const p95 = latencies[Math.floor(latencies.length * 0.95)];
      const p99 = latencies[Math.floor(latencies.length * 0.99)];

      proc.stdin.end();
      proc.kill();
      resolve({ avg, p50, p95, p99, min: latencies[0], max: latencies[latencies.length - 1], throughput: iterations / (totalMs / 1000) });
    }

    proc.on('error', reject);
    run().catch(reject);
  });
}

function benchHttpServer(name, cmd, args, port, iterations) {
  return new Promise((resolve, reject) => {
    const proc = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'pipe'] });
    proc.stderr.on('data', () => {});

    function httpRequest(json) {
      return new Promise((res, rej) => {
        const sentAt = process.hrtime.bigint();
        const req = http.request({
          hostname: '127.0.0.1', port,
          path: '/v1/mcp', method: 'POST',
          headers: { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(json) }
        }, (resp) => {
          let data = '';
          resp.on('data', (chunk) => { data += chunk; });
          resp.on('end', () => {
            const elapsed = Number(process.hrtime.bigint() - sentAt) / 1e6;
            res({ elapsed });
          });
        });
        req.on('error', rej);
        req.write(json);
        req.end();
      });
    }

    async function run() {
      // Wait for server
      for (let i = 0; i < 100; i++) {
        await new Promise(r => setTimeout(r, 50));
        try {
          await new Promise((res, rej) => {
            const check = http.request({ hostname: '127.0.0.1', port, path: '/health', method: 'GET' }, (r) => { r.resume(); res(); });
            check.on('error', rej);
            check.end();
          });
          break;
        } catch (e) {}
      }

      await httpRequest(REQUESTS.initialize);

      const results = {};
      for (const [reqName, reqJson] of Object.entries(REQUESTS)) {
        if (reqName === 'initialize') continue;

        for (let i = 0; i < 10; i++) await httpRequest(reqJson);

        const latencies = [];
        const batchStart = process.hrtime.bigint();
        for (let i = 0; i < iterations; i++) {
          const reqStart = process.hrtime.bigint();
          await httpRequest(reqJson);
          const reqEnd = process.hrtime.bigint();
          latencies.push(Number(reqEnd - reqStart) / 1e6);
        }
        const batchEnd = process.hrtime.bigint();
        const totalMs = Number(batchEnd - batchStart) / 1e6;

        latencies.sort((a, b) => a - b);
        const avg = latencies.reduce((s, v) => s + v, 0) / latencies.length;
        const p50 = latencies[Math.floor(latencies.length * 0.5)];
        const p95 = latencies[Math.floor(latencies.length * 0.95)];
        const p99 = latencies[Math.floor(latencies.length * 0.99)];
        const throughput = iterations / (totalMs / 1000);

        results[reqName] = { avg, p50, p95, p99, min: latencies[0], max: latencies[latencies.length - 1], throughput, totalMs };
      }

      proc.kill();
      resolve({ name, results });
    }

    proc.on('error', reject);
    run().catch(e => { proc.kill(); reject(e); });
  });
}

function benchHttpSingleMethod(name, cmd, args, port, requestJson, iterations) {
  return new Promise((resolve, reject) => {
    const proc = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'pipe'] });
    proc.stderr.on('data', () => {});

    function httpRequest(json) {
      return new Promise((res, rej) => {
        const sentAt = process.hrtime.bigint();
        const req = http.request({
          hostname: '127.0.0.1', port,
          path: '/v1/mcp', method: 'POST',
          headers: { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(json) }
        }, (resp) => {
          let data = '';
          resp.on('data', (chunk) => { data += chunk; });
          resp.on('end', () => {
            const elapsed = Number(process.hrtime.bigint() - sentAt) / 1e6;
            res({ elapsed });
          });
        });
        req.on('error', rej);
        req.write(json);
        req.end();
      });
    }

    async function run() {
      for (let i = 0; i < 100; i++) {
        await new Promise(r => setTimeout(r, 50));
        try {
          await new Promise((res, rej) => {
            const check = http.request({ hostname: '127.0.0.1', port, path: '/health', method: 'GET' }, (r) => { r.resume(); res(); });
            check.on('error', rej);
            check.end();
          });
          break;
        } catch (e) {}
      }

      await httpRequest(REQUESTS.initialize);
      for (let i = 0; i < 10; i++) await httpRequest(requestJson);

      const latencies = [];
      const batchStart = process.hrtime.bigint();
      for (let i = 0; i < iterations; i++) {
        const reqStart = process.hrtime.bigint();
        await httpRequest(requestJson);
        const reqEnd = process.hrtime.bigint();
        latencies.push(Number(reqEnd - reqStart) / 1e6);
      }
      const batchEnd = process.hrtime.bigint();
      const totalMs = Number(batchEnd - batchStart) / 1e6;

      latencies.sort((a, b) => a - b);
      const avg = latencies.reduce((s, v) => s + v, 0) / latencies.length;
      const p50 = latencies[Math.floor(latencies.length * 0.5)];
      const p95 = latencies[Math.floor(latencies.length * 0.95)];
      const p99 = latencies[Math.floor(latencies.length * 0.99)];

      proc.kill();
      resolve({ avg, p50, p95, p99, min: latencies[0], max: latencies[latencies.length - 1], throughput: iterations / (totalMs / 1000) });
    }

    proc.on('error', reject);
    run().catch(e => { proc.kill(); reject(e); });
  });
}

async function main() {
  const iterations = parseInt(process.argv[2] || '200');
  console.log('='.repeat(72));
  console.log('  MCP Server Benchmark: Node.js vs Rust (VELOCITY-MCP v3.0.0)');
  console.log(`  ${iterations} requests per method per server`);
  console.log('  stdio + HTTP transports');
  console.log('='.repeat(72));

  console.log('\nRunning Node.js MCP server benchmark (stdio)...');
  const nodeResults = await benchServer('Node.js stdio', 'node', [NODE_SERVER], iterations);

  console.log('Running Rust VELOCITY-MCP server benchmark (stdio)...');
  const rustResults = await benchServer('Rust stdio', RUST_SERVER, ['--mode', 'stdio'], iterations);

  const nodeHttpPort = 13700 + Math.floor(Math.random() * 100);
  console.log(`\nRunning Node.js MCP server benchmark (HTTP, port ${nodeHttpPort})...`);
  const nodeHttpResults = await benchHttpServer('Node.js HTTP', 'node', [NODE_SERVER, '--http', String(nodeHttpPort)], nodeHttpPort, iterations);

  const rustHttpPort = 13800 + Math.floor(Math.random() * 100);
  console.log(`Running Rust VELOCITY-MCP server benchmark (HTTP, port ${rustHttpPort})...`);
  const rustHttpResults = await benchHttpServer('Rust HTTP', RUST_SERVER, ['--mode', 'http', '--addr', `127.0.0.1:${rustHttpPort}`], rustHttpPort, iterations);

  // Print comparison table
  console.log('\n' + '='.repeat(80));
  console.log('  RESULTS — stdio transport');
  console.log('='.repeat(80));

  for (const reqName of Object.keys(REQUESTS)) {
    if (reqName === 'initialize') continue;

    const node = nodeResults.results[reqName];
    const rust = rustResults.results[reqName];
    if (!node || !rust) continue;

    const speedup = node.avg / rust.avg;
    const speedupP99 = node.p99 / rust.p99;
    const label = reqName.replace(/([A-Z])/g, ' $1').replace(/^./, s => s.toUpperCase());

    console.log(`\n─── ${label} ──────────────────────────────────────────`);
    console.log(`  ${''.padEnd(20)} ${'Node.js'.padStart(12)} ${'Rust'.padStart(12)} ${'Speedup'.padStart(10)}`);
    console.log(`  ${'Avg latency'.padEnd(20)} ${(node.avg).toFixed(3).padStart(10)} ms ${(rust.avg).toFixed(3).padStart(10)} ms ${speedup.toFixed(1).padStart(8)}x`);
    console.log(`  ${'p50'.padEnd(20)} ${(node.p50).toFixed(3).padStart(10)} ms ${(rust.p50).toFixed(3).padStart(10)} ms`);
    console.log(`  ${'p95'.padEnd(20)} ${(node.p95).toFixed(3).padStart(10)} ms ${(rust.p95).toFixed(3).padStart(10)} ms`);
    console.log(`  ${'p99'.padEnd(20)} ${(node.p99).toFixed(3).padStart(10)} ms ${(rust.p99).toFixed(3).padStart(10)} ms ${speedupP99.toFixed(1).padStart(8)}x`);
    console.log(`  ${'Throughput'.padEnd(20)} ${(node.throughput).toFixed(0).padStart(10)} r/s ${(rust.throughput).toFixed(0).padStart(10)} r/s`);
  }

  // HTTP comparison
  console.log('\n' + '='.repeat(80));
  console.log('  RESULTS — HTTP transport');
  console.log('='.repeat(80));

  for (const reqName of Object.keys(REQUESTS)) {
    if (reqName === 'initialize') continue;

    const node = nodeHttpResults.results[reqName];
    const rust = rustHttpResults.results[reqName];
    if (!node || !rust) continue;

    const speedup = node.avg / rust.avg;
    const speedupP99 = node.p99 / rust.p99;
    const label = reqName.replace(/([A-Z])/g, ' $1').replace(/^./, s => s.toUpperCase());

    console.log(`\n─── ${label} ──────────────────────────────────────────`);
    console.log(`  ${''.padEnd(20)} ${'Node.js'.padStart(12)} ${'Rust'.padStart(12)} ${'Speedup'.padStart(10)}`);
    console.log(`  ${'Avg latency'.padEnd(20)} ${(node.avg).toFixed(3).padStart(10)} ms ${(rust.avg).toFixed(3).padStart(10)} ms ${speedup.toFixed(1).padStart(8)}x`);
    console.log(`  ${'p50'.padEnd(20)} ${(node.p50).toFixed(3).padStart(10)} ms ${(rust.p50).toFixed(3).padStart(10)} ms`);
    console.log(`  ${'p95'.padEnd(20)} ${(node.p95).toFixed(3).padStart(10)} ms ${(rust.p95).toFixed(3).padStart(10)} ms`);
    console.log(`  ${'p99'.padEnd(20)} ${(node.p99).toFixed(3).padStart(10)} ms ${(rust.p99).toFixed(3).padStart(10)} ms ${speedupP99.toFixed(1).padStart(8)}x`);
    console.log(`  ${'Throughput'.padEnd(20)} ${(node.throughput).toFixed(0).padStart(10)} r/s ${(rust.throughput).toFixed(0).padStart(10)} r/s`);
  }

  // Transport overhead: stdio vs HTTP for each server
  console.log('\n' + '='.repeat(80));
  console.log('  TRANSPORT OVERHEAD: stdio vs HTTP');
  console.log('='.repeat(80));

  for (const reqName of Object.keys(REQUESTS)) {
    if (reqName === 'initialize') continue;
    const label = reqName.replace(/([A-Z])/g, ' $1').replace(/^./, s => s.toUpperCase());

    const nodeStdio = nodeResults.results[reqName];
    const nodeHttp = nodeHttpResults.results[reqName];
    const rustStdio = rustResults.results[reqName];
    const rustHttp = rustHttpResults.results[reqName];

    if (nodeStdio && nodeHttp && rustStdio && rustHttp) {
      console.log(`\n─── ${label} ──────────────────────────────────────────`);
      console.log(`  Node.js:  stdio ${(nodeStdio.avg).toFixed(3)} ms  →  HTTP ${(nodeHttp.avg).toFixed(3)} ms  (${(nodeHttp.avg / nodeStdio.avg).toFixed(1)}x overhead)`);
      console.log(`  Rust:     stdio ${(rustStdio.avg).toFixed(3)} ms  →  HTTP ${(rustHttp.avg).toFixed(3)} ms  (${(rustHttp.avg / rustStdio.avg).toFixed(1)}x overhead)`);
    }
  }

  // ── Payload Scaling Benchmark ──────────────────────────────────────
  console.log('\n' + '='.repeat(72));
  console.log('  PAYLOAD SCALING: tools/call bench_echo (serialization cost)');
  console.log('='.repeat(72));

  const payloadSizes = [
    { label: '1 KB', bytes: 1024 },
    { label: '100 KB', bytes: 102400 },
    { label: '1 MB', bytes: 1048576 },
    { label: '5 MB', bytes: 5242880 },
    { label: '10 MB', bytes: 10485760 },
  ];

  const scalingBaseIterations = Math.max(50, Math.min(iterations, 200));

  console.log(`  Adaptive iterations (fewer for large payloads)\n`);

  console.log(`  ${'Payload'.padEnd(10)} ${'Iters'.padStart(6)} ${'Node.js avg'.padStart(14)} ${'Rust avg'.padStart(14)} ${'Avg speedup'.padStart(14)} ${'Node p99'.padStart(14)} ${'Rust p99'.padStart(14)} ${'P99 speedup'.padStart(14)}`);
  console.log(`  ${'─'.repeat(10)} ${'─'.repeat(6)} ${'─'.repeat(14)} ${'─'.repeat(14)} ${'─'.repeat(14)} ${'─'.repeat(14)} ${'─'.repeat(14)} ${'─'.repeat(14)}`);

  for (const { label, bytes } of payloadSizes) {
    const iters = bytes <= 102400 ? scalingBaseIterations
                  : bytes <= 1048576 ? Math.min(scalingBaseIterations, 100)
                  : bytes <= 10485760 ? 50
                  : 20;

    const benchReq = JSON.stringify({
      jsonrpc: '2.0',
      method: 'tools/call',
      params: { name: 'bench_echo', arguments: { size: bytes } },
      id: 100
    });

    const nodeScaling = await benchSingleMethod('Node.js', 'node', [NODE_SERVER], benchReq, iters);
    const rustScaling = await benchSingleMethod('Rust', RUST_SERVER, ['--mode', 'stdio'], benchReq, iters);

    const avgSpeedup = nodeScaling.avg / rustScaling.avg;
    const p99Speedup = nodeScaling.p99 / rustScaling.p99;

    console.log(`  ${label.padEnd(10)} ${String(iters).padStart(6)} ${(nodeScaling.avg).toFixed(3).padStart(11)} ms ${(rustScaling.avg).toFixed(3).padStart(11)} ms ${avgSpeedup.toFixed(1).padStart(11)}x ${(nodeScaling.p99).toFixed(3).padStart(11)} ms ${(rustScaling.p99).toFixed(3).padStart(11)} ms ${p99Speedup.toFixed(1).padStart(11)}x`);
  }

  console.log('\n' + '='.repeat(72));
  console.log('  SUMMARY');
  console.log('='.repeat(72));

  function computeOverall(a, b) {
    let totalA = 0, totalB = 0, count = 0, totalA99 = 0, totalB99 = 0;
    for (const reqName of Object.keys(REQUESTS)) {
      if (reqName === 'initialize') continue;
      const ra = a.results[reqName], rb = b.results[reqName];
      if (ra && rb) { totalA += ra.avg; totalB += rb.avg; totalA99 += ra.p99; totalB99 += rb.p99; count++; }
    }
    return { avgA: totalA / count, avgB: totalB / count, p99A: totalA99 / count, p99B: totalB99 / count };
  }

  const stdioOverall = computeOverall(nodeResults, rustResults);
  const httpOverall = computeOverall(nodeHttpResults, rustHttpResults);

  console.log(`\n  stdio transport:`);
  console.log(`    Node.js avg:  ${(stdioOverall.avgA).toFixed(3)} ms    Rust avg:  ${(stdioOverall.avgB).toFixed(3)} ms`);
  console.log(`    Speedup: ${(stdioOverall.avgA / stdioOverall.avgB).toFixed(1)}x avg, ${(stdioOverall.p99A / stdioOverall.p99B).toFixed(1)}x p99`);

  console.log(`\n  HTTP transport:`);
  console.log(`    Node.js avg:  ${(httpOverall.avgA).toFixed(3)} ms    Rust avg:  ${(httpOverall.avgB).toFixed(3)} ms`);
  console.log(`    Speedup: ${(httpOverall.avgA / httpOverall.avgB).toFixed(1)}x avg, ${(httpOverall.p99A / httpOverall.p99B).toFixed(1)}x p99`);

  console.log(`\n  HTTP overhead (Rust):  ${(httpOverall.avgB / stdioOverall.avgB).toFixed(1)}x avg latency increase`);
  console.log(`  HTTP overhead (Node):  ${(httpOverall.avgA / stdioOverall.avgA).toFixed(1)}x avg latency increase`);
  console.log('='.repeat(72));
}

main().catch(e => { console.error(e); process.exit(1); });
