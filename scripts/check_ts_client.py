"""Offline gate for the TypeScript client's notification arms.

The TypeScript SDK ships no test files (its package.json runs `jest`, which has nothing
to collect and no installed dependencies), so the arms documented in
docs/CLIENT_INTEGRATION.md under "Notification Handling" are otherwise unenforced.
This gate runs the shipped `sdk/typescript/src/client.ts` itself: it copies the sources
to a temp directory, rewrites only import specifiers -- Node's type stripper rejects
extensionless relative imports, and a plain `import` of a type-only module leaves a
runtime binding the module never exports -- and executes the probe under
`node --experimental-strip-types`. No SDK logic is edited or reimplemented.

It is sensitive by construction: deleting the `if (!response)` guard from the copy turns
check 1 into `TypeError: Cannot read properties of undefined (reading 'error')` and the
gate exits 1. Like scripts/check_citations.py and scripts/check_doc_examples.py it is run
by nothing in CI yet; wiring it is the operator's decision.

Usage: python scripts/check_ts_client.py
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SDK_SRC = os.path.join(REPO, "sdk", "typescript", "src")

# client.ts only needs the Transport *type*; the shipped transport.ts imports axios,
# which a dependency-free clone does not have, so the interface is restated here.
TRANPORT_STUB = (
    "export interface Transport {\n"
    "  send(request: import(\"./types.ts\").JsonRpcRequest):"
    " Promise<import(\"./types.ts\").JsonRpcResponse | undefined>;\n"
    "  close(): Promise<void>;\n"
    "}\n"
)

PROBE = '''import type { JsonRpcRequest, JsonRpcResponse } from "./types.ts";
import { McpClient } from "./client.ts";
import { TransportError } from "./errors.ts";

// Answers like the server's HTTP leg: an envelope for a body carrying an id, nothing
// at all for a notification.
class StubTransport {
  methods: string[] = [];
  private readonly envelopeForRequests: boolean;
  constructor(envelopeForRequests: boolean) {
    this.envelopeForRequests = envelopeForRequests;
  }

  async send(request: JsonRpcRequest): Promise<JsonRpcResponse | undefined> {
    this.methods.push(request.method);
    if (request.id === undefined || !this.envelopeForRequests) {
      return undefined;
    }
    return {
      jsonrpc: "2.0",
      id: request.id,
      result: { protocolVersion: "2024-11-05", tools: [{ name: "echo" }] },
    };
  }

  async close(): Promise<void> {}
}

let failures = 0;
function check(name: string, ok: boolean, detail: string): void {
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " -- " + detail : ""}`);
  if (!ok) failures++;
}

// The mirror case: a request answered with no envelope is a failure, not an ack.
{
  const client = new McpClient(new StubTransport(false));
  let error: unknown = null;
  try {
    await client.listTools();
  } catch (thrown) {
    error = thrown;
  }
  check(
    "request answered with no envelope raises",
    error instanceof TransportError,
    error === null ? "no error raised" : `${(error as Error).name}: ${(error as Error).message}`,
  );
  check(
    "the error names the request id",
    String((error as Error | null)?.message ?? "").includes("No response envelope for request 1"),
    String((error as Error | null)?.message ?? ""),
  );
}

// The notification leg of the handshake must neither block nor raise.
{
  const transport = new StubTransport(true);
  const client = new McpClient(transport);
  let result: unknown = null;
  let error: unknown = null;
  try {
    result = await client.initialize();
  } catch (thrown) {
    error = thrown;
  }
  check(
    "initialize completes over a silent notification",
    error === null && (result as any).protocolVersion === "2024-11-05",
    error === null ? JSON.stringify(result) : `${(error as Error).name}: ${(error as Error).message}`,
  );
  check(
    "handshake sends initialize then notifications/initialized",
    transport.methods.join(",") === "initialize,notifications/initialized",
    transport.methods.join(","),
  );
  check("client reports itself initialized", client.isInitialized === true, "");
}

console.log(failures === 0 ? "ALL CHECKS PASSED" : `${failures} CHECK(S) FAILED`);
// Set the code and let Node drain: process.exit() on Windows can abort in libuv
// (`Assertion failed: !(handle->flags & UV_HANDLE_CLOSING)`) after a green run.
process.exitCode = failures === 0 ? 0 : 1;
'''


def prepare(dst: str) -> None:
    for name in ("client.ts", "errors.ts", "types.ts"):
        text = open(os.path.join(SDK_SRC, name), encoding="utf-8").read()
        text = text.replace('from "./types"', 'from "./types.ts"')
        text = text.replace('from "./errors"', 'from "./errors.ts"')
        text = text.replace('from "./transport"', 'from "./transport.ts"')
        # types.ts exports nothing at runtime, so its import must be erased.
        text = re.sub(r'import \{([^}]*)\} from "\./types\.ts";', r'import type {\1} from "./types.ts";', text)
        text = re.sub(r'import \{ Transport \} from "\./transport\.ts";',
                      'import type { Transport } from "./transport.ts";', text)
        open(os.path.join(dst, name), "w", encoding="utf-8", newline="\n").write(text)
    open(os.path.join(dst, "transport.ts"), "w", encoding="utf-8", newline="\n").write(TRANPORT_STUB)
    open(os.path.join(dst, "notification_probe.mts"), "w", encoding="utf-8", newline="\n").write(PROBE)


def main() -> int:
    if not shutil.which("node"):
        print("node not found on PATH", file=sys.stderr)
        return 2
    dst = tempfile.mkdtemp(prefix="ts_client_check_")
    try:
        prepare(dst)
        run = subprocess.run(
            ["node", "--experimental-strip-types", os.path.join(dst, "notification_probe.mts")],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        sys.stdout.write(run.stdout)
        sys.stderr.write(run.stderr)
        return run.returncode
    finally:
        shutil.rmtree(dst, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
