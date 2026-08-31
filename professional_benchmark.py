#!/usr/bin/env python3
"""
Professional MCP Benchmark using official MCP Python SDK.

This benchmark uses the proper MCP client library to ensure:
- Correct protocol handling
- No I/O buffering issues
- Real-world usage patterns
- Credible, reproducible results
"""

import asyncio
import time
import statistics
import sys
from pathlib import Path
from datetime import datetime
from contextlib import asynccontextmanager

try:
    from mcp import ClientSession, StdioServerParameters
    from mcp.client.stdio import stdio_client
except ImportError:
    print("ERROR: MCP SDK not installed. Install with: pip install mcp")
    sys.exit(1)

TEST_DIR = "C:/tmp/benchmark_test"


def setup_test_environment(test_dir):
    """Create test directory structure."""
    Path(test_dir).mkdir(parents=True, exist_ok=True)
    (Path(test_dir) / "subdir1").mkdir(exist_ok=True)
    (Path(test_dir) / "subdir2").mkdir(exist_ok=True)

    (Path(test_dir) / "file1.txt").write_text("Hello World - benchmark test file content")
    (Path(test_dir) / "file2.log").write_text("Log entry\n" * 100)
    (Path(test_dir) / "large_file.txt").write_text("X" * 10000)
    (Path(test_dir) / "subdir1" / "nested.txt").write_text("Nested content")


async def benchmark_server(name, server_params, tools_config, num_calls=200):
    """Benchmark an MCP server using the official SDK."""

    print(f"\n{'='*80}")
    print(f"Benchmarking: {name}")
    print(f"{'='*80}")

    results = {
        "name": name,
        "num_calls": num_calls,
    }

    try:
        # Connect to server
        print(f"\nConnecting to server...")
        start_connect = time.time()

        async with stdio_client(server_params) as (read, write):
            connect_time = (time.time() - start_connect) * 1000
            print(f"  Connection established: {connect_time:.1f} ms")

            async with ClientSession(read, write) as session:
                # Initialize
                print(f"Initializing...")
                start_init = time.time()
                init_result = await session.initialize()
                init_time = (time.time() - start_init) * 1000
                print(f"  Initialized in: {init_time:.1f} ms")
                print(f"  Server info: {init_result.serverInfo}")

                results["init_time_ms"] = init_time
                results["server_info"] = str(init_result.serverInfo)

                # List available tools
                tools_result = await session.list_tools()
                print(f"  Available tools: {len(tools_result.tools)}")

                # Warmup (5 calls)
                print(f"\nWarming up (5 calls)...")
                for i in range(5):
                    tool_name = tools_config[i % len(tools_config)]["name"]
                    args = tools_config[i % len(tools_config)]["args"]
                    if callable(args):
                        args = args(i)
                    try:
                        await session.call_tool(tool_name, args)
                    except Exception as e:
                        print(f"  Warmup call {i} failed: {e}")

                # Sustained load test
                print(f"\nRunning sustained load test ({num_calls} calls)...")
                latencies = []
                errors = 0
                start_test = time.time()

                for i in range(num_calls):
                    tool_config = tools_config[i % len(tools_config)]
                    tool_name = tool_config["name"]
                    args = tool_config["args"]
                    if callable(args):
                        args = args(i)

                    call_start = time.time()
                    try:
                        result = await session.call_tool(tool_name, args)
                        elapsed_ms = (time.time() - call_start) * 1000

                        if result.isError:
                            errors += 1
                        else:
                            latencies.append(elapsed_ms)
                    except Exception as e:
                        errors += 1
                        if errors <= 5:
                            print(f"  Call {i} error: {type(e).__name__}: {e}")
                            # Try to see what tool was called
                            print(f"    Tool: {tool_name}, Args: {args}")

                    # Progress
                    if (i + 1) % 50 == 0:
                        elapsed_total = time.time() - start_test
                        rate = (i + 1) / elapsed_total if elapsed_total > 0 else 0
                        print(f"  Progress: {i + 1}/{num_calls} calls ({rate:.1f} req/s)")

                total_time = time.time() - start_test
                throughput = len(latencies) / total_time if total_time > 0 else 0

                print(f"\n--- Results ---")
                print(f"  Total time: {total_time:.2f} s")
                print(f"  Successful calls: {len(latencies)}/{num_calls}")
                print(f"  Errors: {errors}")
                print(f"  Success rate: {(len(latencies)/num_calls)*100:.1f}%")

                if latencies:
                    latencies.sort()
                    mean_lat = statistics.mean(latencies)
                    median_lat = statistics.median(latencies)
                    p95_lat = latencies[int(len(latencies) * 0.95)]
                    p99_lat = latencies[int(len(latencies) * 0.99)]
                    stddev_lat = statistics.stdev(latencies) if len(latencies) > 1 else 0

                    print(f"\n  Latency Statistics:")
                    print(f"    Mean:   {mean_lat:.2f} ms")
                    print(f"    Median: {median_lat:.2f} ms")
                    print(f"    P95:    {p95_lat:.2f} ms")
                    print(f"    P99:    {p99_lat:.2f} ms")
                    print(f"    Min:    {min(latencies):.2f} ms")
                    print(f"    Max:    {max(latencies):.2f} ms")
                    print(f"    StdDev: {stddev_lat:.2f} ms")
                    print(f"\n  Throughput: {throughput:.1f} req/s")

                    results["latency"] = {
                        "mean_ms": mean_lat,
                        "median_ms": median_lat,
                        "p95_ms": p95_lat,
                        "p99_ms": p99_lat,
                        "min_ms": min(latencies),
                        "max_ms": max(latencies),
                        "stddev_ms": stddev_lat,
                    }
                    results["throughput_rps"] = throughput
                    results["success_rate"] = (len(latencies) / num_calls) * 100
                    results["errors"] = errors
                    results["total_time_s"] = total_time

                return results

    except Exception as e:
        print(f"ERROR: {e}")
        import traceback
        traceback.print_exc()
        results["error"] = str(e)
        return results


