#!/usr/bin/env python3
"""
Proper MCP Server Benchmark Harness

Benchmarks official MCP servers vs VELOCITY-MCP with real measurements.
Uses proper JSON-RPC communication over stdio.
"""

import asyncio
import json
import os
import platform
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from statistics import mean, median
from typing import Any


@dataclass
class BenchmarkResult:
    """Stores benchmark results."""
    server_name: str
    tool_name: str
    iterations: int
    latencies_ms: list[float] = field(default_factory=list)
    startup_time_ms: float = 0.0
    memory_mb: float = 0.0
    errors: int = 0

    @property
    def p50(self) -> float:
        return median(self.latencies_ms) if self.latencies_ms else 0

    @property
    def p95(self) -> float:
        sorted_lat = sorted(self.latencies_ms)
        idx = int(len(sorted_lat) * 0.95)
        return sorted_lat[min(idx, len(sorted_lat) - 1)] if sorted_lat else 0

    @property
    def mean(self) -> float:
        return mean(self.latencies_ms) if self.latencies_ms else 0

    @property
    def throughput(self) -> float:
        total_time = sum(self.latencies_ms) / 1000
        return len(self.latencies_ms) / total_time if total_time > 0 else 0


class MCPBenchmarkHarness:
    """Benchmarks MCP servers with proper JSON-RPC communication."""

    def __init__(self, iterations: int = 50):
        self.iterations = iterations
        self.results: list[BenchmarkResult] = []

    def get_process_memory_mb(self, proc: subprocess.Popen) -> float:
        """Get RSS memory usage in MB using platform-specific methods."""
        try:
            if platform.system() == "Windows":
                # Use tasklist on Windows
                result = subprocess.run(
                    ["tasklist", "/FI", f"PID eq {proc.pid}", "/FO", "CSV", "/NH"],
                    capture_output=True,
                    text=True,
                    timeout=5
                )
                if result.returncode == 0 and result.stdout.strip():
                    # Parse CSV output
                    parts = result.stdout.strip().split(",")
                    if len(parts) >= 5:
                        mem_str = parts[4].strip('"').replace(" ", "").replace("K", "")
                        try:
                            mem_kb = int(mem_str)
                            return mem_kb / 1024  # Convert KB to MB
                        except ValueError:
                            pass
            else:
                # Use ps on Unix-like systems
                result = subprocess.run(
                    ["ps", "-o", "rss=", "-p", str(proc.pid)],
                    capture_output=True,
                    text=True,
                    timeout=5
                )
                if result.returncode == 0 and result.stdout.strip():
                    mem_kb = int(result.stdout.strip())
                    return mem_kb / 1024
        except Exception as e:
            print(f"Warning: Could not get memory for PID {proc.pid}: {e}")
        return 0.0

    async def benchmark_server(
        self,
        name: str,
        cmd: list[str],
        tools_to_test: list[tuple[str, dict]],
        setup_fn=None,
    ) -> list[BenchmarkResult]:
        """Benchmark an MCP server."""
        print(f"\n{'='*80}")
        print(f"Benchmarking: {name}")
        print(f"Command: {' '.join(cmd)}")
        print(f"Iterations: {self.iterations}")
        print(f"{'='*80}")

        results = []

        # Start server
        print("\nStarting server...")
        start_time = time.time()

        env = os.environ.copy()
        env["RUST_LOG"] = "error"  # Suppress logs for VELOCITY-MCP
        
        # Ensure Node.js is in PATH on Windows
        if sys.platform == "win32":
            node_path = r"C:\Program Files\nodejs"
            if node_path not in env.get("PATH", ""):
                env["PATH"] = f"{node_path};{env.get('PATH', '')}"

        proc = subprocess.Popen(
            cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
            env=env,
            shell=sys.platform == "win32",  # Use shell on Windows for npx resolution
        )

        # Wait for server to be ready (simple heuristic)
        await asyncio.sleep(0.5)
        startup_ms = (time.time() - start_time) * 1000
        print(f"Startup time: {startup_ms:.1f} ms")

        # Get memory usage
        memory_mb = self.get_process_memory_mb(proc)
        print(f"Memory usage: {memory_mb:.1f} MB")

        # Helper to send JSON-RPC and receive response
        def send_request(request: dict, timeout: float = 5.0) -> dict | None:
            try:
                proc.stdin.write(json.dumps(request) + "\n")
                proc.stdin.flush()

                # Read response with timeout
                start = time.time()
                while time.time() - start < timeout:
                    line = proc.stdout.readline()
                    if line:
                        line = line.strip()
                        if line:
                            try:
                                return json.loads(line)
                            except json.JSONDecodeError:
                                continue  # Skip non-JSON lines (logs)
                    time.sleep(0.01)
                return None
            except Exception as e:
                print(f"Error sending request: {e}")
                return None

        # Initialize MCP connection
        print("\nInitializing MCP connection...")
        init_resp = send_request({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "benchmark", "version": "1.0.0"}
            },
            "id": 1,
        })

        if init_resp:
            server_info = init_resp.get("result", {}).get("serverInfo", {})
            print(f"Server: {server_info.get('name', 'unknown')} v{server_info.get('version', 'unknown')}")
        else:
            print("Warning: No initialization response")

        # Send initialized notification
        send_request({"jsonrpc": "2.0", "method": "notifications/initialized"})

        # Run setup function if provided (e.g., create test files)
        if setup_fn:
            setup_fn()

        # Benchmark each tool
        for tool_name, arguments in tools_to_test:
            print(f"\nBenchmarking tool: {tool_name} ({self.iterations} iterations)...")

            result = BenchmarkResult(
                server_name=name,
                tool_name=tool_name,
                iterations=self.iterations,
                startup_time_ms=startup_ms,
                memory_mb=memory_mb,
            )

            for i in range(self.iterations):
                start = time.time()
                resp = send_request({
                    "jsonrpc": "2.0",
                    "method": "tools/call",
                    "params": {
                        "name": tool_name,
                        "arguments": arguments(i) if callable(arguments) else arguments,
                    },
                    "id": i + 2,
                })
                elapsed_ms = (time.time() - start) * 1000

                if resp and "result" in resp and not resp["result"].get("isError"):
                    result.latencies_ms.append(elapsed_ms)
                else:
                    result.errors += 1
                    if i < 3:  # Show first few errors
                        print(f"  Error on iteration {i}: {resp}")

            if result.latencies_ms:
                print(f"  ✓ Success: {len(result.latencies_ms)}/{self.iterations}")
                print(f"    P50: {result.p50:.3f} ms, P95: {result.p95:.3f} ms, Mean: {result.mean:.3f} ms")
                print(f"    Throughput: {result.throughput:.1f} req/s")
            else:
                print(f"  ✗ Failed: {result.errors}/{self.iterations} errors")

            results.append(result)

        # Cleanup
        print("\nStopping server...")
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()

        return results

    async def run_all_benchmarks(self):
        """Run all benchmarks."""
        print("="*80)
        print("MCP SERVER BENCHMARK SUITE")
        print("="*80)

        # Create test directory structure
        test_dir = Path("C:/tmp/benchmark_test")
        test_dir.mkdir(parents=True, exist_ok=True)
        (test_dir / "subdir1").mkdir(exist_ok=True)
        (test_dir / "subdir2").mkdir(exist_ok=True)
        (test_dir / "file1.txt").write_text("Hello World - benchmark test file")
        (test_dir / "file2.log").write_text("Log entry 1\nLog entry 2")
        (test_dir / "subdir1" / "nested.txt").write_text("Nested content")

        # Setup function for filesystem tests
        def setup_filesystem():
            pass  # Already created above

        # 1. Official Filesystem Server (Node.js)
        # Use globally installed package directly for reliability
        fs_server_path = "C:/Users/ian/AppData/Roaming/npm/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"
        test_dir_unix = str(test_dir).replace("C:/", "/c/") if sys.platform == "win32" else str(test_dir)
        
        fs_results = await self.benchmark_server(
            name="filesystem-official (Node.js)",
            cmd=["node", fs_server_path, test_dir_unix],
            tools_to_test=[
                ("read_file", {"path": str(test_dir / "file1.txt")}),
                ("write_file", lambda i: {"path": str(test_dir / f"output_{i}.txt"), "content": f"Test {i}"}),
                ("list_directory", {"path": str(test_dir)}),
            ],
            setup_fn=setup_filesystem,
        )
        self.results.extend(fs_results)

        await asyncio.sleep(2)  # Cooldown

        # 2. VELOCITY-MCP Filesystem Tools
        velocity_cmd = [str(Path("C:/Users/ian/Documents/MCP/target/release/velocity_mcp.exe")), "--mode", "stdio"]
        velocity_results = await self.benchmark_server(
            name="velocity-mcp (Rust)",
            cmd=velocity_cmd,
            tools_to_test=[
                ("file_read", {"path": str(test_dir / "file1.txt")}),
                ("file_write", lambda i: {"path": str(test_dir / f"velocity_{i}.txt"), "content": f"Test {i}"}),
                ("list_directory", {"path": str(test_dir)}),
            ],
            setup_fn=setup_filesystem,
        )
        self.results.extend(velocity_results)

        # Generate report
        self.generate_report()

    def generate_report(self):
        """Generate comprehensive comparison report."""
        print("\n" + "="*80)
        print("BENCHMARK RESULTS")
        print("="*80)

        # Group by tool type
        comparisons = {
            "File Read": [r for r in self.results if r.tool_name in ["read_file", "file_read"]],
            "File Write": [r for r in self.results if r.tool_name in ["write_file", "file_write"]],
            "List Directory": [r for r in self.results if r.tool_name == "list_directory"],
        }

        for category, results in comparisons.items():
            if not results:
                continue

            print(f"\n{category}:")
            print("-" * 80)
            print(f"{'Server':<30} {'Startup (ms)':<15} {'Memory (MB)':<15} {'P50 (ms)':<12} {'Mean (ms)':<12} {'Throughput':<15}")
            print("-" * 80)

            for r in sorted(results, key=lambda x: x.mean if x.mean > 0 else float('inf')):
                print(
                    f"{r.server_name:<30} {r.startup_time_ms:<15.1f} {r.memory_mb:<15.1f} "
                    f"{r.p50:<12.3f} {r.mean:<12.3f} {r.throughput:<15.1f}"
                )

            # Calculate speedup
            if len(results) >= 2:
                slowest = max(results, key=lambda x: x.mean if x.mean > 0 else 0)
                fastest = min(results, key=lambda x: x.mean if x.mean > 0 else float('inf'))
                if fastest.mean > 0 and slowest.mean > 0:
                    speedup = slowest.mean / fastest.mean
                    print(f"\n  → {fastest.server_name} is {speedup:.1f}x faster than {slowest.server_name}")

                    # Memory savings
                    if slowest.memory_mb > 0:
                        mem_savings = (1 - fastest.memory_mb / slowest.memory_mb) * 100
                        print(f"  → {fastest.server_name} uses {mem_savings:.0f}% less memory")

                    # Startup improvement
                    if slowest.startup_time_ms > 0:
                        startup_speedup = slowest.startup_time_ms / fastest.startup_time_ms
                        print(f"  → {fastest.server_name} starts {startup_speedup:.1f}x faster")

        # Save detailed results
        output_file = Path("benchmark_results.json")
        with open(output_file, "w") as f:
            json.dump([
                {
                    "server": r.server_name,
                    "tool": r.tool_name,
                    "iterations": r.iterations,
                    "startup_ms": round(r.startup_time_ms, 2),
                    "memory_mb": round(r.memory_mb, 2),
                    "p50_ms": round(r.p50, 3),
                    "p95_ms": round(r.p95, 3),
                    "mean_ms": round(r.mean, 3),
                    "throughput_rps": round(r.throughput, 1),
                    "errors": r.errors,
                }
                for r in self.results
            ], f, indent=2)

        print(f"\nDetailed results saved to: {output_file}")
        print("="*80)


async def main():
    harness = MCPBenchmarkHarness(iterations=50)
    await harness.run_all_benchmarks()


if __name__ == "__main__":
    asyncio.run(main())
