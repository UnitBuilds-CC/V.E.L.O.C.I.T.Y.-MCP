# VELOCITY-MCP TypeScript Client SDK

A type-safe TypeScript client for connecting to VELOCITY-MCP servers.

## Installation

```bash
npm install @velocity-mcp/client
```

Or with yarn:

```bash
yarn add @velocity-mcp/client
```

## Quick Start

### HTTP Transport

```typescript
import { McpClient, HttpTransport } from '@velocity-mcp/client';

async function main() {
  // Connect via HTTP
  const transport = new HttpTransport('http://localhost:3000/mcp', {
    apiKey: 'your-api-key', // Optional
    timeout: 30000 // Optional, in milliseconds
  });
  const client = new McpClient(transport);
  
  try {
    // Initialize connection
    const initResult = await client.initialize();
    console.log(`Connected to ${initResult.serverInfo.name} v${initResult.serverInfo.version}`);
    
    // List available tools
    const tools = await client.listTools();
    console.log(`Available tools: ${tools.length}`);
    tools.forEach(tool => {
      console.log(`  - ${tool.name}: ${tool.description}`);
    });
    
    // Call a tool
    const result = await client.callTool('file_read', { path: '/path/to/file.txt' });
    result.content.forEach(content => {
      if (content.type === 'text') {
        console.log(`File contents:\n${content.text}`);
      }
    });
    
  } finally {
    await client.close();
  }
}

main();
```

### Stdio Transport

```typescript
import { McpClient, StdioTransport } from '@velocity-mcp/client';

async function main() {
  // Connect via stdio (spawns server process)
  const transport = new StdioTransport({
    command: 'velocity_mcp',
    args: ['--mode', 'stdio']
  });
  const client = new McpClient(transport);
  
  try {
    await client.initialize();
    
    // Use the client...
    const tools = await client.listTools();
    console.log(`Found ${tools.length} tools`);
    
  } finally {
    await client.close();
  }
}

main();
```

## API Reference

### McpClient

The main client class for interacting with MCP servers.

#### Constructor

```typescript
new McpClient(transport: Transport)
```

#### Methods

- `initialize(): Promise<InitializeResult>` - Initialize the MCP connection
- `isInitialized: boolean` - Check if client is initialized (property)
- `listTools(): Promise<Tool[]>` - List available tools
- `callTool(name: string, args: Record<string, any>): Promise<ToolCallResult>` - Call a tool
- `listResources(): Promise<Resource[]>` - List available resources
- `readResource(uri: string): Promise<ResourceReadResult>` - Read a resource
- `listPrompts(): Promise<Prompt[]>` - List available prompts
- `getPrompt(name: string, args: Record<string, any>): Promise<PromptGetResult>` - Get a prompt
- `ping(): Promise<void>` - Ping the server
- `setLogLevel(level: string): Promise<void>` - Set logging level
- `close(): Promise<void>` - Close the connection

### Transport Classes

#### HttpTransport

HTTP transport for connecting to MCP servers over HTTP.

```typescript
new HttpTransport(url: string, options?: HttpTransportOptions)

interface HttpTransportOptions {
  apiKey?: string;    // Optional API key for authentication
  timeout?: number;   // Request timeout in milliseconds (default: 30000)
}
```

#### StdioTransport

Stdio transport for spawning and communicating with MCP server processes.

```typescript
new StdioTransport(options: StdioTransportOptions)

interface StdioTransportOptions {
  command: string;    // Command to run (e.g., "velocity_mcp")
  args?: string[];    // Command arguments (e.g., ["--mode", "stdio"])
}
```

### Error Handling

The SDK provides specific exception types for different error scenarios:

```typescript
import {
  McpError,              // Base exception
  TransportError,        // Connection/IO errors
  ProtocolError,         // JSON-RPC protocol errors
  ToolExecutionError,    // Tool execution failures
  ResourceNotFoundError, // Resource not found
  PromptNotFoundError,   // Prompt not found
} from '@velocity-mcp/client';

try {
  const result = await client.callTool('file_read', { path: '/missing.txt' });
} catch (error) {
  if (error instanceof ToolExecutionError) {
    console.error(`Tool failed: ${error.message}`);
  } else if (error instanceof TransportError) {
    console.error(`Connection error: ${error.message}`);
  } else if (error instanceof ProtocolError) {
    console.error(`Protocol error ${error.code}: ${error.message}`);
  }
}
```

## Examples

### File Operations

```typescript
// Read a file
const result = await client.callTool('file_read', {
  path: '/path/to/file.txt'
});

// Write a file
await client.callTool('file_write', {
  path: '/path/to/output.txt',
  content: 'Hello, World!'
});
```

### Shell Commands

```typescript
// Execute a shell command
const result = await client.callTool('shell_exec', {
  command: 'ls -la',
  timeout: 30
});

result.content.forEach(content => {
  if (content.type === 'text') {
    console.log(content.text);
  }
});
```

### HTTP Requests

```typescript
// Make an HTTP request
const result = await client.callTool('http_request', {
  url: 'https://api.example.com/data',
  method: 'GET',
  timeout: 60
});
```

### Working with Resources

```typescript
// List all resources
const resources = await client.listResources();
resources.forEach(resource => {
  console.log(`${resource.uri}: ${resource.name}`);
});

// Read a specific resource
const result = await client.readResource('file:///path/to/file.txt');
result.contents.forEach(content => {
  if (content.text) {
    console.log(content.text);
  }
});
```

### Working with Prompts

```typescript
// List all prompts
const prompts = await client.listPrompts();
prompts.forEach(prompt => {
  console.log(`${prompt.name}: ${prompt.description}`);
});

// Get a prompt with arguments
const result = await client.getPrompt('code-review', {
  code: 'function hello() { console.log("Hello"); }'
});
result.messages.forEach(message => {
  console.log(`${message.role}: ${message.content}`);
});
```

## Type Safety

The SDK provides full TypeScript type definitions:

```typescript
import { Tool, Resource, Prompt, Content } from '@velocity-mcp/client';

// All types are properly typed
const tools: Tool[] = await client.listTools();
const resources: Resource[] = await client.listResources();
const prompts: Prompt[] = await client.listPrompts();

// Content is a union type
const content: Content = result.content[0];
if (content.type === 'text') {
  console.log(content.text); // TypeScript knows this is a string
}
```

## Async/Await Support

The SDK is fully async using Promises:

```typescript
async function main() {
  const client = new McpClient(new HttpTransport('http://localhost:3000/mcp'));
  await client.initialize();
  
  // Make multiple concurrent requests
  const [tools, resources] = await Promise.all([
    client.listTools(),
    client.listResources()
  ]);
  
  console.log(`Tools: ${tools.length}, Resources: ${resources.length}`);
  
  await client.close();
}
```

## Development

### Setup

```bash
cd sdk/typescript
npm install
```

### Build

```bash
npm run build
```

### Test

```bash
npm test
```

### Lint

```bash
npm run lint
```

### Format

```bash
npm run format
```

## Requirements

- Node.js 18+
- TypeScript 5.0+ (for development)

## License

MIT OR Apache-2.0

## Links

- [VELOCITY-MCP Server](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP)
- [MCP Protocol Specification](https://modelcontextprotocol.io/)
- [API Documentation](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/tree/main/docs/API.md)
