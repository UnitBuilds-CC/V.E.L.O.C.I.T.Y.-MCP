/**
 * MCP client implementation
 */

import { Transport } from "./transport";
import {
  JsonRpcRequest,
  InitializeResult,
  Tool,
  ToolCallResult,
  Resource,
  ResourceReadResult,
  Prompt,
  PromptGetResult,
} from "./types";
import {
  ProtocolError,
  ToolExecutionError,
  ResourceNotFoundError,
  PromptNotFoundError,
  ServerError,
} from "./errors";

const PROTOCOL_VERSION = "2024-11-05";
const CLIENT_NAME = "velocity-mcp-typescript";
const CLIENT_VERSION = "3.2.0";

export class McpClient {
  private transport: Transport;
  private requestId = 0;
  private initialized = false;

  constructor(transport: Transport) {
    this.transport = transport;
  }

  private nextId(): number {
    return ++this.requestId;
  }

  private async sendRequest(
    method: string,
    params?: Record<string, any>
  ): Promise<any> {
    const request: JsonRpcRequest = {
      jsonrpc: "2.0",
      method,
      params,
      id: this.nextId(),
    };

    const response = await this.transport.send(request);

    if (response.error) {
      throw new ProtocolError(
        response.error.code,
        response.error.message,
        response.error.data
      );
    }

    return response.result;
  }

  async initialize(): Promise<InitializeResult> {
    const result = await this.sendRequest("initialize", {
      protocolVersion: PROTOCOL_VERSION,
      capabilities: {},
      clientInfo: {
        name: CLIENT_NAME,
        version: CLIENT_VERSION,
      },
    });

    // Send initialized notification
    await this.transport.send({
      jsonrpc: "2.0",
      method: "notifications/initialized",
    });

    this.initialized = true;
    return result;
  }

  get isInitialized(): boolean {
    return this.initialized;
  }

  async listTools(): Promise<Tool[]> {
    const result = await this.sendRequest("tools/list", {});
    return result.tools;
  }

  async callTool(name: string, args: Record<string, any>): Promise<ToolCallResult> {
    const result = await this.sendRequest("tools/call", {
      name,
      arguments: args,
    });

    if (result.isError) {
      const errorMsg = result.content
        .filter((c: any) => c.type === "text")
        .map((c: any) => c.text)
        .join("\n");
      throw new ToolExecutionError(errorMsg);
    }

    return result;
  }

  async listResources(): Promise<Resource[]> {
    const result = await this.sendRequest("resources/list", {});
    return result.resources;
  }

  async readResource(uri: string): Promise<ResourceReadResult> {
    let result: any;
    try {
      result = await this.sendRequest("resources/read", { uri });
    } catch (error) {
      // velocity-mcp-core, which Edge dispatches through, reports an unservable
      // URI as a JSON-RPC -32001 error object.
      if (error instanceof ProtocolError && error.code === -32001) {
        throw new ResourceNotFoundError(uri);
      }
      throw error;
    }
    // The native server reports the same condition in band, with no error object, so
    // its message text is the only cause signal. Both servers say
    // "Resource not found: <uri>" for an unservable URI (src/resources.rs,
    // crates/velocity-mcp-core/src/lib.rs); anything else is a read failure.
    if (result?.isError) {
      const text: string = result.content?.[0]?.text ?? "";
      if (text.startsWith("Resource not found")) {
        throw new ResourceNotFoundError(uri);
      }
      throw new ServerError(text || `Resource read failed: ${uri}`);
    }
    return result;
  }

  async listPrompts(): Promise<Prompt[]> {
    const result = await this.sendRequest("prompts/list", {});
    return result.prompts;
  }

  async getPrompt(
    name: string,
    args: Record<string, any>
  ): Promise<PromptGetResult> {
    let result: any;
    try {
      result = await this.sendRequest("prompts/get", {
        name,
        arguments: args,
      });
    } catch (error) {
      // velocity-mcp-core, which Edge dispatches through, reports an unknown
      // prompt name as a JSON-RPC -32001 error object.
      if (error instanceof ProtocolError && error.code === -32001) {
        throw new PromptNotFoundError(name);
      }
      throw error;
    }
    // The native server reports the same condition in band, with no error object, so
    // its message text is the only cause signal. Both servers say
    // "Prompt not found: <name>" for an unknown name; a rejected argument set is a
    // different failure and is not relabelled as a missing prompt.
    if (result?.isError) {
      const text: string = result.content?.[0]?.text ?? "";
      if (text.startsWith("Prompt not found")) {
        throw new PromptNotFoundError(name);
      }
      throw new ServerError(text || `Prompt get failed: ${name}`);
    }
    return result;
  }

  async ping(): Promise<void> {
    await this.sendRequest("ping", {});
  }

  async setLogLevel(level: string): Promise<void> {
    await this.sendRequest("logging/setLevel", { level });
  }

  async close(): Promise<void> {
    await this.transport.close();
  }
}
