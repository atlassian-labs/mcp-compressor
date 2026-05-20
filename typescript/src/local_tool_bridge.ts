import type { IncomingMessage } from "node:http";

import type { ExecutableTool } from "./adapters.js";
import { stringifyToolResult } from "./tool_specs.js";

export interface LocalToolBridge {
  bridgeUrl: string;
  token: string;
  close(): void;
}

export async function startLocalToolBridge(
  tools: Record<string, ExecutableTool<unknown>>,
): Promise<LocalToolBridge> {
  const http = await import("node:http");
  const token = crypto.randomUUID();
  const server = http.createServer(async (request, response) => {
    try {
      if (request.method === "GET" && request.url === "/health") {
        response.writeHead(200, { "content-type": "text/plain" }).end("ok");
        return;
      }
      if (request.method !== "POST" || request.url !== "/exec") {
        response.writeHead(404).end("not found");
        return;
      }
      if (request.headers.authorization !== `Bearer ${token}`) {
        response.writeHead(401).end("unauthorized");
        return;
      }
      const body = JSON.parse(await readRequestBody(request)) as {
        tool?: string;
        input?: Record<string, unknown>;
        tool_name?: string;
        tool_input?: Record<string, unknown>;
      };
      const toolName = String(body.tool ?? body.tool_name ?? "");
      const tool = tools[toolName];
      if (!tool) {
        response
          .writeHead(404, { "content-type": "application/json" })
          .end(JSON.stringify({ error: `Tool not found: ${toolName}` }));
        return;
      }
      const result = await tool.execute(body.input ?? body.tool_input ?? {});
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify({ result: stringifyToolResult(result) }));
    } catch (error) {
      response
        .writeHead(500, { "content-type": "application/json" })
        .end(JSON.stringify({ error: error instanceof Error ? error.message : String(error) }));
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Failed to bind local tool bridge");
  return {
    bridgeUrl: `http://127.0.0.1:${address.port}`,
    token,
    close: () => server.close(),
  };
}

function readRequestBody(request: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer) => chunks.push(chunk));
    request.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    request.on("error", reject);
  });
}
