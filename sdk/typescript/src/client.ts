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
} from "./errors";

const PROTOCOL_VERSION = "2024-11-05";
const CLIENT_NAME = "velocity-mcp-typescript";
const CLIENT_VERSION = "3.0.0";

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
    try {
      return await this.sendRequest("resources/read", { uri });
    } catch (error) {
      if (error instanceof ProtocolError && error.code === -32003) {
        throw new ResourceNotFoundError(uri);
      }
      throw error;
    }
  }

  async listPrompts(): Promise<Prompt[]> {
    const result = await this.sendRequest("prompts/list", {});
    return result.prompts;
  }

  async getPrompt(
    name: string,
    args: Record<string, any>
  ): Promise<PromptGetResult> {
    try {
      return await this.sendRequest("prompts/get", {
        name,
        arguments: args,
      });
    } catch (error) {
      if (error instanceof ProtocolError && error.code === -32004) {
        throw new PromptNotFoundError(name);
      }
      throw error;
    }
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
