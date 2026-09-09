#!/usr/bin/env python3
"""
Interop test: Official @modelcontextprotocol/server-filesystem vs our NMCP server.
Uses newline-delimited JSON for the official server (per MCP SDK stdio transport).
"""
import subprocess
import json
import sys
import os
import hashlib
import time
import threading
import urllib.request

NODE = r"C:\Program Files\nodejs\node.exe"
MCP_FS_ENTRY = r"C:\Users\ian\AppData\Roaming\npm\node_modules\@modelcontextprotocol\server-filesystem\dist\index.js"
TEST_DIR = r"C:\Users\ian\Documents\MCP\test_nda_dir"
OUR_SERVER = "http://127.0.0.1:8080/v1"


class McpStdioClient:
    """MCP client using newline-delimited JSON (official SDK protocol)."""
    def __init__(self, node_path, script_path, args):
        self.proc = subprocess.Popen(
            [node_path, script_path] + args,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,
        )
        self._id = 0
        self._responses = {}
        self._lock = threading.Lock()
        self._event = threading.Event()
        self._stderr_lines = []

        # Reader thread for stdout
        self._reader = threading.Thread(target=self._read_loop, daemon=True)
        self._reader.start()

        # Reader thread for stderr
        self._err_reader = threading.Thread(target=self._read_stderr, daemon=True)
        self._err_reader.start()

    def _read_loop(self):
        buf = b""
        while True:
            try:
                chunk = self.proc.stdout.read(1)
                if not chunk:
                    break
                buf += chunk
                if buf.endswith(b"\n"):
                    line = buf.decode("utf-8").strip()
                    buf = b""
                    if line:
                        msg = json.loads(line)
                        msg_id = msg.get("id")
                        if msg_id is not None:
                            with self._lock:
                                self._responses[msg_id] = msg
                            self._event.set()
            except Exception:
                break

    def _read_stderr(self):
        while True:
            line = self.proc.stderr.readline()
            if not line:
                break
            self._stderr_lines.append(line.decode().strip())

    def request(self, method, params=None, timeout=10):
        self._id += 1
        msg_id = self._id
        msg = {"jsonrpc": "2.0", "id": msg_id, "method": method}
        if params is not None:
            msg["params"] = params
        line = json.dumps(msg) + "\n"
        self.proc.stdin.write(line.encode("utf-8"))
        self.proc.stdin.flush()

        # Wait for response with this ID
        deadline = time.time() + timeout
        while time.time() < deadline:
            self._event.wait(timeout=0.5)
            self._event.clear()
            with self._lock:
                if msg_id in self._responses:
                    return self._responses.pop(msg_id)
        return {"error": {"message": f"Timeout waiting for response to {method}"}}

    def notify(self, method, params=None):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        line = json.dumps(msg) + "\n"
        self.proc.stdin.write(line.encode("utf-8"))
        self.proc.stdin.flush()

    def initialize(self):
        resp = self.request("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "interop-test", "version": "1.0.0"}
        })
        self.notify("notifications/initialized")
        return resp

    def list_tools(self):
        return self.request("tools/list")

    def call_tool(self, name, arguments):
        return self.request("tools/call", {"name": name, "arguments": arguments})

    def close(self):
        try:
            self.proc.stdin.close()
        except:
            pass
        self.proc.terminate()
        try:
            self.proc.wait(timeout=5)
        except:
            self.proc.kill()


def our_json_rpc(method, params=None):
    msg = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        msg["params"] = params
    body = json.dumps(msg).encode("utf-8")
    req = urllib.request.Request(
        f"{OUR_SERVER}/mcp",
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read())


def our_call_tool(name, arguments):
    return our_json_rpc("tools/call", {"name": name, "arguments": arguments})


def our_list_tools():
    return our_json_rpc("tools/list")


def sha256(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]


