#!/usr/bin/env python3
"""
Thorough MCP benchmark - measures sustained performance over long-lived sessions.

This benchmark properly models how MCP servers are actually used:
1. Start server ONCE
2. Initialize connection
3. Call tools MANY times (simulating real AI assistant workflow)
4. Measure steady-state performance, not per-call startup costs
5. Track memory growth over time
6. Test concurrent requests
7. Measure error rates and reliability
"""

import subprocess
import time
import json
import sys
import os
import statistics
from pathlib import Path
from datetime import datetime

TEST_DIR = "C:/tmp/benchmark_test"
FS_SERVER_PATH = "C:/Users/ian/AppData/Roaming/npm/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"
VELOCITY_MCP_PATH = "C:/Users/ian/Documents/MCP/target/release/velocity_mcp.exe"


class MCPServerBenchmark:
    """Benchmark an MCP server with realistic workload patterns."""

    def __init__(self, name, cmd, test_dir):
        self.name = name
        self.cmd = cmd
        self.test_dir = test_dir
        self.proc = None
        self.results = {}

    def start_server(self):
        """Start the MCP server process."""
        env = os.environ.copy()
        env["RUST_LOG"] = "error"

        if sys.platform == "win32":
            node_path = r"C:\Program Files\nodejs"
            if node_path not in env.get("PATH", ""):
                env["PATH"] = f"{node_path};{env.get('PATH', '')}"

        self.proc = subprocess.Popen(
            self.cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=0,  # Unbuffered
            env=env,
        )

        # Wait for server to be ready
        time.sleep(0.5 if "node" in str(self.cmd) else 0.1)
        return self.proc

    def send_request(self, request, timeout=5.0):
        """Send JSON-RPC request and get response."""
        try:
            self.proc.stdin.write(json.dumps(request) + "\n")
            self.proc.stdin.flush()

            start = time.time()
            buffer = ""
            while time.time() - start < timeout:
                char = self.proc.stdout.read(1)
                if not char:
                    # EOF - check if process died
                    if self.proc.poll() is not None:
                        stderr_out = self.proc.stderr.read()
                        print(f"Process exited: {stderr_out[:200]}")
                        break
                    continue
                buffer += char
                if char == '\n':
                    line = buffer.strip()
                    buffer = ""
                    if line:
                        try:
                            return json.loads(line)
                        except json.JSONDecodeError:
                            continue  # Skip log messages
            return None
        except Exception as e:
            print(f"Request error: {e}")
            return None

    def initialize(self):
        """Initialize MCP connection."""
        init_req = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "benchmark", "version": "1.0"}
            }
        }

        start = time.time()
        response = self.send_request(init_req)
        init_time_ms = (time.time() - start) * 1000

        # Send initialized notification
        notif = {
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }
        self.send_request(notif)
        time.sleep(0.1)

        return response, init_time_ms

    def get_memory_mb(self):
        """Get current memory usage in MB."""
        try:
            pid = self.proc.pid
            result = subprocess.run(
                ["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"],
                capture_output=True,
                text=True,
            )

            if result.returncode == 0 and result.stdout:
                parts = result.stdout.strip().split(",")
                if len(parts) >= 5:
                    mem_str = parts[4].strip('"').replace(",", "").replace("K", "")
                    try:
                        mem_kb = int(mem_str)
                        return mem_kb / 1024
                    except ValueError:
                        pass
            return None
        except:
            return None

    def benchmark_sustained_load(self, tool_calls, iterations=100):
        """Benchmark sustained tool call performance."""
        latencies = []
        errors = 0

        print(f"\n  Running {iterations} tool calls...")
        for i in range(iterations):
            req_id = 1000 + i
            tool_spec = tool_calls[i % len(tool_calls)]

            # Build request
            if callable(tool_spec["args"]):
                args = tool_spec["args"](i)
            else:
                args = tool_spec["args"]

            request = {
                "jsonrpc": "2.0",
                "id": req_id,
                "method": "tools/call",
                "params": {
                    "name": tool_spec["name"],
                    "arguments": args
                }
            }

            # Time the call
            start = time.time()
            response = self.send_request(request, timeout=10.0)
            elapsed_ms = (time.time() - start) * 1000

            if response and response.get("result"):
                latencies.append(elapsed_ms)
            else:
                errors += 1
                if errors <= 3:
                    print(f"    Error on call {i}: {response}")

            # Progress indicator
            if (i + 1) % 20 == 0:
                print(f"    Completed {i + 1}/{iterations} calls...")

        if latencies:
            latencies.sort()
            return {
                "count": len(latencies),
                "errors": errors,
                "success_rate": (len(latencies) / iterations) * 100,
                "mean_ms": statistics.mean(latencies),
                "median_ms": statistics.median(latencies),
                "p95_ms": latencies[int(len(latencies) * 0.95)],
                "p99_ms": latencies[int(len(latencies) * 0.99)],
                "min_ms": min(latencies),
                "max_ms": max(latencies),
                "stddev_ms": statistics.stdev(latencies) if len(latencies) > 1 else 0,
                "throughput_rps": len(latencies) / (sum(latencies) / 1000),
            }
        return None

    def benchmark_memory_growth(self, duration_secs=30, calls_per_sec=10):
        """Measure memory growth over time under load."""
        samples = []
        total_calls = duration_secs * calls_per_sec

        print(f"\n  Measuring memory over {duration_secs}s ({total_calls} calls)...")

        for i in range(total_calls):
            # Make a tool call
            request = {
                "jsonrpc": "2.0",
                "id": 2000 + i,
                "method": "tools/call",
                "params": {
                    "name": "read_file",
                    "arguments": {"path": f"{self.test_dir}/file1.txt"}
                }
            }
            self.send_request(request, timeout=2.0)

            # Sample memory every second
            if i % calls_per_sec == 0:
                mem = self.get_memory_mb()
                if mem:
                    samples.append({
                        "time_sec": i // calls_per_sec,
                        "memory_mb": mem
                    })
                    if (i // calls_per_sec) % 5 == 0:
                        print(f"    t={i // calls_per_sec}s: {mem:.1f} MB")

            time.sleep(1.0 / calls_per_sec)

        return samples

    def benchmark_concurrent(self, tool_calls, num_workers=4, calls_per_worker=25):
        """Simulate concurrent access (multiple rapid sequential calls)."""
        all_latencies = []

        print(f"\n  Simulating {num_workers} concurrent workers ({calls_per_worker} calls each)...")

        for worker in range(num_workers):
            worker_latencies = []

            for i in range(calls_per_worker):
                req_id = 3000 + worker * 1000 + i
                tool_spec = tool_calls[i % len(tool_calls)]

                if callable(tool_spec["args"]):
                    args = tool_spec["args"](i)
                else:
                    args = tool_spec["args"]

                request = {
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "method": "tools/call",
                    "params": {
                        "name": tool_spec["name"],
                        "arguments": args
                    }
                }

                start = time.time()
                response = self.send_request(request, timeout=5.0)
                elapsed_ms = (time.time() - start) * 1000

                if response and response.get("result"):
                    worker_latencies.append(elapsed_ms)

            all_latencies.extend(worker_latencies)
            print(f"    Worker {worker + 1}: {len(worker_latencies)}/{calls_per_worker} successful")

        if all_latencies:
            all_latencies.sort()
            return {
                "total_calls": len(all_latencies),
                "mean_ms": statistics.mean(all_latencies),
                "median_ms": statistics.median(all_latencies),
                "p95_ms": all_latencies[int(len(all_latencies) * 0.95)],
                "p99_ms": all_latencies[int(len(all_latencies) * 0.99)],
                "throughput_rps": len(all_latencies) / (sum(all_latencies) / 1000),
            }
        return None

    def cleanup(self):
        """Stop the server."""
        if self.proc:
            try:
                self.proc.kill()
                self.proc.wait(timeout=5)
            except:
                pass


def setup_test_environment(test_dir):
    """Create test directory structure with various files."""
    Path(test_dir).mkdir(parents=True, exist_ok=True)
    (Path(test_dir) / "subdir1").mkdir(exist_ok=True)
    (Path(test_dir) / "subdir2").mkdir(exist_ok=True)

    # Create test files of various sizes
    (Path(test_dir) / "file1.txt").write_text("Hello World - benchmark test file content here for testing")
    (Path(test_dir) / "file2.log").write_text("Log entry 1\nLog entry 2\nLog entry 3\n" * 100)
    (Path(test_dir) / "large_file.txt").write_text("X" * 10000)  # 10KB file
    (Path(test_dir) / "subdir1" / "nested.txt").write_text("Nested content in subdir1")
    (Path(test_dir) / "subdir2" / "deep.txt").write_text("Deep nested content")


def main():
    print("="*80)
    print("THOROUGH MCP SERVER BENCHMARK")
    print("="*80)
    print(f"Date: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print(f"Test Directory: {TEST_DIR}")
    print()

    # Setup test environment
    setup_test_environment(TEST_DIR)

    # Define tool call patterns
    filesystem_tools = [
        {"name": "read_file", "args": {"path": f"{TEST_DIR}\\file1.txt"}},
        {"name": "list_directory", "args": {"path": TEST_DIR}},
        {"name": "read_file", "args": lambda i: {"path": f"{TEST_DIR}\\file{'1' if i % 2 == 0 else '2'}.txt"}},
        {"name": "search_files", "args": {"path": TEST_DIR, "pattern": "*.txt"}},
    ]

    velocity_tools = [
        {"name": "file_read", "args": {"path": f"{TEST_DIR}\\file1.txt"}},
        {"name": "list_directory", "args": {"path": TEST_DIR}},
        {"name": "file_read", "args": lambda i: {"path": f"{TEST_DIR}\\file{'1' if i % 2 == 0 else '2'}.txt"}},
        {"name": "search_files", "args": {"path": TEST_DIR, "pattern": "*.txt"}},
    ]

    results = {}

    # ========================================================================
    # 1. OFFICIAL FILESYSTEM SERVER
    # ========================================================================
    print("\n" + "="*80)
    print("1. OFFICIAL FILESYSTEM SERVER (@modelcontextprotocol/server-filesystem v0.2.0)")
    print("="*80)

    official = MCPServerBenchmark(
        "filesystem-official",
        ["node", FS_SERVER_PATH, TEST_DIR],
        TEST_DIR
    )

    try:
        # Start server
        print("\nStarting server...")
        proc = official.start_server()

        # Initialize
        print("Initializing MCP connection...")
        init_resp, init_time = official.initialize()
        print(f"  Init time: {init_time:.1f} ms")
        if init_resp:
            print(f"  Server: {init_resp.get('result', {}).get('serverInfo', {})}")

        # Get baseline memory
        baseline_mem = official.get_memory_mb()
        print(f"  Baseline memory: {baseline_mem:.1f} MB" if baseline_mem else "  Memory: unknown")

        # Sustained load test (100 calls)
        print("\n--- Sustained Load Test (100 calls) ---")
        sustained = official.benchmark_sustained_load(filesystem_tools, iterations=100)
        if sustained:
            print(f"  Success rate: {sustained['success_rate']:.1f}%")
            print(f"  Mean latency: {sustained['mean_ms']:.2f} ms")
            print(f"  Median latency: {sustained['median_ms']:.2f} ms")
            print(f"  P95 latency: {sustained['p95_ms']:.2f} ms")
            print(f"  P99 latency: {sustained['p99_ms']:.2f} ms")
            print(f"  Std dev: {sustained['stddev_ms']:.2f} ms")
            print(f"  Throughput: {sustained['throughput_rps']:.1f} req/s")
            print(f"  Errors: {sustained['errors']}")

        # Memory growth test (30 seconds)
        print("\n--- Memory Growth Test (30s) ---")
        mem_samples = official.benchmark_memory_growth(duration_secs=30, calls_per_sec=10)
        if mem_samples and len(mem_samples) >= 2:
            initial_mem = mem_samples[0]['memory_mb']
            final_mem = mem_samples[-1]['memory_mb']
            peak_mem = max(s['memory_mb'] for s in mem_samples)
            print(f"  Initial: {initial_mem:.1f} MB")
            print(f"  Final: {final_mem:.1f} MB")
            print(f"  Peak: {peak_mem:.1f} MB")
            print(f"  Growth: {final_mem - initial_mem:+.1f} MB ({((final_mem - initial_mem) / initial_mem * 100):+.1f}%)")

        # Concurrent simulation
        print("\n--- Concurrent Access Simulation (4 workers x 25 calls) ---")
        concurrent = official.benchmark_concurrent(filesystem_tools, num_workers=4, calls_per_worker=25)
        if concurrent:
            print(f"  Total calls: {concurrent['total_calls']}")
            print(f"  Mean latency: {concurrent['mean_ms']:.2f} ms")
            print(f"  P95 latency: {concurrent['p95_ms']:.2f} ms")
            print(f"  Throughput: {concurrent['throughput_rps']:.1f} req/s")

        results["official"] = {
            "init_time_ms": init_time,
            "baseline_memory_mb": baseline_mem,
            "sustained": sustained,
            "memory_samples": mem_samples,
            "concurrent": concurrent,
        }

    finally:
        official.cleanup()

    # ========================================================================
    # 2. VELOCITY-MCP
    # ========================================================================
    print("\n" + "="*80)
    print("2. VELOCITY-MCP v3.0.0 (Rust)")
    print("="*80)

    velocity = MCPServerBenchmark(
        "velocity-mcp",
        [VELOCITY_MCP_PATH],
        TEST_DIR
    )

    try:
        # Start server
        print("\nStarting server...")
        proc = velocity.start_server()

        # Initialize
        print("Initializing MCP connection...")
        init_resp, init_time = velocity.initialize()
        print(f"  Init time: {init_time:.1f} ms")

        # Get baseline memory
        baseline_mem = velocity.get_memory_mb()
        print(f"  Baseline memory: {baseline_mem:.1f} MB" if baseline_mem else "  Memory: unknown")

        # Sustained load test (100 calls)
        print("\n--- Sustained Load Test (100 calls) ---")
        sustained = velocity.benchmark_sustained_load(velocity_tools, iterations=100)
        if sustained:
            print(f"  Success rate: {sustained['success_rate']:.1f}%")
            print(f"  Mean latency: {sustained['mean_ms']:.2f} ms")
            print(f"  Median latency: {sustained['median_ms']:.2f} ms")
            print(f"  P95 latency: {sustained['p95_ms']:.2f} ms")
            print(f"  P99 latency: {sustained['p99_ms']:.2f} ms")
            print(f"  Std dev: {sustained['stddev_ms']:.2f} ms")
            print(f"  Throughput: {sustained['throughput_rps']:.1f} req/s")
            print(f"  Errors: {sustained['errors']}")

        # Memory growth test (30 seconds)
        print("\n--- Memory Growth Test (30s) ---")
        mem_samples = velocity.benchmark_memory_growth(duration_secs=30, calls_per_sec=10)
        if mem_samples and len(mem_samples) >= 2:
            initial_mem = mem_samples[0]['memory_mb']
            final_mem = mem_samples[-1]['memory_mb']
            peak_mem = max(s['memory_mb'] for s in mem_samples)
            print(f"  Initial: {initial_mem:.1f} MB")
            print(f"  Final: {final_mem:.1f} MB")
            print(f"  Peak: {peak_mem:.1f} MB")
            print(f"  Growth: {final_mem - initial_mem:+.1f} MB ({((final_mem - initial_mem) / initial_mem * 100):+.1f}%)")

        # Concurrent simulation
        print("\n--- Concurrent Access Simulation (4 workers x 25 calls) ---")
        concurrent = velocity.benchmark_concurrent(velocity_tools, num_workers=4, calls_per_worker=25)
        if concurrent:
            print(f"  Total calls: {concurrent['total_calls']}")
            print(f"  Mean latency: {concurrent['mean_ms']:.2f} ms")
            print(f"  P95 latency: {concurrent['p95_ms']:.2f} ms")
            print(f"  Throughput: {concurrent['throughput_rps']:.1f} req/s")

        results["velocity"] = {
            "init_time_ms": init_time,
            "baseline_memory_mb": baseline_mem,
            "sustained": sustained,
            "memory_samples": mem_samples,
            "concurrent": concurrent,
        }

    finally:
        velocity.cleanup()

    # ========================================================================
    # 3. COMPARISON
    # ========================================================================
    print("\n" + "="*80)
    print("COMPARISON SUMMARY")
    print("="*80)

    if results["official"]["sustained"] and results["velocity"]["sustained"]:
        off = results["official"]["sustained"]
        vel = results["velocity"]["sustained"]

        print("\n📊 Sustained Load Performance (100 calls):")
        print(f"  {'Metric':<20} {'Official':>12} {'VELOCITY':>12} {'Speedup':>10}")
        print(f"  {'-'*60}")
        print(f"  {'Mean Latency':<20} {off['mean_ms']:>10.2f} ms {vel['mean_ms']:>10.2f} ms {off['mean_ms']/vel['mean_ms']:>8.2f}x")
        print(f"  {'Median Latency':<20} {off['median_ms']:>10.2f} ms {vel['median_ms']:>10.2f} ms {off['median_ms']/vel['median_ms']:>8.2f}x")
        print(f"  {'P95 Latency':<20} {off['p95_ms']:>10.2f} ms {vel['p95_ms']:>10.2f} ms {off['p95_ms']/vel['p95_ms']:>8.2f}x")
        print(f"  {'P99 Latency':<20} {off['p99_ms']:>10.2f} ms {vel['p99_ms']:>10.2f} ms {off['p99_ms']/vel['p99_ms']:>8.2f}x")
        print(f"  {'Std Deviation':<20} {off['stddev_ms']:>10.2f} ms {vel['stddev_ms']:>10.2f} ms {'':>8}")
        print(f"  {'Throughput':<20} {off['throughput_rps']:>10.1f} rps {vel['throughput_rps']:>10.1f} rps {vel['throughput_rps']/off['throughput_rps']:>8.2f}x")
        print(f"  {'Success Rate':<20} {off['success_rate']:>10.1f}% {vel['success_rate']:>10.1f}% {'':>8}")

    if results["official"]["concurrent"] and results["velocity"]["concurrent"]:
        off_c = results["official"]["concurrent"]
        vel_c = results["velocity"]["concurrent"]

        print("\n⚡ Concurrent Access Performance (100 calls across 4 workers):")
        print(f"  {'Metric':<20} {'Official':>12} {'VELOCITY':>12} {'Speedup':>10}")
        print(f"  {'-'*60}")
        print(f"  {'Mean Latency':<20} {off_c['mean_ms']:>10.2f} ms {vel_c['mean_ms']:>10.2f} ms {off_c['mean_ms']/vel_c['mean_ms']:>8.2f}x")
        print(f"  {'P95 Latency':<20} {off_c['p95_ms']:>10.2f} ms {vel_c['p95_ms']:>10.2f} ms {off_c['p95_ms']/vel_c['p95_ms']:>8.2f}x")
        print(f"  {'Throughput':<20} {off_c['throughput_rps']:>10.1f} rps {vel_c['throughput_rps']:>10.1f} rps {vel_c['throughput_rps']/off_c['throughput_rps']:>8.2f}x")

    if results["official"].get("memory_samples") and results["velocity"].get("memory_samples"):
        off_mem = results["official"]["memory_samples"]
        vel_mem = results["velocity"]["memory_samples"]

        if off_mem and vel_mem:
            print("\n💾 Memory Usage:")
            print(f"  Official:  {off_mem[0]['memory_mb']:.1f} MB → {off_mem[-1]['memory_mb']:.1f} MB ({off_mem[-1]['memory_mb'] - off_mem[0]['memory_mb']:+.1f} MB)")
            print(f"  VELOCITY:  {vel_mem[0]['memory_mb']:.1f} MB → {vel_mem[-1]['memory_mb']:.1f} MB ({vel_mem[-1]['memory_mb'] - vel_mem[0]['memory_mb']:+.1f} MB)")

    # Save detailed results
    output_file = Path("C:/Users/ian/Documents/MCP/benchmark_thorough_results.json")
    with open(output_file, "w") as f:
        json.dump(results, f, indent=2, default=str)
    print(f"\n📁 Detailed results saved to: {output_file}")

    print("\n" + "="*80)
    print("BENCHMARK COMPLETE")
    print("="*80)


if __name__ == "__main__":
    main()
