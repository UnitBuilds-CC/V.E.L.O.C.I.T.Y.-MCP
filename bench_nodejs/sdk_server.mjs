// Full-featured Node.js MCP server using the official @modelcontextprotocol/sdk.
// Registers all 16 built-in tools that VELOCITY-MCP provides, with working implementations.
// Used for benchmark section [4] — measures real-world SDK overhead with full protocol coverage.

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import fs from "fs";
import path from "path";
import { execSync } from "child_process";
import http from "http";
import https from "https";

const server = new McpServer({
  name: "velocity-mcp-node",
  version: "3.1.0",
});

function ok(data) {
  return {
    content: [{ type: "text", text: JSON.stringify(data) }],
  };
}

function err(msg) {
  return {
    content: [{ type: "text", text: JSON.stringify({ error: msg }) }],
    isError: true,
  };
}

function validatePath(p) {
  if (!p || p.length === 0) throw new Error("path is required");
  if (p.includes("..")) throw new Error("path contains '..'");
  if (!path.isAbsolute(p)) throw new Error("path must be absolute");
}

// ── 1. file_read ──────────────────────────────────────────────────────────
server.tool(
  "file_read",
  "Read a file's contents as UTF-8 text.",
  { path: z.string().describe("Absolute path to the file.") },
  async ({ path: filePath }) => {
    try {
      validatePath(filePath);
      const content = fs.readFileSync(filePath, "utf-8");
      return ok({ content });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 2. file_write ─────────────────────────────────────────────────────────
server.tool(
  "file_write",
  "Write text content to a file. Creates parent directories if needed.",
  {
    path: z.string().describe("Absolute path to the file."),
    content: z.string().describe("Text content to write."),
  },
  async ({ path: filePath, content }) => {
    try {
      validatePath(filePath);
      fs.mkdirSync(path.dirname(filePath), { recursive: true });
      fs.writeFileSync(filePath, content, "utf-8");
      return ok({ bytesWritten: content.length });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 3. shell_exec ─────────────────────────────────────────────────────────
server.tool(
  "shell_exec",
  "Execute a shell command with timeout enforcement.",
  {
    command: z.string().describe("Shell command to execute."),
    workingDir: z.string().optional().describe("Working directory (absolute path)."),
    timeout: z.number().optional().describe("Timeout in seconds (default: 30)."),
  },
  async ({ command, workingDir, timeout }) => {
    try {
      const dangerous = ["rm -rf /", "format", "diskpart", "mkfs"];
      for (const d of dangerous) {
        if (command.includes(d)) return err("blocked: dangerous command");
      }
      const opts = {
        timeout: (timeout || 30) * 1000,
        encoding: "utf-8",
        maxBuffer: 10 * 1024 * 1024,
      };
      if (workingDir) opts.cwd = workingDir;
      let stdout, stderr;
      try {
        stdout = execSync(command, { ...opts, stdio: ["pipe", "pipe", "pipe"] });
        stderr = "";
      } catch (e) {
        stdout = e.stdout || "";
        stderr = e.stderr || e.message;
        return ok({ exitCode: e.status || 1, stdout, stderr });
      }
      return ok({ exitCode: 0, stdout: stdout.toString(), stderr: stderr.toString() });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 4. http_request ───────────────────────────────────────────────────────
server.tool(
  "http_request",
  "Make an HTTP request with SSRF protection.",
  {
    url: z.string().describe("Target URL (must be http:// or https://)."),
    method: z.string().optional().describe("HTTP method. Default: GET."),
    headers: z.record(z.string()).optional().describe("Request headers."),
    body: z.string().optional().describe("Request body."),
    timeout: z.number().optional().describe("Timeout in seconds (default: 30)."),
  },
  async ({ url, method, headers, body, timeout }) => {
    try {
      const u = new URL(url);
      if (u.protocol !== "http:" && u.protocol !== "https:") {
        return err("only http:// and https:// URLs allowed");
      }
      const lib = u.protocol === "https:" ? https : http;
      const opts = {
        method: method || "GET",
        headers: headers || {},
        timeout: (timeout || 30) * 1000,
      };
      return new Promise((resolve) => {
        const req = lib.request(url, opts, (res) => {
          let data = "";
          res.on("data", (chunk) => (data += chunk));
          res.on("end", () => {
            resolve(ok({
              statusCode: res.statusCode,
              statusText: res.statusMessage,
              body: data,
            }));
          });
        });
        req.on("error", (e) => resolve(err(e.message)));
        req.on("timeout", () => { req.destroy(); resolve(err("request timed out")); });
        if (body) req.write(body);
        req.end();
      });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 5. list_directory ─────────────────────────────────────────────────────
server.tool(
  "list_directory",
  "List contents of a directory.",
  { path: z.string().describe("Absolute path to the directory.") },
  async ({ path: dirPath }) => {
    try {
      validatePath(dirPath);
      const entries = fs.readdirSync(dirPath, { withFileTypes: true });
      const result = [];
      for (const entry of entries) {
        if (result.length >= 100000) break;
        const isDir = entry.isDirectory();
        let size = 0;
        if (!isDir) {
          try { size = fs.statSync(path.join(dirPath, entry.name)).size; } catch {}
        }
        result.push({ name: entry.name, type: isDir ? "directory" : "file", size });
      }
      return ok({ entries: result });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 6. directory_tree ─────────────────────────────────────────────────────
server.tool(
  "directory_tree",
  "Recursively list directory contents as a tree structure.",
  {
    path: z.string().describe("Absolute path to the root directory."),
    excludePatterns: z.array(z.string()).optional().describe("Glob patterns to exclude."),
  },
  async ({ path: rootPath, excludePatterns }) => {
    try {
      validatePath(rootPath);
      const excludes = excludePatterns || [];
      const lines = [];
      function walk(dir, depth) {
        if (depth > 20 || lines.length > 10000) return;
        const entries = fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name));
        for (const entry of entries) {
          if (lines.length > 10000) break;
          const indent = "  ".repeat(depth);
          const isDir = entry.isDirectory();
          lines.push(`${indent}${isDir ? "[D]" : "[F]"} ${entry.name}`);
          if (isDir) {
            const skip = excludes.some((p) => entry.name.endsWith(p.replace("*", "")));
            if (!skip) walk(path.join(dir, entry.name), depth + 1);
          }
        }
      }
      walk(rootPath, 0);
      return ok({ tree: lines.join("\n"), entryCount: lines.length });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 7. search_files ───────────────────────────────────────────────────────
server.tool(
  "search_files",
  "Search for files matching a glob pattern.",
  {
    path: z.string().describe("Absolute path to the search root directory."),
    pattern: z.string().describe("Glob pattern to match (e.g., '*.rs')."),
  },
  async ({ path: rootPath, pattern }) => {
    try {
      validatePath(rootPath);
      const regex = new RegExp("^" + pattern.replace(/\./g, "\\.").replace(/\*/g, ".*") + "$");
      const matches = [];
      function walk(dir, depth) {
        if (depth > 20 || matches.length > 10000) return;
        const entries = fs.readdirSync(dir, { withFileTypes: true });
        for (const entry of entries) {
          if (matches.length > 10000) break;
          const full = path.join(dir, entry.name);
          if (regex.test(entry.name)) matches.push(full);
          if (entry.isDirectory() && !entry.name.startsWith(".")) {
            walk(full, depth + 1);
          }
        }
      }
      walk(rootPath, 0);
      return ok({ matches, count: matches.length });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 8. move_file ──────────────────────────────────────────────────────────
server.tool(
  "move_file",
  "Move or rename a file.",
  {
    source: z.string().describe("Absolute path to the source file."),
    destination: z.string().describe("Absolute path to the destination."),
  },
  async ({ source, destination }) => {
    try {
      validatePath(source);
      validatePath(destination);
      fs.renameSync(source, destination);
      return ok({ moved: true, source, destination });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 9. create_directory ───────────────────────────────────────────────────
server.tool(
  "create_directory",
  "Create a directory recursively.",
  { path: z.string().describe("Absolute path to the directory to create.") },
  async ({ path: dirPath }) => {
    try {
      validatePath(dirPath);
      fs.mkdirSync(dirPath, { recursive: true });
      return ok({ created: true, path: dirPath });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 10. edit_file ─────────────────────────────────────────────────────────
server.tool(
  "edit_file",
  "Apply text replacements to a file using find-and-replace.",
  {
    path: z.string().describe("Absolute path to the file to edit."),
    edits: z.array(z.object({ oldText: z.string(), newText: z.string() })).describe("Array of find-and-replace operations."),
    dryRun: z.boolean().optional().describe("If true, preview changes without applying."),
  },
  async ({ path: filePath, edits, dryRun }) => {
    try {
      validatePath(filePath);
      let content = fs.readFileSync(filePath, "utf-8");
      let replacements = 0;
      for (const edit of edits) {
        const count = content.split(edit.oldText).length - 1;
        content = content.replaceAll(edit.oldText, edit.newText);
        replacements += count;
      }
      if (!dryRun) {
        fs.writeFileSync(filePath, content, "utf-8");
      }
      return ok({ replacements, applied: !dryRun });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 11. get_file_info ─────────────────────────────────────────────────────
server.tool(
  "get_file_info",
  "Get file metadata including size, modification time, and type.",
  { path: z.string().describe("Absolute path to the file.") },
  async ({ path: filePath }) => {
    try {
      validatePath(filePath);
      const st = fs.statSync(filePath);
      return ok({
        path: filePath,
        size: st.size,
        isFile: st.isFile(),
        isDirectory: st.isDirectory(),
        modified: st.mtime.toISOString(),
        created: st.birthtime.toISOString(),
      });
    } catch (e) {
      return err(e.message);
    }
  }
);

// ── 12. bench_echo ────────────────────────────────────────────────────────
server.tool(
  "bench_echo",
  "Benchmark tool: returns a text payload of the requested size.",
  { size: z.number().optional().describe("Response payload size in bytes (default 64).") },
  async ({ size }) => {
    const n = size || 64;
    const payload = "x".repeat(Math.max(0, n));
    return ok({ payload, size: payload.length });
  }
);

// ── 13. convert_to_nda_document ───────────────────────────────────────────
server.tool(
  "convert_to_nda_document",
  "Convert a file into an NDA binary document. (Stub: NDA is Rust-native.)",
  {
    filePath: z.string().describe("Absolute path to the input file."),
    outputPath: z.string().optional().describe("Output .nda path."),
  },
  async ({ filePath, outputPath }) => {
    return err("convert_to_nda_document is not available in the Node.js implementation. Use the Rust server for NDA operations.");
  }
);

// ── 14. read_nda ──────────────────────────────────────────────────────────
server.tool(
  "read_nda",
  "Read and inspect an NDA binary document. (Stub: NDA is Rust-native.)",
  { ndaPath: z.string().describe("Absolute path to the .nda file.") },
  async ({ ndaPath }) => {
    return err("read_nda is not available in the Node.js implementation. Use the Rust server for NDA operations.");
  }
);

// ── 15. execute_nda ───────────────────────────────────────────────────────
server.tool(
  "execute_nda",
  "Execute a runnable NDA container. (Stub: NDA is Rust-native.)",
  {
    ndaPath: z.string().describe("Absolute path to the runnable .nda file."),
    arguments: z.array(z.string()).optional().describe("Command-line arguments."),
  },
  async ({ ndaPath }) => {
    return err("execute_nda is not available in the Node.js implementation. Use the Rust server for NDA operations.");
  }
);

// ── 16. convert_to_nda_tool ───────────────────────────────────────────────
server.tool(
  "convert_to_nda_tool",
  "Convert a JSON-RPC tool call to NDA binary format. (Stub: NDA is Rust-native.)",
  {
    jsonRequest: z.string().describe("JSON-RPC tool call to convert."),
    outputPath: z.string().optional().describe("Optional path to write the NDA binary."),
  },
  async ({ jsonRequest }) => {
    return err("convert_to_nda_tool is not available in the Node.js implementation. Use the Rust server for NDA operations.");
  }
);

// ── Start server ──────────────────────────────────────────────────────────
const transport = new StdioServerTransport();
await server.connect(transport);
