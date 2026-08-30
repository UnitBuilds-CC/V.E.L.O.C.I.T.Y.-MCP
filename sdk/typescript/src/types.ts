/**
 * MCP protocol types
 */

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  method: string;
  params?: Record<string, any>;
  id?: number | string;
}

export interface JsonRpcError {
  code: number;
  message: string;
  data?: any;
}

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  result?: any;
  error?: JsonRpcError;
  id?: number | string;
}

export interface ClientInfo {
  name: string;
  version: string;
}

export interface ClientCapabilities {
  roots?: {
    listChanged?: boolean;
  };
}

export interface InitializeParams {
  protocolVersion: string;
  capabilities: ClientCapabilities;
  clientInfo: ClientInfo;
}

export interface ServerInfo {
  name: string;
  version: string;
}

export interface ServerCapabilities {
  tools?: {
    listChanged: boolean;
  };
  resources?: {
    subscribe: boolean;
    listChanged: boolean;
  };
  prompts?: {
    listChanged: boolean;
  };
  sampling?: Record<string, any>;
  logging?: Record<string, any>;
}

export interface InitializeResult {
  protocolVersion: string;
  capabilities: ServerCapabilities;
  serverInfo: ServerInfo;
}

export interface Tool {
  name: string;
  description: string;
  inputSchema: Record<string, any>;
}

export interface ToolsListResult {
  tools: Tool[];
  nextCursor?: string;
}

export interface TextContent {
  type: "text";
  text: string;
}

export interface ImageContent {
  type: "image";
  data: string;
  mimeType: string;
}

export interface ResourceContent {
  uri: string;
  mimeType?: string;
  text?: string;
}

export interface ResourceContentBlock {
  type: "resource";
  resource: ResourceContent;
}

export type Content = TextContent | ImageContent | ResourceContentBlock;

export interface ToolCallResult {
  content: Content[];
  isError: boolean;
}

export interface Resource {
  uri: string;
  name: string;
  description?: string;
  mimeType?: string;
}

export interface ResourcesListResult {
  resources: Resource[];
  nextCursor?: string;
}

export interface ResourceReadResult {
  contents: ResourceContent[];
}

export interface PromptArgument {
  name: string;
  description?: string;
  required?: boolean;
}

export interface Prompt {
  name: string;
  description?: string;
  arguments?: PromptArgument[];
}

export interface PromptsListResult {
  prompts: Prompt[];
  nextCursor?: string;
}

export interface PromptMessage {
  role: string;
  content: Content;
}

export interface PromptGetResult {
  description: string;
  messages: PromptMessage[];
}
