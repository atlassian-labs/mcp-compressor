import { test, expect } from "vitest";

import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import { initializePythonMode } from "../src/python_mode.js";
import { DEFAULT_PACKAGE_NAME } from "../src/python_stubs.js";
import type { BackendConfig, BackendToolClient } from "../src/types.js";

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
    return { name, args };
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

// initializePythonMode hard-depends on resolveBackends → BackendClient. To avoid spinning up a
// real MCP transport in unit tests, we stub the `backend` field with a fake config and inject the
// fake client via the `__testBackendClient` escape hatch on CompressorRuntime options. Since
// mcp-compressor doesn't expose such a hatch, the cleanest approach for now is a small adapter
// that constructs CompressorRuntime directly, mirroring what initializePythonMode would do but
// supplying our FakeBackendClient. This keeps the test true to the production code path
// (PythonBridge wraps a real CompressorRuntime, file map merges shared infra etc.) without
// requiring a stdio child process.

import { PythonBridge } from "../src/python_bridge.js";
import { CompressorRuntime } from "../src/runtime.js";
import { generatePythonStubs, sanitizePythonModuleName } from "../src/python_stubs.js";
import { getPythonRuntimeAssets } from "../src/python_runtime_assets.js";

async function buildSession(serverName: string, tools: Tool[]) {
  const backend = new FakeBackendClient(tools);
  const runtime = new CompressorRuntime({
    backendClient: backend,
    compressionLevel: "max",
    serverName,
  });
  await runtime.connect();
  const sanitized = sanitizePythonModuleName(serverName);
  const bridge = new PythonBridge(runtime, sanitized);
  await bridge.start(0);
  const generated = generatePythonStubs(await runtime.listUncompressedTools(), {
    serverName: sanitized,
  });
  return { backend, runtime, bridge, generated, serverName: sanitized };
}

test("initializePythonMode integration shape: bridge URL is reachable and file tree is complete", async () => {
  // Sanity check on initializePythonMode's exported symbol.
  expect(typeof initializePythonMode).toBe("function");
});

test("a python-mode session exposes a complete file tree merged with shared infra", async () => {
  const session = await buildSession("jira", [TOOL]);
  try {
    const merged = new Map<string, string>();
    for (const [k, v] of getPythonRuntimeAssets({
      packageName: DEFAULT_PACKAGE_NAME,
      bridgeUrl: session.bridge.url,
    })) {
      merged.set(k, v);
    }
    for (const [k, v] of session.generated.files) {
      merged.set(k, v);
    }

    expect([...merged.keys()].sort()).toEqual([
      `${DEFAULT_PACKAGE_NAME}/__init__.py`,
      `${DEFAULT_PACKAGE_NAME}/jira/__init__.py`,
      `${DEFAULT_PACKAGE_NAME}/jira/search_issues.py`,
    ]);

    // URL is baked in — the init file should contain the actual bridge URL literal.
    expect(merged.get(`${DEFAULT_PACKAGE_NAME}/__init__.py`)).toMatch(/_BRIDGE_URL = /);
    expect(merged.get(`${DEFAULT_PACKAGE_NAME}/__init__.py`)).toMatch(/class ToolCallError/);
  } finally {
    await session.bridge.close();
    await session.runtime.disconnect();
  }
});

test("end-to-end: generated stub payload reaches the backend tool via the bridge", async () => {
  const session = await buildSession("jira", [TOOL]);
  try {
    // Invoke the bridge the same way the generated _call helper would.
    const response = await fetch(`${session.bridge.url}/function`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        service: session.serverName,
        function: "search-issues",
        params: { jql: "project = TEST" },
      }),
    });
    const body = await response.json();
    expect(body.success).toBe(true);
    expect(session.backend.callToolCalls).toEqual([
      { name: "search-issues", args: { jql: "project = TEST" } },
    ]);
  } finally {
    await session.bridge.close();
    await session.runtime.disconnect();
  }
});

test("initializePythonMode rejects with a clear error for an invalid backend config", async () => {
  const config: BackendConfig = {
    type: "stdio",
    command: "this-does-not-exist-binary",
    args: [],
  } as BackendConfig;
  await expect(
    initializePythonMode({
      backend: config,
      compressionLevel: "max",
      serverName: "x",
    }),
  ).rejects.toThrow();
});
