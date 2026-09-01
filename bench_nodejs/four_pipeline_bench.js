// DEPRECATED — superseded by bench_nda (unified 7-pipeline benchmark).
// This file is kept for reference only. Run bench_nda.exe instead.
// See bench_nda/src/main.rs for the authoritative benchmark harness.

// 6-pipeline benchmark: Node.js JSON vs Rust JSON vs Rust NDA-wrapped vs Rust pure NDA
// + Node.js HTTP vs Rust HTTP
const { spawn } = require('child_process');
const http = require('http');
const path = require('path');

const NODE_SERVER = path.join(__dirname, 'server.js');
const RUST_SERVER = path.join(__dirname, '..', 'target', 'release', 'velocity_mcp.exe');
const BENCH_NDA = path.join(__dirname, '..', 'bench_nda', 'target', 'release', 'bench_nda.exe');

function benchStdioServer(name, cmd, args, setupFn = null) {
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

      if (setupFn) await setupFn(sendAndWait);

      const results = {
        toolsList: [],
        toolsCall: [],
        ping: []
      };

      const iterations = 100;

      // tools/list
      for (let i = 10; i < 10 + iterations; i++) {
        const r = await sendAndWait(i, { jsonrpc: '2.0', method: 'tools/list', params: {}, id: i });
        results.toolsList.push(r.elapsed);
      }

      // tools/call (bench_echo 64 bytes)
      for (let i = 110; i < 110 + iterations; i++) {
        const r = await sendAndWait(i, { jsonrpc: '2.0', method: 'tools/call', params: { name: 'bench_echo', arguments: { size: 64 } }, id: i });
        results.toolsCall.push(r.elapsed);
      }

      // ping
      for (let i = 210; i < 210 + iterations; i++) {
        const r = await sendAndWait(i, { jsonrpc: '2.0', method: 'ping', params: {}, id: i });
        results.ping.push(r.elapsed);
      }

      proc.kill();

      function stats(arr) {
        const sorted = [...arr].sort((a, b) => a - b);
        const avg = arr.reduce((a, b) => a + b, 0) / arr.length;
        return {
          avg,
          p50: sorted[Math.floor(sorted.length * 0.50)],
          p95: sorted[Math.floor(sorted.length * 0.95)],
          p99: sorted[Math.floor(sorted.length * 0.99)]
        };
      }

      resolve({
        name,
        toolsList: stats(results.toolsList),
        toolsCall: stats(results.toolsCall),
        ping: stats(results.ping)
      });
    }

    run().then(resolve).catch(e => { proc.kill(); resolve({ name, error: e.message }); });
  });
}

function benchHttpServer(name, cmd, args, port) {
  return new Promise((resolve) => {
    const proc = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'pipe'] });
    proc.stderr.on('data', () => {});

    function httpRequest(obj) {
      return new Promise((res, rej) => {
        const body = JSON.stringify(obj);
        const sentAt = process.hrtime.bigint();
        const req = http.request({
          hostname: '127.0.0.1',
          port,
          path: '/v1/mcp',
          method: 'POST',
          headers: { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(body) }
        }, (resp) => {
          let data = '';
          resp.on('data', (chunk) => { data += chunk; });
          resp.on('end', () => {
            const receivedAt = process.hrtime.bigint();
            const elapsed = Number(receivedAt - sentAt) / 1e6;
            try {
              const parsed = data ? JSON.parse(data) : {};
              res({ resp: parsed, elapsed });
            } catch (e) {
              res({ resp: {}, elapsed });
            }
          });
        });
        req.on('error', rej);
        req.write(body);
        req.end();
      });
    }

    async function run() {
      // Wait for server
      for (let i = 0; i < 100; i++) {
        await new Promise(r => setTimeout(r, 50));
        try {
          const checkReq = http.request({ hostname: '127.0.0.1', port, path: '/health', method: 'GET' }, (res) => {
            res.resume();
          });
          checkReq.on('error', () => {});
          checkReq.end();
          await new Promise(r => setTimeout(r, 50));
          break;
        } catch (e) {}
      }

      await httpRequest({ jsonrpc: '2.0', method: 'initialize', params: { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'bench', version: '1.0' } }, id: 1 });

      const results = { toolsList: [], toolsCall: [], ping: [] };
      const iterations = 100;

      for (let i = 10; i < 10 + iterations; i++) {
        const r = await httpRequest({ jsonrpc: '2.0', method: 'tools/list', params: {}, id: i });
        results.toolsList.push(r.elapsed);
      }

      for (let i = 110; i < 110 + iterations; i++) {
        const r = await httpRequest({ jsonrpc: '2.0', method: 'tools/call', params: { name: 'bench_echo', arguments: { size: 64 } }, id: i });
        results.toolsCall.push(r.elapsed);
      }

      for (let i = 210; i < 210 + iterations; i++) {
        const r = await httpRequest({ jsonrpc: '2.0', method: 'ping', params: {}, id: i });
        results.ping.push(r.elapsed);
      }

      proc.kill();

      function stats(arr) {
        const sorted = [...arr].sort((a, b) => a - b);
        const avg = arr.reduce((a, b) => a + b, 0) / arr.length;
        return { avg, p50: sorted[Math.floor(sorted.length * 0.50)], p95: sorted[Math.floor(sorted.length * 0.95)], p99: sorted[Math.floor(sorted.length * 0.99)] };
      }

      resolve({ name, toolsList: stats(results.toolsList), toolsCall: stats(results.toolsCall), ping: stats(results.ping) });
    }

    run().then(resolve).catch(e => { proc.kill(); resolve({ name, error: e.message }); });
  });
}

