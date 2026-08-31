// Minimal Node.js MCP server for benchmark comparison.
// Handles: initialize, notifications/initialized, ping, tools/list, tools/call, health/check
// Uses raw JSON-RPC over stdio — no SDK, to match what a typical Node.js MCP server does.

const readline = require('readline');

const TOOLS = [
  {
    name: 'read_file',
    description: 'Read the contents of a file',
    inputSchema: {
      type: 'object',
      properties: { path: { type: 'string', description: 'File path' } },
      required: ['path']
    }
  },
  {
    name: 'write_file',
    description: 'Write content to a file',
    inputSchema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'File path' },
        content: { type: 'string', description: 'Content to write' }
      },
      required: ['path', 'content']
    }
  },
  {
    name: 'list_dir',
    description: 'List directory contents',
    inputSchema: {
      type: 'object',
      properties: { path: { type: 'string', description: 'Directory path' } },
      required: ['path']
    }
  },
  {
    name: 'grep_search',
    description: 'Search for a pattern in files',
    inputSchema: {
      type: 'object',
      properties: {
        pattern: { type: 'string', description: 'Search pattern' },
        path: { type: 'string', description: 'Directory to search' }
      },
      required: ['pattern', 'path']
    }
  },
  {
    name: 'bench_echo',
    description: 'Benchmark tool: returns a text payload of the requested size in bytes',
    inputSchema: {
      type: 'object',
      properties: {
        size: { type: 'integer', description: 'Response payload size in bytes (default 64)' }
      },
      required: []
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
