#!/usr/bin/env python3
"""Simple MCP benchmark - measures startup, memory, and tool latency separately."""

import subprocess
import time
import json
import sys
import os
from pathlib import Path

TEST_DIR = "C:/tmp/benchmark_test"
FS_SERVER_PATH = "C:/Users/ian/AppData/Roaming/npm/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"
VELOCITY_MCP_PATH = "C:/Users/ian/Documents/MCP/target/release/velocity_mcp.exe"


def run_command(cmd, input_data=None, timeout=10):
    """Run a command and return (stdout, stderr, elapsed_time)."""
    start = time.time()
    try:
        proc = subprocess.run(
            cmd,
            input=input_data,
            capture_output=True,
            text=True,
            timeout=timeout,
            shell=True if isinstance(cmd, str) else False,
        )
        elapsed = time.time() - start
        return proc.stdout, proc.stderr, elapsed
    except subprocess.TimeoutExpired:
        elapsed = time.time() - start
        return "", f"Timeout after {elapsed:.2f}s", elapsed


def measure_startup(server_cmd, test_input):
    """Measure server startup time by sending initialize request."""
    start = time.time()
    try:
        proc = subprocess.Popen(
            server_cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )

        # Send initialize request
        proc.stdin.write(test_input + "\n")
        proc.stdin.flush()

        # Read response with timeout
        response = ""
        start_read = time.time()
        while time.time() - start_read < 5:
            line = proc.stdout.readline()
            if line:
                response = line.strip()
                break
            time.sleep(0.01)

        elapsed = time.time() - start
        proc.kill()
        proc.wait()

        return elapsed * 1000, response  # Return ms
    except Exception as e:
        return None, str(e)


def measure_memory(server_cmd, duration=2):
    """Measure peak memory usage of a server process."""
    try:
        proc = subprocess.Popen(
            server_cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )

        # Let it run for a bit
        time.sleep(duration)

        # Get memory on Windows using tasklist
        pid = proc.pid
        result = subprocess.run(
            ["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"],
            capture_output=True,
            text=True,
        )

        proc.kill()
        proc.wait()

        if result.returncode == 0 and result.stdout:
            # Parse CSV output: "Image Name","PID","Session Name","Session#","Mem Usage"
            parts = result.stdout.strip().split(",")
            if len(parts) >= 5:
                mem_str = parts[4].strip('"').replace(",", "").replace("K", "")
                try:
                    mem_kb = int(mem_str)
                    return mem_kb / 1024  # Convert to MB
                except ValueError:
                    pass

        return None
    except Exception as e:
        print(f"Memory measurement error: {e}")
        return None