function benchNdaShmem(name) {
  return new Promise((resolve) => {
    const proc = spawn(BENCH_NDA, [], { stdio: ['pipe', 'pipe', 'pipe'] });
    let output = '';

    proc.stdout.on('data', (chunk) => { output += chunk.toString(); });
    proc.stderr.on('data', (chunk) => { output += chunk.toString(); });

    proc.on('close', (code) => {
      const lines = output.split('\n');
      const results = { name, ping: {}, toolsList: {}, toolsCall: {}, jsonStdio: { ping: {}, toolsList: {}, toolsCall: {} } };

      let currentSection = null;
      for (const line of lines) {
        if (line.includes('─── Ping')) currentSection = 'ping';
        else if (line.includes('─── Tools/List')) currentSection = 'toolsList';
        else if (line.includes('─── Tools/Call')) currentSection = 'toolsCall';
        else if (line.includes('───') && currentSection) currentSection = null;

        if (!currentSection) continue;

        const avgMatch = line.match(/Avg latency\s+([\d.]+)\s*ms\s+([\d.]+)\s*ms\s+([\d.]+)\s*ms/);
        if (avgMatch) {
          results[currentSection].avg = parseFloat(avgMatch[1]);
          results.jsonStdio[currentSection].avg = parseFloat(avgMatch[2]);
        }

        const p99Match = line.match(/p99\s+([\d.]+)\s*ms\s+([\d.]+)\s*ms\s+([\d.]+)\s*ms/);
        if (p99Match) {
          results[currentSection].p99 = parseFloat(p99Match[1]);
          results.jsonStdio[currentSection].p99 = parseFloat(p99Match[2]);
        }

        const p50Match = line.match(/p50\s+([\d.]+)\s*ms\s+([\d.]+)\s*ms\s+([\d.]+)\s*ms/);
        if (p50Match) {
          results[currentSection].p50 = parseFloat(p50Match[1]);
        }

        const p95Match = line.match(/p95\s+([\d.]+)\s*ms\s+([\d.]+)\s*ms\s+([\d.]+)\s*ms/);
        if (p95Match) {
          results[currentSection].p95 = parseFloat(p95Match[1]);
        }
      }

      resolve(results);
    });

    proc.on('error', (e) => { resolve({ name, error: e.message }); });
  });
}

