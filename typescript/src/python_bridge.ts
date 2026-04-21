/**
 * Loopback HTTP bridge for the `python` transform mode.
 *
 * Sibling of {@link CliBridge} but with a single, JSON-only endpoint that the generated Python
 * stubs (via `callback_client.py`) call to invoke MCP tools through the `CompressorRuntime`.
 *
 * Wire protocol:
 *
 *   POST /function
 *   { "service": "<server>", "function": "<tool>", "params": { ... } }
 *
 *   → 200 OK
 *     { "success": true,  "data": "<string>" }
 *     { "success": false, "error": "<msg>", "errorType": "<class>", "statusCode": <int> }
 *
 * The bridge always returns HTTP 200 and encodes failure in the JSON body so that the Python
 * client can distinguish protocol errors from tool errors uniformly. Other paths return 404.
 *
 * The bridge listens only on `127.0.0.1` and binds an OS-assigned port by default. Consumers are
 * responsible for ensuring the execution environment that runs the generated Python can reach
 * the loopback interface.
 */

import http from "node:http";

import type { Tool } from "@modelcontextprotocol/sdk/types.js";

interface PythonBridgeRuntime {
  invokeToolForCli(
    toolName: string,
    toolInput: Record<string, unknown> | undefined,
    options?: { toonify?: boolean },
  ): Promise<string>;
  listUncompressedTools(): Promise<Tool[]>;
}

interface FunctionRequestBody {
  service?: unknown;
  function?: unknown;
  params?: unknown;
}

interface SuccessResponse {
  success: true;
  data: string;
}

interface FailureResponse {
  success: false;
  error: string;
  errorType: string;
  statusCode: number;
}

export type FunctionResponse = SuccessResponse | FailureResponse;

export class PythonBridge {
  private readonly runtimes: ReadonlyMap<string, PythonBridgeRuntime>;
  private server: http.Server | null = null;
  private cachedToolsByService: Map<string, Tool[]> = new Map();

  /**
   * Construct a bridge that fronts one or more services. The single-service overload (passing a
   * runtime + name) is preserved for backwards compatibility; the multi-service overload accepts
   * a `Map<serviceName, runtime>` so one HTTP listener can route to N backends by `service`.
   */
  constructor(runtime: PythonBridgeRuntime, serverName: string);
  constructor(runtimes: ReadonlyMap<string, PythonBridgeRuntime>);
  constructor(
    runtimeOrMap: PythonBridgeRuntime | ReadonlyMap<string, PythonBridgeRuntime>,
    serverName?: string,
  ) {
    if (runtimeOrMap instanceof Map) {
      this.runtimes = runtimeOrMap;
    } else if (serverName !== undefined) {
      this.runtimes = new Map([[serverName, runtimeOrMap as PythonBridgeRuntime]]);
    } else {
      throw new Error("PythonBridge requires either a (runtime, serverName) pair or a Map.");
    }
  }

  /** Service names this bridge fronts. */
  get serverNames(): readonly string[] {
    return [...this.runtimes.keys()];
  }

  /** URL the Python client should POST to. Throws if `start` hasn't been called. */
  get url(): string {
    const address = this.server?.address();
    if (!address || typeof address === "string") {
      throw new Error("Code bridge is not listening.");
    }
    return `http://127.0.0.1:${address.port}`;
  }

  /** Bound port (after `start`). Throws if not listening. */
  get port(): number {
    const address = this.server?.address();
    if (!address || typeof address === "string") {
      throw new Error("Code bridge is not listening.");
    }
    return address.port;
  }