def benchmark_tool_calls(server_name, server_cmd, tool_requests, iterations=20):
    """Benchmark tool call latency."""
    latencies = []
    failures = 0
    failure_reasons = []

    for i in range(iterations):
        try:
            proc = subprocess.Popen(
                server_cmd,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,  # Capture stderr separately
                text=True,
                bufsize=0,  # Unbuffered for immediate output
            )

            # Initialize first
            init_req = json.dumps({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "benchmark", "version": "1.0"}
                }
            })
            proc.stdin.write(init_req + "\n")
            proc.stdin.flush()

            # Read init response - read character by character to avoid blocking on readline
            start_read = time.time()
            init_ok = False
            buffer = ""
            while time.time() - start_read < 5:
                try:
                    char = proc.stdout.read(1)
                    if not char:
                        # EOF reached - check if process is still running
                        if proc.poll() is not None:
                            # Process exited, check stderr
                            stderr_out = proc.stderr.read()
                            failures += 1
                            failure_reasons.append(f"Process exited early, stderr: {stderr_out[:300]}")
                            proc.wait()
                            break
                        continue
                    buffer += char
                    if char == '\n':
                        # We have a complete line
                        line = buffer.strip()
                        buffer = ""
                        if line:
                            try:
                                resp = json.loads(line)
                                if resp.get("id") == 1:
                                    init_ok = True
                                    break
                            except json.JSONDecodeError:
                                # Log message, skip it
                                continue
                except Exception as e:
                    failures += 1
                    failure_reasons.append(f"Read error: {e}")
                    break
            
            if not init_ok and not any("Process exited" in r for r in failure_reasons[-1:] if failure_reasons):
                failures += 1
                failure_reasons.append(f"Init timeout after {time.time() - start_read:.2f}s, got {len(buffer)} chars: {buffer[:100]}")
                proc.kill()
                proc.wait()
                continue

            # Send initialized notification
            init_notif = json.dumps({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            })
            proc.stdin.write(init_notif + "\n")
            proc.stdin.flush()
            time.sleep(0.1)  # Give server time to process

            # Now send tool call
            req_id = 100 + i
            tool_req_dict = tool_requests[i % len(tool_requests)].copy()
            tool_req_dict["id"] = req_id
            tool_req = json.dumps(tool_req_dict)

            start = time.time()
            proc.stdin.write(tool_req + "\n")
            proc.stdin.flush()

            # Read response - need to handle both stdout and stderr
            response = None
            start_read = time.time()
            while time.time() - start_read < 5:
                # Try to read from stdout
                if proc.stdout:
                    line = proc.stdout.readline()
                    if line and line.strip():
                        try:
                            resp = json.loads(line)
                            if resp.get("id") == req_id:
                                response = resp
                                break
                        except:
                            # Not JSON, might be log message
                            continue
                time.sleep(0.01)

            elapsed = (time.time() - start) * 1000  # ms
            if response:
                latencies.append(elapsed)
            else:
                failures += 1
                # Check if there was an error response
                stderr_output = ""
                try:
                    # Non-blocking read of remaining output
                    import select
                    if select.select([proc.stdout], [], [], 0.1)[0]:
                        line = proc.stdout.readline()
                        if line:
                            stderr_output = line.strip()
                except:
                    pass
                failure_reasons.append(f"No response, last stderr snippet: {stderr_output[:200]}")

            proc.kill()
            proc.wait()

        except Exception as e:
            failures += 1
            failure_reasons.append(str(e))
            continue

    if latencies:
        latencies.sort()
        p50 = latencies[len(latencies) // 2]
        p95 = latencies[int(len(latencies) * 0.95)]
        mean = sum(latencies) / len(latencies)
        return {
            "p50_ms": p50,
            "p95_ms": p95,
            "mean_ms": mean,
            "min_ms": min(latencies),
            "max_ms": max(latencies),
            "count": len(latencies),
            "failures": failures,
        }
    else:
        # Print some failure reasons for debugging
        if failure_reasons:
            print(f"  First 3 failure reasons:")
            for reason in failure_reasons[:3]:
                print(f"    - {reason}")
    return None


def main():
    print("="*80)
    print("MCP SERVER BENCHMARK - SIMPLE")
    print("="*80)

    # Ensure test directory exists
    Path(TEST_DIR).mkdir(parents=True, exist_ok=True)
    (Path(TEST_DIR) / "file1.txt").write_text("Hello World - benchmark test file content here")
    (Path(TEST_DIR) / "subdir1").mkdir(exist_ok=True)

    results = {}

    # 1. Official Filesystem Server
    print("\n" + "="*80)
    print("1. OFFICIAL FILESYSTEM SERVER (Node.js)")
    print("="*80)

    fs_server_cmd = ["node", FS_SERVER_PATH, TEST_DIR]  # Use Windows path directly

    # Startup time
    print("\nMeasuring startup...")
    init_req = json.dumps({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "benchmark", "version": "1.0"}
        }
    })
    startup_ms, response = measure_startup(fs_server_cmd, init_req)
    if startup_ms:
        print(f"  Startup time: {startup_ms:.1f} ms")
        print(f"  Response: {response[:100]}")
    else:
        print(f"  Startup measurement failed: {response}")

    # Memory usage
    print("\nMeasuring memory...")
    memory_mb = measure_memory(fs_server_cmd)
    if memory_mb:
        print(f"  Memory usage: {memory_mb:.1f} MB")
    else:
        print("  Memory measurement failed")

    # Tool calls
    print("\nBenchmarking tool calls (20 iterations)...")
    tool_requests = [
        {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "read_file",
                "arguments": {"path": f"{TEST_DIR}\\file1.txt"}  # Use Windows path
            }
        },
        {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "list_directory",
                "arguments": {"path": TEST_DIR}  # Use Windows path
            }
        },
    ]
    fs_latency = benchmark_tool_calls("filesystem-official", fs_server_cmd, tool_requests, iterations=20)
    if fs_latency:
        print(f"  p50: {fs_latency['p50_ms']:.1f} ms")
        print(f"  p95: {fs_latency['p95_ms']:.1f} ms")
        print(f"  mean: {fs_latency['mean_ms']:.1f} ms")
        print(f"  min: {fs_latency['min_ms']:.1f} ms")
        print(f"  max: {fs_latency['max_ms']:.1f} ms")
        print(f"  successful calls: {fs_latency['count']}")
    else:
        print("  Tool call benchmark failed")

    results["filesystem-official"] = {
        "startup_ms": startup_ms,
        "memory_mb": memory_mb,
        "latency": fs_latency,
    }

    # 2. VELOCITY-MCP
    print("\n" + "="*80)
    print("2. VELOCITY-MCP (Rust)")
    print("="*80)

    velocity_cmd = [VELOCITY_MCP_PATH]

    # Startup time
    print("\nMeasuring startup...")
    startup_ms, response = measure_startup(velocity_cmd, init_req)
    if startup_ms:
        print(f"  Startup time: {startup_ms:.1f} ms")
        print(f"  Response: {response[:100]}")
    else:
        print(f"  Startup measurement failed: {response}")

    # Memory usage
    print("\nMeasuring memory...")
    memory_mb = measure_memory(velocity_cmd)
    if memory_mb:
        print(f"  Memory usage: {memory_mb:.1f} MB")
    else:
        print("  Memory measurement failed")

    # Tool calls
    print("\nBenchmarking tool calls (20 iterations)...")
    velocity_tool_requests = [
        {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "file_read",
                "arguments": {"path": f"{TEST_DIR}/file1.txt"}
            }
        },
        {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "list_directory",
                "arguments": {"path": TEST_DIR}
            }
        },
    ]
    velocity_latency = benchmark_tool_calls("velocity-mcp", velocity_cmd, velocity_tool_requests, iterations=20)
    if velocity_latency:
        print(f"  p50: {velocity_latency['p50_ms']:.1f} ms")
        print(f"  p95: {velocity_latency['p95_ms']:.1f} ms")
        print(f"  mean: {velocity_latency['mean_ms']:.1f} ms")
        print(f"  min: {velocity_latency['min_ms']:.1f} ms")
        print(f"  max: {velocity_latency['max_ms']:.1f} ms")
        print(f"  successful calls: {velocity_latency['count']}")
    else:
        print("  Tool call benchmark failed")

    results["velocity-mcp"] = {
        "startup_ms": startup_ms,
        "memory_mb": memory_mb,
        "latency": velocity_latency,
    }

    # 3. Comparison
    print("\n" + "="*80)
    print("COMPARISON SUMMARY")
    print("="*80)

    if results["filesystem-official"]["latency"] and results["velocity-mcp"]["latency"]:
        fs_mean = results["filesystem-official"]["latency"]["mean_ms"]
        vel_mean = results["velocity-mcp"]["latency"]["mean_ms"]
        speedup = fs_mean / vel_mean if vel_mean > 0 else float('inf')

        print(f"\nTool Call Latency (mean):")
        print(f"  Official:  {fs_mean:.1f} ms")
        print(f"  VELOCITY:  {vel_mean:.1f} ms")
        print(f"  Speedup:   {speedup:.2f}x faster")

    if results["filesystem-official"]["startup_ms"] and results["velocity-mcp"]["startup_ms"]:
        fs_startup = results["filesystem-official"]["startup_ms"]
        vel_startup = results["velocity-mcp"]["startup_ms"]
        speedup = fs_startup / vel_startup if vel_startup > 0 else float('inf')

        print(f"\nStartup Time:")
        print(f"  Official:  {fs_startup:.1f} ms")
        print(f"  VELOCITY:  {vel_startup:.1f} ms")
        print(f"  Speedup:   {speedup:.2f}x faster")

    if results["filesystem-official"]["memory_mb"] and results["velocity-mcp"]["memory_mb"]:
        fs_mem = results["filesystem-official"]["memory_mb"]
        vel_mem = results["velocity-mcp"]["memory_mb"]
        reduction = (fs_mem - vel_mem) / fs_mem * 100 if fs_mem > 0 else 0

        print(f"\nMemory Usage:")
        print(f"  Official:  {fs_mem:.1f} MB")
        print(f"  VELOCITY:  {vel_mem:.1f} MB")
        print(f"  Reduction: {reduction:.1f}% smaller")

    # Save results
    output_file = Path("C:/Users/ian/Documents/MCP/benchmark_results_simple.json")
    with open(output_file, "w") as f:
        json.dump(results, f, indent=2, default=str)
    print(f"\nDetailed results saved to: {output_file}")


if __name__ == "__main__":
    main()