def main():
    print("=" * 70)
    print("INTEROP TEST: Official MCP Filesystem Server vs NMCP Server")
    print("=" * 70)

    # Start official MCP filesystem server
    print("\n[1] Starting official @modelcontextprotocol/server-filesystem...")
    client = McpStdioClient(NODE, MCP_FS_ENTRY, [TEST_DIR])
    time.sleep(1)  # Let it start

    init_resp = client.initialize()
    server_info = init_resp.get("result", {}).get("serverInfo", {})
    print(f"    Initialized: {server_info}")
    if "error" in init_resp:
        print(f"    ERROR: {init_resp['error']}")
        if client._stderr_lines:
            print(f"    STDERR: {client._stderr_lines}")
        client.close()
        return 1

    # List tools from official server
    tools_resp = client.list_tools()
    official_tools = {t["name"]: t for t in tools_resp.get("result", {}).get("tools", [])}
    print(f"    Official tools ({len(official_tools)}): {sorted(official_tools.keys())}")

    # List tools from our server
    our_tools_resp = our_list_tools()
    our_tools = {t["name"]: t for t in our_tools_resp.get("result", {}).get("tools", [])}
    our_fs_tools = {k: v for k, v in our_tools.items() if any(
        x in k for x in ["file_", "list_directory", "search_files", "edit_file",
                          "create_directory", "move_file"]
    )}
    print(f"\n[2] Our filesystem tools ({len(our_fs_tools)}): {sorted(our_fs_tools.keys())}")

    common = set(official_tools.keys()) & set(our_tools.keys())
    print(f"    Common tools: {sorted(common)}")

    passed = 0
    failed = 0
    tests = []

    def record(name, ok, detail=""):
        nonlocal passed, failed
        status = "PASS" if ok else "FAIL"
        if ok:
            passed += 1
        else:
            failed += 1
        tests.append((name, status, detail))
        print(f"    [{status}] {name}" + (f" -- {detail}" if detail else ""))

    # ─── Test: list_directory ───
    print("\n[3] Testing list_directory...")
    off_list = client.call_tool("list_directory", {"path": TEST_DIR})
    our_list = our_call_tool("list_directory", {"path": TEST_DIR})

    off_content = off_list.get("result", {}).get("content", [])
    our_content = our_list.get("result", {}).get("content", [])

    # Extract file names from both (handle different formats)
    off_text = off_content[0].get("text", "") if off_content else ""
    our_text = our_content[0].get("text", "") if our_content else ""

    # Official: "[FILE] filename" format
    off_files = sorted([l.split("] ")[1] for l in off_text.split("\n") if "[FILE]" in l or "[DIR]" in l])
    # Ours: JSON array with "name" fields
    try:
        our_json = json.loads(our_text)
        our_files = sorted([e["name"] for e in our_json if "name" in e])
    except:
        our_files = sorted([f for f in our_text.split("\n") if f.strip()])
    record("list_directory contents match",
           set(off_files) == set(our_files),
           f"official={off_files}, ours={our_files}")

    # ─── Test: read_file ───
    print("\n[4] Testing read_file...")
    hello_path = os.path.join(TEST_DIR, "hello.txt").replace("\\", "/")
    off_read = client.call_tool("read_file", {"path": hello_path})
    our_read = our_call_tool("file_read", {"path": hello_path})

    off_read_text = off_read.get("result", {}).get("content", [{}])[0].get("text", "")
    our_read_text = our_read.get("result", {}).get("content", [{}])[0].get("text", "")
    record("read_file content match",
           off_read_text.strip() == our_read_text.strip(),
           f"sha(off)={sha256(off_read_text)}, sha(ours)={sha256(our_read_text)}")

    # ─── Test: read_file (JSON) ───
    print("\n[5] Testing read_file (JSON)...")
    json_path = os.path.join(TEST_DIR, "data.json").replace("\\", "/")
    off_json = client.call_tool("read_file", {"path": json_path})
    our_json = our_call_tool("file_read", {"path": json_path})

    off_json_text = off_json.get("result", {}).get("content", [{}])[0].get("text", "")
    our_json_text = our_json.get("result", {}).get("content", [{}])[0].get("text", "")
    record("read_file JSON match",
           off_json_text.strip() == our_json_text.strip(),
           f"sha(off)={sha256(off_json_text)}, sha(ours)={sha256(our_json_text)}")

    # ─── Test: get_file_info ───
    print("\n[6] Testing get_file_info...")
    off_info = client.call_tool("get_file_info", {"path": hello_path})
    our_info = our_call_tool("get_file_info", {"path": hello_path})

    off_info_text = off_info.get("result", {}).get("content", [{}])[0].get("text", "")
    our_info_text = our_info.get("result", {}).get("content", [{}])[0].get("text", "")

    # Parse official server's plain text format: "size: 29\ncreated: ...\nisDirectory: false"
    import re
    off_info_json = {}
    if off_info_text.strip():
        try:
            off_info_json = json.loads(off_info_text)
        except json.JSONDecodeError:
            # Plain text format: "key: value" per line
            for line in off_info_text.split("\n"):
                m = re.match(r'^(\w+):\s+(.+)$', line.strip())
                if m:
                    key, val = m.group(1), m.group(2)
                    if key == "size":
                        off_info_json["size"] = int(val)
                    elif key == "isDirectory":
                        off_info_json["isDirectory"] = val.lower() == "true"
                    elif key == "isFile":
                        off_info_json["isFile"] = val.lower() == "true"

    our_info_json = {}
    if our_info_text.strip():
        try:
            our_info_json = json.loads(our_info_text)
        except json.JSONDecodeError:
            for line in our_info_text.split("\n"):
                m = re.match(r'^(\w+):\s+(.+)$', line.strip())
                if m:
                    key, val = m.group(1), m.group(2)
                    if key == "size":
                        our_info_json["size"] = int(val)
                    elif key == "isDirectory":
                        our_info_json["isDirectory"] = val.lower() == "true"

    size_match = off_info_json.get("size") == our_info_json.get("size")
    record("get_file_info size match",
           size_match,
           f"off={off_info_json.get('size')}, ours={our_info_json.get('size')}")

    off_dir = off_info_json.get("isDirectory")
    our_dir = our_info_json.get("isDirectory")
    dir_match = off_dir == our_dir
    record("get_file_info isDirectory match",
           dir_match,
           f"off={off_dir}, ours={our_dir}")

    # ─── Test: error handling (file not found) ───
    print("\n[7] Testing error handling (file not found)...")
    fake_path = os.path.join(TEST_DIR, "nonexistent.txt").replace("\\", "/")
    off_err = client.call_tool("read_file", {"path": fake_path})
    our_err = our_call_tool("file_read", {"path": fake_path})

    off_is_err = off_err.get("result", {}).get("isError", False)
    our_is_err = our_err.get("result", {}).get("isError", False)
    record("file_not_found both return isError",
           off_is_err == our_is_err == True,
           f"official={off_is_err}, ours={our_is_err}")

    # ─── Test: write_file + read back ───
    print("\n[8] Testing write_file + read back...")
    write_path = os.path.join(TEST_DIR, "interop_test.txt").replace("\\", "/")
    test_content = f"Interop test at {time.time()}"

    off_write = client.call_tool("write_file", {"path": write_path, "content": test_content})
    our_write = our_call_tool("file_write", {"path": write_path, "content": test_content})

    off_write_err = off_write.get("result", {}).get("isError", False)
    our_write_err = our_write.get("result", {}).get("isError", False)
    record("write_file both succeed",
           not off_write_err and not our_write_err,
           f"off_err={off_write_err}, our_err={our_write_err}")

    off_readback = client.call_tool("read_file", {"path": write_path})
    our_readback = our_call_tool("file_read", {"path": write_path})

    off_rb_text = off_readback.get("result", {}).get("content", [{}])[0].get("text", "")
    our_rb_text = our_readback.get("result", {}).get("content", [{}])[0].get("text", "")
    record("write+read roundtrip match",
           off_rb_text.strip() == our_rb_text.strip() == test_content,
           f"content verified")

    # ─── Test: search_files ───
    print("\n[9] Testing search_files...")
    off_search = client.call_tool("search_files", {"path": TEST_DIR, "pattern": "hello"})
    our_search = our_call_tool("search_files", {"path": TEST_DIR, "pattern": "hello"})

    off_search_text = off_search.get("result", {}).get("content", [{}])[0].get("text", "")
    our_search_text = our_search.get("result", {}).get("content", [{}])[0].get("text", "")

    off_hits = [l for l in off_search_text.split("\n") if l.strip()]
    our_hits = [l for l in our_search_text.split("\n") if l.strip()]
    record("search_files both find matches",
           len(off_hits) > 0 and len(our_hits) > 0,
           f"off_hits={len(off_hits)}, our_hits={len(our_hits)}")

    # ─── Test: NDA conversion pipeline ───
    print("\n[10] Testing NDA conversion pipeline...")
    nda_convert = our_json_rpc("tools/call", {
        "name": "convert_to_nda_tool",
        "arguments": {
            "jsonRequest": json.dumps({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "file_read", "arguments": {"path": hello_path}}
            })
        }
    })
    record("convert file_read to NDA",
           not nda_convert.get("result", {}).get("isError", False))

    nda_result = our_call_tool("file_read", {"path": hello_path})
    nda_text = nda_result.get("result", {}).get("content", [{}])[0].get("text", "")
    record("NDA file_read matches direct",
           nda_text.strip() == our_read_text.strip(),
           f"sha(nda)={sha256(nda_text)}, sha(direct)={sha256(our_read_text)}")

    nda_list = our_json_rpc("tools/call", {
        "name": "convert_to_nda_tool",
        "arguments": {
            "jsonRequest": json.dumps({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "list_directory", "arguments": {"path": TEST_DIR}}
            })
        }
    })
    record("convert list_directory to NDA",
           not nda_list.get("result", {}).get("isError", False))

    nda_info = our_json_rpc("tools/call", {
        "name": "convert_to_nda_tool",
        "arguments": {
            "jsonRequest": json.dumps({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "get_file_info", "arguments": {"path": hello_path}}
            })
        }
    })
    record("convert get_file_info to NDA",
           not nda_info.get("result", {}).get("isError", False))

    # ─── Test: large file read comparison ───
    print("\n[11] Testing large file (binary_data.txt ~68KB)...")
    bin_path = os.path.join(TEST_DIR, "binary_data.txt").replace("\\", "/")
    off_bin = client.call_tool("read_file", {"path": bin_path})
    our_bin = our_call_tool("file_read", {"path": bin_path})

    off_bin_text = off_bin.get("result", {}).get("content", [{}])[0].get("text", "")
    our_bin_text = our_bin.get("result", {}).get("content", [{}])[0].get("text", "")
    record("large file read match",
           off_bin_text == our_bin_text,
           f"sha(off)={sha256(off_bin_text)}, sha(ours)={sha256(our_bin_text)}, "
           f"len(off)={len(off_bin_text)}, len(ours)={len(our_bin_text)}")

    # ─── Cleanup ───
    client.close()

    # ─── Summary ───
    print("\n" + "=" * 70)
    print(f"RESULTS: {passed} passed, {failed} failed, {passed+failed} total")
    print("=" * 70)
    if failed:
        print("\nFailed tests:")
        for name, status, detail in tests:
            if status == "FAIL":
                print(f"  X {name}: {detail}")
    print()
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
