import { test, expect } from "vitest";
import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import { CompressorRuntime } from "../src/runtime.js";
import type { BackendToolClient } from "../src/types.js";

class FakeBackendClient implements BackendToolClient {
  connectCalls = 0;
  disconnectCalls = 0;
  listToolsCalls = 0;
  callToolCalls: Array<{ name: string; args: Record<string, unknown> | undefined }> = [];

  constructor(
    private readonly tools: Tool[],
    private readonly result: unknown = { ok: true },
  ) {}

  async connect(): Promise<void> {
    this.connectCalls += 1;
  }

  async disconnect(): Promise<void> {
    this.disconnectCalls += 1;
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

  await runtime.disconnect();
  expect(backendClient.disconnectCalls).toBe(1);
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

test("CompressorRuntime getAiSdkTools returns AI SDK-compatible tool objects", async () => {
  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: "medium",
    serverName: "docs",
  });
  await runtime.connect();

  const tools = await runtime.getAiSdkTools();

  // Should have get_tool_schema and invoke_tool (no list_tools at medium compression)
  expect(Object.keys(tools).sort()).toEqual(["docs_get_tool_schema", "docs_invoke_tool"]);

  // Each tool should have description, parameters (Zod schema), and execute function
  const getSchema = tools.docs_get_tool_schema!;
  expect(getSchema.description).toContain("search_docs");
  expect(getSchema.description).toContain("create_ticket");
  expect(getSchema.parameters).toBeDefined();
  expect(typeof getSchema.execute).toBe("function");

  const invoke = tools.docs_invoke_tool!;
  expect(invoke.description).toContain("Invoke a tool");
  expect(invoke.parameters).toBeDefined();
  expect(typeof invoke.execute).toBe("function");

  // Execute get_tool_schema
  const schemaResult = await getSchema.execute({ tool_name: "search_docs" });
  expect(schemaResult).toContain("search_docs");

  // Execute invoke_tool
  const invokeResult = await invoke.execute({
    tool_name: "search_docs",
    tool_input: { query: "test" },
  });
  expect(invokeResult).toBe(JSON.stringify({ ok: true }, null, 2));
  expect(backendClient.callToolCalls).toEqual([{ name: "search_docs", args: { query: "test" } }]);
});

test("CompressorRuntime getAiSdkTools includes list_tools at max compression", async () => {
  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: "max",
    serverName: "docs",
  });
  await runtime.connect();

  const tools = await runtime.getAiSdkTools();

  expect(Object.keys(tools).sort()).toEqual([
    "docs_get_tool_schema",
    "docs_invoke_tool",
    "docs_list_tools",
  ]);

  const listResult = await tools.docs_list_tools!.execute({});
  expect(JSON.parse(listResult)).toEqual(["create_ticket", "search_docs"]);
});

test("CompressorRuntime getAiSdkTools works without serverName prefix", async () => {
  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: "medium",
  });
  await runtime.connect();

  const tools = await runtime.getAiSdkTools();

  expect(Object.keys(tools).sort()).toEqual(["get_tool_schema", "invoke_tool"]);
  expect(tools.get_tool_schema!.description).toContain("this toolset");
});

test("CompressorRuntime getAiSdkTools returns single help tool in cliMode", async () => {
  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: "medium",
    serverName: "docs",
    cli: { cliMode: true, cliName: "docs", scriptDir: "/tmp/rovodev-test-cli-scripts" },
  });
  await runtime.connect();

  try {
    const tools = await runtime.getAiSdkTools();

    // Should have a single help tool
    expect(Object.keys(tools)).toEqual(["docs_help"]);

    const helpTool = tools.docs_help!;
    // Description should contain CLI help text with subcommand names
    expect(helpTool.description).toContain("docs");
    expect(helpTool.description).toContain("search-docs");
    expect(helpTool.description).toContain("create-ticket");
    expect(helpTool.description).toContain("subcommand");
    // Should NOT contain get_tool_schema or invoke_tool
    expect(Object.keys(tools)).not.toContain("docs_get_tool_schema");
    expect(Object.keys(tools)).not.toContain("docs_invoke_tool");

    // Execute returns the help text
    const result = await helpTool.execute({});
    expect(result).toBe(helpTool.description);
  } finally {
    await runtime.disconnect();
  }
});

test("createBashCommand creates a parent command with subcommands", async () => {
  const { createBashCommand } = await import("../src/bash_commands.js");
  const { Bash } = await import("just-bash");

  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: "medium",
    serverName: "docs",
  });
  await runtime.connect();

  const tools = await runtime.listUncompressedTools();
  const command = createBashCommand(runtime, tools);

  // Should create a single parent command named after the server
  expect(command.name).toBe("docs");

  // Without installPipingHintPlugin the wrapper defaults to TOON.  See
  // just_bash_transform.test.ts for the auto-detect-on-pipe behavior.
  const bash = new Bash({ customCommands: [command] });
  const result = await bash.exec("docs search-docs --query test");
  expect(result.stdout).toBe("ok: true");
  expect(result.exitCode).toBe(0);

  // Explicit --json forces raw JSON.
  const jsonResult = await bash.exec("docs search-docs --query test --json");
  expect(jsonResult.stdout).toBe(JSON.stringify({ ok: true }, null, 2));
  expect(jsonResult.exitCode).toBe(0);

  // Parent --help should list subcommands
  const helpResult = await bash.exec("docs --help");
  expect(helpResult.stdout).toContain("search-docs");
  expect(helpResult.stdout).toContain("create-ticket");
  expect(helpResult.exitCode).toBe(0);

  // Subcommand --help should show tool options
  const subHelpResult = await bash.exec("docs search-docs --help");
  expect(subHelpResult.stdout).toContain("--query");
  expect(subHelpResult.exitCode).toBe(0);

  // Unknown subcommand should error with help
  const errorResult = await bash.exec("docs unknown-sub");
  expect(errorResult.exitCode).toBe(1);
  expect(errorResult.stderr).toContain("unknown subcommand");
});

test("buildBashToolDescription lists top-level commands only (not subcommands)", async () => {
  const { createBashCommand, buildBashToolDescription } = await import("../src/bash_commands.js");

  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: "medium",
    serverName: "docs",
  });
  await runtime.connect();

  const tools = await runtime.listUncompressedTools();
  const command = createBashCommand(runtime, tools);

  const description = buildBashToolDescription([{ serverName: "docs", command, tools }]);

  expect(description).toContain("sandboxed environment");
  expect(description).toContain("custom commands are installed");
  expect(description).toContain("- `docs`");
  expect(description).toContain("TOON");
  // Subcommands belong in the per-server help tools, not the bash description.
  expect(description).not.toContain("search-docs");
  expect(description).not.toContain("create-ticket");
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