async def main():
    print("="*80)
    print("PROFESSIONAL MCP BENCHMARK (using official MCP Python SDK)")
    print("="*80)
    print(f"Date: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print(f"Test Directory: {TEST_DIR}")
    print(f"MCP SDK Version: 1.29.1")
    print()

    # Setup test environment
    setup_test_environment(TEST_DIR)

    # ========================================================================
    # 1. OFFICIAL FILESYSTEM SERVER
    # ========================================================================
    fs_server_path = "C:/Users/ian/AppData/Roaming/npm/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"

    from mcp.client.stdio import StdioServerParameters
    official_params = StdioServerParameters(
        command="node",
        args=[fs_server_path, TEST_DIR],
    )

    official_tools = [
        {"name": "read_file", "args": {"path": f"{TEST_DIR}\\file1.txt"}},
        {"name": "list_directory", "args": {"path": TEST_DIR}},
        {"name": "read_file", "args": lambda i: {"path": f"{TEST_DIR}\\file{'1' if i % 2 == 0 else '2'}.txt"}},
        {"name": "search_files", "args": {"path": TEST_DIR, "pattern": "*.txt"}},
    ]

    official_results = await benchmark_server(
        "Official Filesystem Server (Node.js)",
        official_params,
        official_tools,
        num_calls=200
    )

    # Give system a moment to clean up
    await asyncio.sleep(1)

    # ========================================================================
    # 2. VELOCITY-MCP
    # ========================================================================
    velocity_mcp_path = "C:/Users/ian/Documents/MCP/target/release/velocity_mcp.exe"

    velocity_params = StdioServerParameters(
        command=velocity_mcp_path,
        args=[],
        env={"RUST_LOG": "error"},  # Suppress all logs except errors
    )

    velocity_tools = [
        {"name": "file_read", "args": {"path": f"{TEST_DIR}\\file1.txt"}},
        {"name": "list_directory", "args": {"path": TEST_DIR}},
        {"name": "file_read", "args": lambda i: {"path": f"{TEST_DIR}\\file{'1' if i % 2 == 0 else '2'}.txt"}},
        {"name": "search_files", "args": {"path": TEST_DIR, "pattern": "*.txt"}},
    ]

    velocity_results = await benchmark_server(
        "VELOCITY-MCP v3.0.0 (Rust)",
        velocity_params,
        velocity_tools,
        num_calls=200
    )

    # ========================================================================
    # 3. COMPARISON
    # ========================================================================
    print("\n" + "="*80)
    print("COMPARISON SUMMARY")
    print("="*80)

    if "latency" in official_results and "latency" in velocity_results:
        off = official_results["latency"]
        vel = velocity_results["latency"]

        print("\n[COMPARISON] Sustained Load Performance (200 calls per server):")
        print(f"  {'Metric':<20} {'Official':>12} {'VELOCITY':>12} {'Speedup':>10}")
        print(f"  {'-'*60}")
        print(f"  {'Mean Latency':<20} {off['mean_ms']:>10.2f} ms {vel['mean_ms']:>10.2f} ms {off['mean_ms']/vel['mean_ms']:>8.2f}x")
        print(f"  {'Median Latency':<20} {off['median_ms']:>10.2f} ms {vel['median_ms']:>10.2f} ms {off['median_ms']/vel['median_ms']:>8.2f}x")
        print(f"  {'P95 Latency':<20} {off['p95_ms']:>10.2f} ms {vel['p95_ms']:>10.2f} ms {off['p95_ms']/vel['p95_ms']:>8.2f}x")
        print(f"  {'P99 Latency':<20} {off['p99_ms']:>10.2f} ms {vel['p99_ms']:>10.2f} ms {off['p99_ms']/vel['p99_ms']:>8.2f}x")
        print(f"  {'Std Deviation':<20} {off['stddev_ms']:>10.2f} ms {vel['stddev_ms']:>10.2f} ms {'':>8}")
        print(f"  {'Throughput':<20} {official_results['throughput_rps']:>10.1f} rps {velocity_results['throughput_rps']:>10.1f} rps {velocity_results['throughput_rps']/official_results['throughput_rps']:>8.2f}x")
        print(f"  {'Success Rate':<20} {official_results['success_rate']:>10.1f}% {velocity_results['success_rate']:>10.1f}% {'':>8}")
        print(f"  {'Init Time':<20} {official_results['init_time_ms']:>10.1f} ms {velocity_results['init_time_ms']:>10.1f} ms {official_results['init_time_ms']/velocity_results['init_time_ms']:>8.2f}x")

        print(f"\n[VERDICT]:")
        speedup = off['mean_ms'] / vel['mean_ms']
        if speedup >= 2:
            print(f"  VELOCITY-MCP is {speedup:.1f}x faster - SIGNIFICANT IMPROVEMENT")
        elif speedup >= 1.2:
            print(f"  VELOCITY-MCP is {speedup:.1f}x faster - MODERATE IMPROVEMENT")
        else:
            print(f"  VELOCITY-MCP is {speedup:.1f}x faster - MARGINAL IMPROVEMENT")

    # Save results
    import json
    output_file = Path("C:/Users/ian/Documents/MCP/benchmark_professional_results.json")
    all_results = {
        "date": datetime.now().isoformat(),
        "sdk_version": "1.29.1",
        "test_dir": TEST_DIR,
        "official": official_results,
        "velocity": velocity_results,
    }
    with open(output_file, "w") as f:
        json.dump(all_results, f, indent=2, default=str)
    print(f"\n[SAVED] Results saved to: {output_file}")

    print("\n" + "="*80)
    print("BENCHMARK COMPLETE")
    print("="*80)


if __name__ == "__main__":
    asyncio.run(main())