  async start(port = 0): Promise<number> {
    await this.ensureTools();
    this.server = http.createServer((request, response) => {
      this.handleRequest(request, response).catch((error: unknown) => {
        response.statusCode = 500;
        response.setHeader("content-type", "application/json; charset=utf-8");
        response.end(
          JSON.stringify({
            success: false,
            error: error instanceof Error ? error.message : String(error),
            errorType: error instanceof Error ? error.constructor.name : "Error",
            statusCode: 500,
          } satisfies FailureResponse),
        );
      });
    });

    await new Promise<void>((resolve, reject) => {
      this.server?.once("error", reject);
      this.server?.listen(port, "127.0.0.1", () => {
        resolve();
      });
    });

    return this.port;
  }

  async close(): Promise<void> {
    if (!this.server) {
      return;
    }
    const server = this.server;
    this.server = null;
    await new Promise<void>((resolve, reject) => {
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve();
      });
    });
  }

  private async ensureTools(): Promise<void> {
    if (this.cachedToolsByService.size > 0) {
      return;
    }
    for (const [name, runtime] of this.runtimes) {
      this.cachedToolsByService.set(name, await runtime.listUncompressedTools());
    }
  }

  private async handleRequest(
    request: http.IncomingMessage,
    response: http.ServerResponse,
  ): Promise<void> {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");

    if (request.method === "GET" && url.pathname === "/health") {
      this.sendJson(response, 200, { ok: true, services: this.serverNames });
      return;
    }

    if (request.method === "POST" && url.pathname === "/function") {
      const body = await this.readJsonBody(request);
      const result = await this.invokeFunction(body);
      this.sendJson(response, 200, result);
      return;
    }

    this.sendJson(response, 404, {
      success: false,
      error: `Not found: ${request.method ?? "?"} ${url.pathname}`,
      errorType: "NotFound",
      statusCode: 404,
    } satisfies FailureResponse);
  }

  private async invokeFunction(body: FunctionRequestBody): Promise<FunctionResponse> {
    const { service, function: fnName, params } = body;
    if (typeof service !== "string" || service.length === 0) {
      return failure("Missing or invalid `service` field.", "InvalidArguments", 400);
    }
    const runtime = this.runtimes.get(service);
    if (runtime === undefined) {
      return failure(
        `Unknown service: ${service} (this bridge serves: ${this.serverNames.join(", ")}).`,
        "UnknownService",
        404,
      );
    }
    if (typeof fnName !== "string" || fnName.length === 0) {
      return failure("Missing or invalid `function` field.", "InvalidArguments", 400);
    }
    const inputObj = normalizeParams(params);
    if (inputObj instanceof Error) {
      return failure(inputObj.message, "InvalidArguments", 400);
    }

    try {
      const data = await runtime.invokeToolForCli(fnName, inputObj, { toonify: false });
      return { success: true, data };
    } catch (error) {
      return failure(
        error instanceof Error ? error.message : String(error),
        error instanceof Error ? error.constructor.name : "Error",
        500,
      );
    }
  }

  private sendJson(response: http.ServerResponse, statusCode: number, body: unknown): void {
    response.statusCode = statusCode;
    response.setHeader("content-type", "application/json; charset=utf-8");
    response.end(JSON.stringify(body));
  }

  private async readJsonBody(request: http.IncomingMessage): Promise<FunctionRequestBody> {
    const chunks: Buffer[] = [];
    for await (const chunk of request) {
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk as Uint8Array));
    }
    const raw = Buffer.concat(chunks).toString("utf8");
    if (raw.length === 0) {
      return {};
    }
    try {
      const parsed: unknown = JSON.parse(raw);
      if (parsed === null || typeof parsed !== "object") {
        return {};
      }
      return parsed as FunctionRequestBody;
    } catch {
      return {};
    }
  }
}

function failure(message: string, errorType: string, statusCode: number): FailureResponse {
  return { success: false, error: message, errorType, statusCode };
}

function normalizeParams(params: unknown): Record<string, unknown> | undefined | Error {
  if (params === undefined || params === null) {
    return undefined;
  }
  if (typeof params !== "object" || Array.isArray(params)) {
    return new Error("`params` must be an object.");
  }
  return params as Record<string, unknown>;
}
