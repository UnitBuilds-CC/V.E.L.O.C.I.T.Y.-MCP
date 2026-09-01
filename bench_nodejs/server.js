// Minimal Node.js MCP server for benchmark comparison.
// Handles: initialize, notifications/initialized, ping, tools/list, tools/call, health/check
// Uses raw JSON-RPC over stdio — no SDK, to match what a typical Node.js MCP server does.

const readline = require('readline');

const TOOLS = [
  {
    name: 'file_read',
    description: "Read a file's contents as UTF-8 text. Use for inspecting source code, configs, logs, or any text file. Returns the full file content as a string. Fails if the file is binary or does not exist.",
    inputSchema: {
      type: 'object',
      properties: { path: { type: 'string', description: 'Absolute path to the file.' } },
      required: ['path']
    }
  },
  {
    name: 'file_write',
    description: 'Write text content to a file. Creates parent directories if needed. Overwrites existing files entirely. Use for generating code, configs, or any text output. Returns bytes written.',
    inputSchema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'Absolute path to the file.' },
        content: { type: 'string', description: 'Text content to write.' }
      },
      required: ['path', 'content']
    }
  },
  {
    name: 'shell_exec',
    description: "Execute a shell command with timeout enforcement and security validation. Blocks dangerous system-level patterns (rm -rf /, format, diskpart, encoded PowerShell, etc). All invocations are audit-logged. Returns exit code, stdout, and stderr. Use for running builds, tests, git commands, or system utilities. Commands run with the server's permissions.",
    inputSchema: {
      type: 'object',
      properties: {
        command: { type: 'string', description: 'Shell command to execute.' },
        workingDir: { type: 'string', description: 'Working directory (absolute path). Optional.' },
        timeout: { type: 'integer', description: 'Timeout in seconds (default: 30). Command will be killed if it exceeds this.' }
      },
      required: ['command']
    }
  },
  {
    name: 'http_request',
    description: 'Make an HTTP request with timeout enforcement and SSRF protection. Blocks requests to localhost and private IPs. Supports GET, POST, PUT, DELETE, PATCH, HEAD. Returns status code, status text, and response body. Use for calling APIs, fetching data, or testing endpoints.',
    inputSchema: {
      type: 'object',
      properties: {
        url: { type: 'string', description: 'Target URL (must be http:// or https://).' },
        method: { type: 'string', description: 'HTTP method. Default: GET.' },
        headers: { type: 'object', description: 'Request headers as key-value pairs.' },
        body: { type: 'string', description: 'Request body (for POST/PUT/PATCH).' },
        timeout: { type: 'integer', description: 'Timeout in seconds (default: 30).' }
      },
      required: ['url']
    }
  },
  {
    name: 'list_directory',
    description: 'List contents of a directory. Returns files and subdirectories with metadata (size, type). Use for exploring directory structure.',
    inputSchema: {
      type: 'object',
      properties: { path: { type: 'string', description: 'Absolute path to the directory.' } },
      required: ['path']
    }
  },
  {
    name: 'directory_tree',
    description: 'Recursively list directory contents as a tree structure. Shows nested files and directories with indentation. Use for visualizing project structure.',
    inputSchema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'Absolute path to the root directory.' },
        excludePatterns: { type: 'array', items: { type: 'string' }, description: "Glob patterns to exclude (e.g., ['*.log', 'node_modules'])" }
      },
      required: ['path']
    }
  },
  {
    name: 'search_files',
    description: 'Search for files matching a glob pattern within a directory. Recursively searches subdirectories. Use for finding files by name or extension.',
    inputSchema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'Absolute path to the search root directory.' },
        pattern: { type: 'string', description: "Glob pattern to match (e.g., '*.rs', 'test_*.py')." }
      },
      required: ['path', 'pattern']
    }
  },
  {
    name: 'move_file',
    description: 'Move or rename a file. Can move across directories. Fails if destination exists. Use for reorganizing files.',
    inputSchema: {
      type: 'object',
      properties: {
        source: { type: 'string', description: 'Absolute path to the source file.' },
        destination: { type: 'string', description: 'Absolute path to the destination.' }
      },
      required: ['source', 'destination']
    }
  },
  {
    name: 'create_directory',
    description: 'Create a directory recursively (like mkdir -p). Creates parent directories if needed. Succeeds silently if directory already exists.',
    inputSchema: {
      type: 'object',
      properties: { path: { type: 'string', description: 'Absolute path to the directory to create.' } },
      required: ['path']
    }
  },
  {
    name: 'edit_file',
    description: 'Apply text replacements to a file using find-and-replace. Supports dry-run mode to preview changes. Use for targeted edits without rewriting entire file.',
    inputSchema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'Absolute path to the file to edit.' },
        edits: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              oldText: { type: 'string', description: 'Exact text to find and replace.' },
              newText: { type: 'string', description: 'Replacement text.' }
            },
            required: ['oldText', 'newText']
          },
          description: 'Array of find-and-replace operations.'
        },
        dryRun: { type: 'boolean', description: 'If true, preview changes without applying them.' }
      },
      required: ['path', 'edits']
    }
  },
  {
    name: 'get_file_info',
    description: 'Get file metadata including size, modification time, permissions, and type. Use for inspecting file properties.',
    inputSchema: {
      type: 'object',
      properties: { path: { type: 'string', description: 'Absolute path to the file.' } },
      required: ['path']
    }
  },
  {
    name: 'bench_echo',
    description: 'Benchmark tool: returns a text payload of the requested size in bytes. Used for measuring serialization cost at different payload sizes.',
    inputSchema: {
      type: 'object',
      properties: { size: { type: 'integer', description: 'Response payload size in bytes (default 64).' } },
      required: []
    }
  },
  {
    name: 'convert_to_nda_document',
    description: 'Convert a file into a cryptographically signed NDA binary document. NDA is a zero-allocation format with semantic triples, Merkle integrity, and Ed25519 signatures. Accepts: source code, PDF, CSV, Excel, images, archives. Returns the output path and file size.',
    inputSchema: {
      type: 'object',
      properties: {
        filePath: { type: 'string', description: 'Absolute path to the input file.' },
        outputPath: { type: 'string', description: 'Output .nda path. Defaults to input with .nda extension.' }
      },
      required: ['filePath']
    }
  },
  {
    name: 'read_nda',
    description: 'Read and inspect an NDA binary document. Shows semantic triples, visual display commands, string pool contents, Merkle integrity status, and Ed25519 signature verification. Use to examine or debug NDA files.',
    inputSchema: {
      type: 'object',
      properties: { ndaPath: { type: 'string', description: 'Absolute path to the .nda file.' } },
      required: ['ndaPath']
    }
  },
  {
    name: 'execute_nda',
    description: "Execute a runnable NDA container. Runs compiled binaries in-memory or scripts (Python, Node.js, PowerShell, Bash) via shell. Returns the program's stdout. Use for running sandboxed executables packaged as NDA documents.",
    inputSchema: {
      type: 'object',
      properties: {
        ndaPath: { type: 'string', description: 'Absolute path to the runnable .nda file.' },
        arguments: { type: 'array', items: { type: 'string' }, description: 'Command-line arguments.' }
      },
      required: ['ndaPath']
    }
  },
  {
    name: 'convert_to_nda_tool',
    description: 'Convert a JSON-RPC tool call to NDA binary format and register it for fast execution. Subsequent calls parse about 2.8x faster than JSON (measured). The converted tool is immediately available by name.',
    inputSchema: {
      type: 'object',
      properties: {
        jsonRequest: { type: 'string', description: 'JSON-RPC tool call to convert.' },
        outputPath: { type: 'string', description: 'Optional path to write the NDA binary. Tool is registered regardless.' }
      },
      required: ['jsonRequest']
    }
  }
];

