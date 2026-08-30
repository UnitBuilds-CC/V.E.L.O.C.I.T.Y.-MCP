/**
 * Error types for the MCP client
 */

export class McpError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "McpError";
  }
}

export class TransportError extends McpError {
  constructor(message: string) {
    super(message);
    this.name = "TransportError";
  }
}

export class ProtocolError extends McpError {
  code: number;
  data?: any;

  constructor(code: number, message: string, data?: any) {
    super(`Protocol error ${code}: ${message}`);
    this.name = "ProtocolError";
    this.code = code;
    this.data = data;
  }
}

export class ServerError extends McpError {
  constructor(message: string) {
    super(message);
    this.name = "ServerError";
  }
}

export class InvalidParameterError extends McpError {
  constructor(message: string) {
    super(message);
    this.name = "InvalidParameterError";
  }
}

export class ConnectionClosedError extends McpError {
  constructor(message: string = "Connection closed") {
    super(message);
    this.name = "ConnectionClosedError";
  }
}

export class TimeoutError extends McpError {
  constructor(message: string = "Request timed out") {
    super(message);
    this.name = "TimeoutError";
  }
}

export class ToolExecutionError extends McpError {
  constructor(message: string) {
    super(message);
    this.name = "ToolExecutionError";
  }
}

export class ResourceNotFoundError extends McpError {
  constructor(uri: string) {
    super(`Resource not found: ${uri}`);
    this.name = "ResourceNotFoundError";
  }
}

export class PromptNotFoundError extends McpError {
  constructor(name: string) {
    super(`Prompt not found: ${name}`);
    this.name = "PromptNotFoundError";
  }
}
