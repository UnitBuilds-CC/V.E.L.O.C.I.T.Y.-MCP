/**
 * VELOCITY-MCP TypeScript Client SDK
 * 
 * A type-safe TypeScript client for connecting to VELOCITY-MCP servers.
 * 
 * @example
 * ```typescript
 * import { McpClient, HttpTransport } from '@velocity-mcp/client';
 * 
 * async function main() {
 *   // Connect via HTTP
 *   const transport = new HttpTransport('http://localhost:3000/mcp', {
 *     apiKey: 'your-api-key'
 *   });
 *   const client = new McpClient(transport);
 *   
 *   // Initialize connection
 *   await client.initialize();
 *   
 *   // List tools
 *   const tools = await client.listTools();
 *   console.log(`Available tools: ${tools.length}`);
 *   
 *   // Call a tool
 *   const result = await client.callTool('file_read', { path: '/path/to/file.txt' });
 *   console.log(result);
 *   
 *   await client.close();
 * }
 * 
 * main();
 * ```
 */

export { McpClient } from "./client";
export { Transport, HttpTransport, StdioTransport } from "./transport";
export type { HttpTransportOptions, StdioTransportOptions } from "./transport";
export * from "./types";
export * from "./errors";

export const VERSION = "3.0.0";
export const PROTOCOL_VERSION = "2024-11-05";
