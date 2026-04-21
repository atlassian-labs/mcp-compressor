import { test, expect } from "vitest";

import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import { PythonBridge } from "../src/python_bridge.js";
import { CompressorRuntime } from "../src/runtime.js";
import type { BackendToolClient } from "../src/types.js";

class FakeBackendClient implements BackendToolClient {
  callToolCalls: Array<{ name: string; args: Record<string, unknown> | undefined }> = [];

  constructor(private readonly tools: Tool[]) {}

  async connect(): Promise<void> {}
  async disconnect(): Promise<void> {}
  async listTools(): Promise<Tool[]> {
    return this.tools;
  }
  async callTool(name: string, args: Record<string, unknown> | undefined): Promise<unknown> {
    this.callToolCalls.push({ name, args });
    if (name === "boom") {
      throw new Error("explosion");
    }
    return { name, args, ok: true };
  }
}

const TOOL: Tool = {
  name: "search-issues",
  description: "Search Jira issues using JQL.",
  inputSchema: {
    type: "object",
    properties: { jql: { type: "string" } },
    required: ["jql"],
  },
} as Tool;

const BOOM_TOOL: Tool = {
  name: "boom",
  description: "Always throws.",
  inputSchema: { type: "object", properties: {} },
} as Tool;

async function makeBridge(tools: Tool[]): Promise<{
  bridge: PythonBridge;
  runtime: CompressorRuntime;
  backend: FakeBackendClient;
  cleanup: () => Promise<void>;
}> {
  const backend = new FakeBackendClient(tools);
  const runtime = new CompressorRuntime({
    backendClient: backend,
    compressionLevel: "max",
    serverName: "jira",
  });
  await runtime.connect();
  const bridge = new PythonBridge(runtime, "jira");
  await bridge.start(0);
  return {
    backend,
    bridge,
    runtime,
    async cleanup() {
      await bridge.close();
      await runtime.disconnect();
    },
  };
}

test("PythonBridge /health returns ok with the server name", async () => {
  const { bridge, cleanup } = await makeBridge([TOOL]);
  try {
    const response = await fetch(`${bridge.url}/health`);
    expect(response.status).toBe(200);
    const body = await response.json();
    expect(body).toEqual({ ok: true, services: ["jira"] });
  } finally {
    await cleanup();
  }
});

test("PythonBridge invokes a tool via /function", async () => {
  const { bridge, backend, cleanup } = await makeBridge([TOOL]);
  try {
    const response = await fetch(`${bridge.url}/function`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        service: "jira",
        function: "search-issues",
        params: { jql: "project = TEST" },
      }),
    });
    expect(response.status).toBe(200);
    const body = await response.json();
    expect(body.success).toBe(true);
    expect(typeof body.data).toBe("string");
    expect(body.data).toMatch(/search-issues/);
    expect(backend.callToolCalls).toEqual([
      { name: "search-issues", args: { jql: "project = TEST" } },
    ]);
  } finally {
    await cleanup();
  }
});

test("PythonBridge rejects calls to a different service", async () => {
  const { bridge, cleanup } = await makeBridge([TOOL]);
  try {
    const response = await fetch(`${bridge.url}/function`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ service: "github", function: "x", params: {} }),
    });
    expect(response.status).toBe(200);
    const body = await response.json();
    expect(body.success).toBe(false);
    expect(body.errorType).toBe("UnknownService");
  } finally {
    await cleanup();
  }
});

test("PythonBridge wraps backend exceptions as failure responses", async () => {
  const { bridge, cleanup } = await makeBridge([BOOM_TOOL]);
  try {
    const response = await fetch(`${bridge.url}/function`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ service: "jira", function: "boom", params: {} }),
    });
    expect(response.status).toBe(200);
    const body = await response.json();
    expect(body.success).toBe(false);
    expect(body.error).toMatch(/explosion/);
  } finally {
    await cleanup();
  }
});

test("PythonBridge rejects malformed requests with structured failures", async () => {
  const { bridge, cleanup } = await makeBridge([TOOL]);
  try {
    const noService = await fetch(`${bridge.url}/function`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ function: "x", params: {} }),
    });
    const noServiceBody = await noService.json();
    expect(noServiceBody.success).toBe(false);
    expect(noServiceBody.errorType).toBe("InvalidArguments");

    const arrayParams = await fetch(`${bridge.url}/function`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ service: "jira", function: "search-issues", params: [] }),
    });
    const arrayParamsBody = await arrayParams.json();
    expect(arrayParamsBody.success).toBe(false);
    expect(arrayParamsBody.error).toMatch(/object/);
  } finally {
    await cleanup();
  }
});

test("PythonBridge returns 404-shaped JSON for unknown paths", async () => {
  const { bridge, cleanup } = await makeBridge([TOOL]);
  try {
    const response = await fetch(`${bridge.url}/something-else`);
    expect(response.status).toBe(404);
    const body = await response.json();
    expect(body.success).toBe(false);
    expect(body.errorType).toBe("NotFound");
  } finally {
    await cleanup();
  }
});

test("PythonBridge.url throws before start and after close", async () => {
  const backend = new FakeBackendClient([TOOL]);
  const runtime = new CompressorRuntime({
    backendClient: backend,
    compressionLevel: "max",
    serverName: "jira",
  });
  await runtime.connect();
  const bridge = new PythonBridge(runtime, "jira");
  expect(() => bridge.url).toThrow(/not listening/);
  await bridge.start(0);
  expect(() => bridge.url).not.toThrow();
  await bridge.close();
  expect(() => bridge.url).toThrow(/not listening/);
  await runtime.disconnect();
});
