/**
 * Transport implementations for MCP client
 */

import axios, { AxiosInstance } from "axios";
import { spawn, ChildProcess } from "child_process";
import { JsonRpcRequest, JsonRpcResponse } from "./types";
import { TransportError, ConnectionClosedError } from "./errors";

export interface Transport {
  send(request: JsonRpcRequest): Promise<JsonRpcResponse | undefined>;
  close(): Promise<void>;
}

export interface HttpTransportOptions {
  apiKey?: string;
  timeout?: number;
}

export class HttpTransport implements Transport {
  private client: AxiosInstance;
  private url: string;

  constructor(url: string, options: HttpTransportOptions = {}) {
    this.url = url;
    const { apiKey, timeout = 30000 } = options;

    this.client = axios.create({
      timeout,
      headers: {
        "Content-Type": "application/json",
        ...(apiKey && { Authorization: `Bearer ${apiKey}` }),
      },
    });
  }

  async send(request: JsonRpcRequest): Promise<JsonRpcResponse | undefined> {
    try {
      const response = await this.client.post(this.url, request);
      if (!response.data) {
        // A notification is answered with an empty 204, so there is no envelope.
        return undefined;
      }
      return response.data;
    } catch (error: any) {
      if (error.code === "ECONNABORTED") {
        throw new TransportError("Request timed out");
      }
      if (error.response) {
        throw new TransportError(
          `HTTP error ${error.response.status}: ${error.response.data}`
        );
      }
      throw new TransportError(`Transport error: ${error.message}`);
    }
  }

  async close(): Promise<void> {
    // HTTP transport doesn't need explicit close
  }
}

export interface StdioTransportOptions {
  command: string;
  args?: string[];
}

export class StdioTransport implements Transport {
  private process: ChildProcess | null = null;
  private command: string;
  private args: string[];
  private requestId = 0;

  constructor(options: StdioTransportOptions) {
    this.command = options.command;
    this.args = options.args || [];
  }

  private ensureProcess(): ChildProcess {
    if (!this.process || this.process.exitCode !== null) {
      this.process = spawn(this.command, this.args, {
        stdio: ["pipe", "pipe", "pipe"],
      });

      this.process.on("error", (error) => {
        throw new TransportError(`Process error: ${error.message}`);
      });
    }
    return this.process;
  }

  async send(request: JsonRpcRequest): Promise<JsonRpcResponse | undefined> {
    const process = this.ensureProcess();

    if (!process.stdin || !process.stdout) {
      throw new ConnectionClosedError("Process stdin/stdout not available");
    }

    const requestJson = JSON.stringify(request);

    if (request.id === undefined) {
      // The server writes nothing to stdout for a notification, so waiting for a
      // line here would leave the initialize handshake pending forever.
      process.stdin!.write(requestJson + "\n");
      return;
    }

    return new Promise((resolve, reject) => {
      process.stdin!.write(requestJson + "\n");

      let responseData = "";
      const onData = (data: Buffer) => {
        responseData += data.toString();
        if (responseData.includes("\n")) {
          process.stdout!.removeListener("data", onData);
          try {
            const response = JSON.parse(responseData.trim());
            resolve(response);
          } catch (error: any) {
            reject(new TransportError(`Failed to parse response: ${error.message}`));
          }
        }
      };

      process.stdout.on("data", onData);

      process.once("exit", (code) => {
        if (code !== null && code !== 0) {
          reject(new ConnectionClosedError(`Process exited with code ${code}`));
        }
      });
    });
  }

  async close(): Promise<void> {
    if (this.process && this.process.exitCode === null) {
      this.process.kill();
      await new Promise<void>((resolve) => {
        this.process!.once("exit", () => resolve());
      });
    }
  }
}