async function main() {
  console.log('6-Pipeline Benchmark: stdio + HTTP transports across Node.js and Rust\n');
  console.log('Pipelines:');
  console.log('  1. Node.js JSON/stdio  — JSON tools, JSON/stdio transport');
  console.log('  2. Rust JSON/stdio     — JSON tools, JSON/stdio transport');
  console.log('  3. Rust NDA-wrapped    — JSON→NDA converted tools, JSON/stdio transport');
  console.log('  4. Rust pure NDA       — native NDA tools, NDA/shmem transport');
  console.log('  5. Node.js JSON/HTTP   — JSON tools, JSON/HTTP transport');
  console.log('  6. Rust JSON/HTTP      — JSON tools, JSON/HTTP transport (Axum)');
  console.log('');

  // 1. Node.js JSON/stdio
  console.log('Running Node.js JSON/stdio...');
  const nodeJson = await benchStdioServer('Node.js JSON/stdio', 'node', [NODE_SERVER]);

  // 2. Rust JSON/stdio
  console.log('Running Rust JSON/stdio...');
  const rustJson = await benchStdioServer('Rust JSON/stdio', RUST_SERVER, ['--mode', 'stdio']);

  // 3. Rust NDA-wrapped (convert bench_echo to NDA first)
  console.log('Running Rust NDA-wrapped...');
  const rustNdaWrapped = await benchStdioServer('Rust NDA-wrapped', RUST_SERVER, ['--mode', 'stdio'], async (sendAndWait) => {
    await sendAndWait(5, {
      jsonrpc: '2.0',
      method: 'tools/call',
      params: {
        name: 'convert_to_nda_tool',
        arguments: {
          jsonRequest: JSON.stringify({
            jsonrpc: '2.0',
            method: 'tools/call',
            params: { name: 'bench_echo', arguments: { size: 64 } },
            id: 999
          })
        }
      },
      id: 5
    });
  });

  // 4. Rust pure NDA (NDA/shmem transport)
  console.log('Running Rust pure NDA...');
  const rustPureNda = await benchNdaShmem('Rust pure NDA');

  // 5. Node.js JSON/HTTP
  console.log('Running Node.js JSON/HTTP...');
  const nodeHttpPort = 13500 + Math.floor(Math.random() * 100);
  const nodeHttp = await benchHttpServer('Node.js JSON/HTTP', 'node', [NODE_SERVER, '--http', String(nodeHttpPort)], nodeHttpPort);

  // 6. Rust JSON/HTTP
  console.log('Running Rust JSON/HTTP...');
  const rustHttpPort = 13600 + Math.floor(Math.random() * 100);
  const rustHttp = await benchHttpServer('Rust JSON/HTTP', RUST_SERVER, ['--mode', 'http', '--addr', `127.0.0.1:${rustHttpPort}`], rustHttpPort);

  // Print results
  console.log('\n' + '='.repeat(90));
  console.log('  RESULTS');
  console.log('='.repeat(90));

  const pipelines = [nodeJson, rustJson, rustNdaWrapped, rustPureNda, nodeHttp, rustHttp];

  for (const method of ['ping', 'toolsList', 'toolsCall']) {
    const label = method.replace(/([A-Z])/g, ' $1').replace(/^./, s => s.toUpperCase());
    console.log(`\n─── ${label} ──────────────────────────────────────────────────────────────────────`);
    console.log(`  ${''.padEnd(22)} ${'Avg'.padStart(12)} ${'p50'.padStart(12)} ${'p95'.padStart(12)} ${'p99'.padStart(12)}`);

    for (const p of pipelines) {
      if (p.error) {
        console.log(`  ${p.name.padEnd(22)} ERROR: ${p.error}`);
        continue;
      }
      const data = p[method];
      if (!data || !data.avg) {
        console.log(`  ${p.name.padEnd(22)} ${'N/A'.padStart(12)} ${'N/A'.padStart(12)} ${'N/A'.padStart(12)} ${'N/A'.padStart(12)}`);
        continue;
      }
      console.log(`  ${p.name.padEnd(22)} ${(data.avg).toFixed(3).padStart(9)} ms ${(data.p50 || 0).toFixed(3).padStart(9)} ms ${(data.p95 || 0).toFixed(3).padStart(9)} ms ${(data.p99 || 0).toFixed(3).padStart(9)} ms`);
    }
  }

  console.log('\n' + '='.repeat(90));
  console.log('  ANALYSIS');
  console.log('='.repeat(90));

  // Service difference: Node.js JSON vs Rust JSON (both JSON/stdio)
  if (nodeJson.ping.avg && rustJson.ping.avg) {
    console.log(`\n  1. SERVICE DIFFERENCE (Node.js vs Rust, both JSON/stdio):`);
    console.log(`    tools/list: ${(nodeJson.toolsList.avg / rustJson.toolsList.avg).toFixed(1)}x`);
    console.log(`    tools/call: ${(nodeJson.toolsCall.avg / rustJson.toolsCall.avg).toFixed(1)}x`);
    console.log(`    ping:       ${(nodeJson.ping.avg / rustJson.ping.avg).toFixed(1)}x`);
  }

  // Tool format difference: Rust JSON vs Rust NDA-wrapped
  if (rustJson.toolsCall.avg && rustNdaWrapped.toolsCall.avg) {
    console.log(`\n  2. TOOL FORMAT (Rust JSON vs Rust NDA-wrapped, same JSON/stdio):`);
    console.log(`    tools/call: ${(rustJson.toolsCall.avg / rustNdaWrapped.toolsCall.avg).toFixed(1)}x (NDA binary exec vs JSON parse)`);
  }

  // Transport difference: JSON/stdio vs NDA/shmem
  if (rustPureNda.ping && rustPureNda.ping.avg && rustPureNda.jsonStdio && rustPureNda.jsonStdio.ping && rustPureNda.jsonStdio.ping.avg) {
    console.log(`\n  3. TRANSPORT (Rust JSON/stdio vs Rust NDA/shmem, from bench_nda):`);
    console.log(`    ping:       ${(rustPureNda.jsonStdio.ping.avg / rustPureNda.ping.avg).toFixed(1)}x`);
    if (rustPureNda.jsonStdio.toolsList && rustPureNda.toolsList.avg) {
      console.log(`    tools/list: ${(rustPureNda.jsonStdio.toolsList.avg / rustPureNda.toolsList.avg).toFixed(1)}x`);
    }
    if (rustPureNda.jsonStdio.toolsCall && rustPureNda.toolsCall.avg) {
      console.log(`    tools/call: ${(rustPureNda.jsonStdio.toolsCall.avg / rustPureNda.toolsCall.avg).toFixed(1)}x`);
    }
  }

  // HTTP overhead: Rust JSON/stdio vs Rust JSON/HTTP
  if (rustJson.ping.avg && rustHttp.ping.avg) {
    console.log(`\n  4. HTTP OVERHEAD (Rust JSON/stdio vs Rust JSON/HTTP):`);
    console.log(`    ping:       ${(rustHttp.ping.avg / rustJson.ping.avg).toFixed(1)}x`);
    console.log(`    tools/list: ${(rustHttp.toolsList.avg / rustJson.toolsList.avg).toFixed(1)}x`);
    console.log(`    tools/call: ${(rustHttp.toolsCall.avg / rustJson.toolsCall.avg).toFixed(1)}x`);
  }

  // Node.js HTTP overhead
  if (nodeJson.ping.avg && nodeHttp.ping.avg) {
    console.log(`\n  5. NODE HTTP OVERHEAD (Node.js JSON/stdio vs Node.js JSON/HTTP):`);
    console.log(`    ping:       ${(nodeHttp.ping.avg / nodeJson.ping.avg).toFixed(1)}x`);
    console.log(`    tools/list: ${(nodeHttp.toolsList.avg / nodeJson.toolsList.avg).toFixed(1)}x`);
    console.log(`    tools/call: ${(nodeHttp.toolsCall.avg / nodeJson.toolsCall.avg).toFixed(1)}x`);
  }

  // Full stack: Node.js JSON/stdio vs Rust NDA/shmem
  if (nodeJson.ping.avg && rustPureNda.ping && rustPureNda.ping.avg) {
    console.log(`\n  6. FULL STACK (Node.js JSON/stdio vs Rust NDA/shmem):`);
    console.log(`    ping:       ${(nodeJson.ping.avg / rustPureNda.ping.avg).toFixed(1)}x`);
    if (rustPureNda.toolsList && rustPureNda.toolsList.avg) {
      console.log(`    tools/list: ${(nodeJson.toolsList.avg / rustPureNda.toolsList.avg).toFixed(1)}x`);
    }
    if (rustPureNda.toolsCall && rustPureNda.toolsCall.avg) {
      console.log(`    tools/call: ${(nodeJson.toolsCall.avg / rustPureNda.toolsCall.avg).toFixed(1)}x`);
    }
  }
}

main();
