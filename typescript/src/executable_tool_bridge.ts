import type { ExecutableTool } from "./adapters.js";
import { startLocalToolBridge } from "./local_tool_bridge.js";
import type { ToolSpec } from "./rust_core.js";
import { executableToolsToSpecs } from "./tool_specs.js";

export interface ExecutableToolBridgeServer {
  name: string;
  tools: ToolSpec[];
  bridgeUrl: string;
}

export interface ExecutableToolBridgeRuntime {
  serverName: string;
  server: ExecutableToolBridgeServer;
  invokeTool(toolName: string, input?: Record<string, unknown>): Promise<unknown>;
}

export interface ExecutableToolBridge extends ExecutableToolBridgeRuntime {
  bridgeUrl: string;
  token: string;
  close(): void;
}

export interface CreateExecutableToolBridgeOptions {
  serverName: string;
}

export async function createExecutableToolBridge(
  tools: Record<string, ExecutableTool<unknown>>,
  options: CreateExecutableToolBridgeOptions,
): Promise<ExecutableToolBridge> {
  const bridge = await startLocalToolBridge(tools);
  const server: ExecutableToolBridgeServer = {
    name: options.serverName,
    tools: executableToolsToSpecs(tools),
    bridgeUrl: bridge.bridgeUrl,
  };

  return {
    serverName: options.serverName,
    server,
    bridgeUrl: bridge.bridgeUrl,
    token: bridge.token,
    close: bridge.close,
    invokeTool: async (toolName, input = {}) =>
      invokeBridgeTool(bridge.bridgeUrl, bridge.token, toolName, input),
  };
}

async function invokeBridgeTool(
  bridgeUrl: string,
  token: string,
  toolName: string,
  input: Record<string, unknown>,
): Promise<unknown> {
  const response = await fetch(`${bridgeUrl}/exec`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ tool: toolName, input }),
  });
  const payload = (await response.json()) as { result?: unknown; error?: unknown };
  if (!response.ok) {
    throw new Error(
      typeof payload.error === "string" ? payload.error : JSON.stringify(payload.error),
    );
  }
  return payload.result;
}