function handleRequest(req) {
  const { method, id, params } = req;

  switch (method) {
    case 'initialize':
      return {
        jsonrpc: '2.0', id,
        result: {
          protocolVersion: '2024-11-05',
          capabilities: { tools: {} },
          serverInfo: { name: 'nodejs-mcp-benchmark', version: '1.0.0' }
        }
      };

    case 'notifications/initialized':
      return null;

    case 'ping':
      return { jsonrpc: '2.0', id, result: {} };

    case 'tools/list':
      return { jsonrpc: '2.0', id, result: { tools: TOOLS } };

    case 'tools/call': {
      const name = params?.name || '';
      const args = params?.arguments || {};
      if (name === 'bench_echo') {
        const size = typeof args.size === 'number' ? args.size : 64;
        const padding = 'x'.repeat(Math.max(0, size));
        return {
          jsonrpc: '2.0', id,
          result: {
            content: [{ type: 'text', text: padding }],
            isError: false
          }
        };
      }
      return {
        jsonrpc: '2.0', id,
        result: {
          content: [{ type: 'text', text: `Executed tool '${name}' with args: ${JSON.stringify(args)}` }],
          isError: false
        }
      };
    }

    case 'health/check':
      return { jsonrpc: '2.0', id, result: { status: 'healthy', mode: 'stdio', version: '1.0.0' } };

    default:
      if (id !== undefined) {
        return { jsonrpc: '2.0', id, error: { code: -32601, message: `Method '${method}' not found` } };
      }
      return null;
  }
}

const http = require('http');

function startStdio() {
  const rl = readline.createInterface({ input: process.stdin, terminal: false });
  rl.on('line', (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    try {
      const req = JSON.parse(trimmed);
      const res = handleRequest(req);
      if (res) process.stdout.write(JSON.stringify(res) + '\n');
    } catch (e) {
      process.stdout.write(JSON.stringify({
        jsonrpc: '2.0', error: { code: -32700, message: 'Parse error' }, id: null
      }) + '\n');
    }
  });
}

function startHttp(port) {
  const server = http.createServer((req, res) => {
    if (req.method === 'POST' && (req.url === '/v1/mcp' || req.url === '/mcp')) {
      let body = '';
      req.on('data', (chunk) => { body += chunk; });
      req.on('end', () => {
        try {
          const jsonReq = JSON.parse(body);
          const jsonRes = handleRequest(jsonReq);
          if (jsonRes) {
            const out = JSON.stringify(jsonRes);
            res.writeHead(200, { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(out) });
            res.end(out);
          } else {
            res.writeHead(204);
            res.end();
          }
        } catch (e) {
          const err = JSON.stringify({ jsonrpc: '2.0', error: { code: -32700, message: 'Parse error' }, id: null });
          res.writeHead(400, { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(err) });
          res.end(err);
        }
      });
    } else if (req.method === 'GET' && req.url === '/health') {
      const out = JSON.stringify({ status: 'healthy', mode: 'http', version: '1.0.0' });
      res.writeHead(200, { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(out) });
      res.end(out);
    } else {
      res.writeHead(404);
      res.end();
    }
  });
  server.listen(port, '127.0.0.1', () => {
    process.stderr.write(`Node.js HTTP MCP server listening on 127.0.0.1:${port}\n`);
  });
}

const args = process.argv.slice(2);
const httpIdx = args.indexOf('--http');
if (httpIdx !== -1 && args[httpIdx + 1]) {
  startHttp(parseInt(args[httpIdx + 1], 10));
} else {
  startStdio();
}
