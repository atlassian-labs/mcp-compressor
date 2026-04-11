import { test, expect } from "vitest";
import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import { CompressorRuntime } from "../src/runtime.js";
import type { BackendToolClient } from "../src/types.js";

class FakeBackendClient implements BackendToolClient {
  connectCalls = 0;
  closeCalls = 0;
  listToolsCalls = 0;
  callToolCalls: Array<{ name: string; args: Record<string, unknown> | undefined }> = [];

  constructor(
    private readonly tools: Tool[],
    private readonly result: unknown = { ok: true },
  ) {}

  async connect(): Promise<void> {
    this.connectCalls += 1;
  }

  async close(): Promise<void> {
    this.closeCalls += 1;
  }

  async listTools(): Promise<Tool[]> {
    this.listToolsCalls += 1;
    return this.tools;
  }

  async callTool(name: string, args: Record<string, unknown> | undefined): Promise<unknown> {
    this.callToolCalls.push({ name, args });
    return this.result;
  }
}

const SAMPLE_TOOLS: Tool[] = [
  {
    name: "search_docs",
    description: "Search documentation. Returns matching pages.",
    inputSchema: {
      type: "object",
      properties: {
        query: { type: "string" },
      },
    },
  } as Tool,
  {
    name: "create_ticket",
    description: "Create a ticket.",
    inputSchema: {
      type: "object",
      properties: {
        summary: { type: "string" },
      },
    },
  } as Tool,
];

test("CompressorRuntime exposes wrapper operations in-process", async () => {
  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: "max",
    includeTools: ["search_docs"],
    serverName: "docs",
  });

  await runtime.connect();

  expect(backendClient.connectCalls).toBe(1);
  expect(await runtime.listToolNames()).toEqual(["search_docs"]);
  expect((await runtime.getToolSchema("search_docs")).name).toBe("search_docs");
  expect(await runtime.buildCompressedDescription()).toMatch(/search_docs/);
  expect(await runtime.invokeTool("search_docs", { query: "oauth" })).toBe(
    JSON.stringify({ ok: true }, null, 2),
  );
  expect(backendClient.callToolCalls).toEqual([{ name: "search_docs", args: { query: "oauth" } }]);

  await runtime.close();
  expect(backendClient.closeCalls).toBe(1);
});

test("CompressorRuntime function toolset mirrors wrapper tools", async () => {
  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: "max",
    serverName: "docs",
  });
  await runtime.connect();

  const toolset = runtime.getFunctionToolset();
  expect(Object.keys(toolset).sort()).toEqual([
    "docs_get_tool_schema",
    "docs_invoke_tool",
    "docs_list_tools",
  ]);
  expect(await toolset.docs_get_tool_schema({ tool_name: "search_docs" })).toMatch(/search_docs/);
  expect(await toolset.docs_list_tools()).toMatch(/search_docs/);
  expect(
    await toolset.docs_invoke_tool({ tool_name: "search_docs", tool_input: { query: "mcp" } }),
  ).toBe(JSON.stringify({ ok: true }, null, 2));
});

test("CompressorRuntime treats an empty includeTools list as no include filter", async () => {
  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    includeTools: [],
  });

  await runtime.connect();

  expect(await runtime.listToolNames()).toEqual(["create_ticket", "search_docs"]);
});
