import { test, expect } from "vitest";
import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import { CompressorClient } from "../src/client.js";
import { CompressorRuntime } from "../src/runtime.js";
import type { BackendToolClient } from "../src/types.js";

// ---------------------------------------------------------------------------
// Fake backend
// ---------------------------------------------------------------------------

class FakeBackendClient implements BackendToolClient {
  connectCalls = 0;
  disconnectCalls = 0;
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
    return this.tools;
  }

  async callTool(name: string, args: Record<string, unknown> | undefined): Promise<unknown> {
    this.callToolCalls.push({ name, args });
    return this.result;
  }
}

const ALPHA_TOOLS: Tool[] = [
  {
    name: "search_issues",
    description: "Search Jira issues. Returns matching results.",
    inputSchema: {
      type: "object",
      properties: { query: { type: "string" } },
    },
  } as Tool,
  {
    name: "create_issue",
    description: "Create a Jira issue.",
    inputSchema: {
      type: "object",
      properties: { summary: { type: "string" } },
    },
  } as Tool,
];

const BETA_TOOLS: Tool[] = [
  {
    name: "get_page",
    description: "Get a Confluence page.",
    inputSchema: {
      type: "object",
      properties: { page_id: { type: "string" } },
    },
  } as Tool,
];

// ---------------------------------------------------------------------------
// Helper: create a client with fake backends injected
// ---------------------------------------------------------------------------

function createClientWithFakes(
  backends: Array<{ serverName: string; tools: Tool[]; result?: unknown }>,
  options?: {
    mode?: "compressed" | "cli" | "bash";
    compressionLevel?: "low" | "medium" | "high" | "max";
    toonify?: boolean;
    includeTools?: string[];
    excludeTools?: string[];
    bash?: { bash?: import("just-bash").Bash; bashOptions?: Record<string, unknown> };
  },
): { client: CompressorClient; fakes: FakeBackendClient[] } {
  const fakes: FakeBackendClient[] = [];

  // Construct the client by monkey-patching runtimeEntries with fake runtimes
  const client = Object.create(CompressorClient.prototype) as CompressorClient;

  const mode = options?.mode ?? "compressed";

  const runtimeEntries: Array<{ serverName: string; runtime: CompressorRuntime }> = [];
  for (const { serverName, tools, result } of backends) {
    const fake = new FakeBackendClient(tools, result);
    fakes.push(fake);

    const runtime = new CompressorRuntime({
      backendClient: fake,
      compressionLevel: options?.compressionLevel ?? "medium",
      includeTools: options?.includeTools,
      excludeTools: options?.excludeTools,
      serverName,
      toonify: options?.toonify ?? (mode === "cli" || mode === "bash"),
    });

    runtimeEntries.push({ serverName, runtime });
  }

  // Set private fields
  (client as any).runtimeEntries = runtimeEntries;
  (client as any).connected = false;
  (client as any).closed = false;
  (client as any).mode = mode;
  (client as any).cliScripts = null;
  (client as any).bashInstance = null;
  (client as any).bashOwned = false;
  (client as any).options = { bash: options?.bash };

  return { client, fakes };
}

/**
 * Connect a test client, handling bash mode initialization manually since
 * we bypass the constructor's mode initialization path.
 */
async function connectTestClient(
  client: CompressorClient,
  _fakes?: FakeBackendClient[],
): Promise<void> {
  // Manually connect each runtime (bypassing the real connect() which also does mode init)
  for (const runtime of client.runtimes) {
    await runtime.connect();
  }
  (client as any).connected = true;

  // Initialize mode-specific resources
  const mode = (client as any).mode;
  if (mode === "bash") {
    await (client as any).initBashMode();
  }
  // Skip CLI mode init in tests — it requires real HTTP bridges
}

// ---------------------------------------------------------------------------
// Tests: Lifecycle
// ---------------------------------------------------------------------------

test("CompressorClient connect and close lifecycle", async () => {
  const { client, fakes } = createClientWithFakes([
    { serverName: "alpha", tools: ALPHA_TOOLS },
    { serverName: "beta", tools: BETA_TOOLS },
  ]);

  expect(client.isConnected).toBe(false);

  await connectTestClient(client, fakes);
  expect(client.isConnected).toBe(true);
  expect(fakes[0]!.connectCalls).toBe(1);
  expect(fakes[1]!.connectCalls).toBe(1);

  await client.close();
  expect(client.isConnected).toBe(false);
  expect(fakes[0]!.disconnectCalls).toBe(1);
  expect(fakes[1]!.disconnectCalls).toBe(1);
});

test("CompressorClient throws when using getTools before connect", async () => {
  const { client } = createClientWithFakes([{ serverName: "alpha", tools: ALPHA_TOOLS }]);

  await expect(client.getTools()).rejects.toThrow("not connected");
});

test("CompressorClient throws when reconnecting after close", async () => {
  const { client, fakes } = createClientWithFakes([{ serverName: "alpha", tools: ALPHA_TOOLS }]);

  await connectTestClient(client, fakes);
  await client.close();

  await expect(client.connect()).rejects.toThrow("closed");
});

// ---------------------------------------------------------------------------
// Tests: Compressed mode (default)
// ---------------------------------------------------------------------------

test("compressed mode: getTools returns merged compressed tools for multiple servers", async () => {
  const { client, fakes } = createClientWithFakes([
    { serverName: "jira", tools: ALPHA_TOOLS },
    { serverName: "confluence", tools: BETA_TOOLS },
  ]);
  await connectTestClient(client, fakes);

  const tools = await client.getTools();

  expect(Object.keys(tools).sort()).toEqual([
    "confluence_get_tool_schema",
    "confluence_invoke_tool",
    "jira_get_tool_schema",
    "jira_invoke_tool",
  ]);

  // Jira get_tool_schema should describe jira tools
  expect(tools.jira_get_tool_schema!.description).toContain("search_issues");
  expect(tools.jira_get_tool_schema!.description).toContain("create_issue");

  // Confluence get_tool_schema should describe confluence tools
  expect(tools.confluence_get_tool_schema!.description).toContain("get_page");

  await client.close();
});

test("compressed mode: includes list_tools at max compression", async () => {
  const { client, fakes } = createClientWithFakes(
    [{ serverName: "jira", tools: ALPHA_TOOLS }],
    { compressionLevel: "max" },
  );
  await connectTestClient(client, fakes);

  const tools = await client.getTools();

  expect(Object.keys(tools).sort()).toEqual([
    "jira_get_tool_schema",
    "jira_invoke_tool",
    "jira_list_tools",
  ]);

  const listResult = await tools.jira_list_tools!.execute({});
  expect(JSON.parse(listResult)).toEqual(["create_issue", "search_issues"]);

  await client.close();
});

test("compressed mode: invoke_tool calls backend", async () => {
  const { client, fakes } = createClientWithFakes([
    { serverName: "jira", tools: ALPHA_TOOLS, result: { issues: [] } },
  ]);
  await connectTestClient(client, fakes);

  const tools = await client.getTools();
  const result = await tools.jira_invoke_tool!.execute({
    tool_name: "search_issues",
    tool_input: { query: "bug" },
  });

  expect(result).toContain("issues");
  expect(fakes[0]!.callToolCalls).toEqual([
    { name: "search_issues", args: { query: "bug" } },
  ]);

  await client.close();
});

// ---------------------------------------------------------------------------
// Tests: Bash mode — getTools returns bash + help tools
// ---------------------------------------------------------------------------

test("bash mode: getTools returns bash tool + per-server help tools", async () => {
  const { client, fakes } = createClientWithFakes(
    [
      { serverName: "jira", tools: ALPHA_TOOLS },
      { serverName: "confluence", tools: BETA_TOOLS },
    ],
    { mode: "bash" },
  );
  await connectTestClient(client, fakes);

  const tools = await client.getTools();

  expect(Object.keys(tools).sort()).toEqual([
    "bash",
    "confluence_help",
    "jira_help",
  ]);

  // bash tool has a simple description
  expect(tools.bash!.description).toContain("bash commands");
  expect(tools.bash!.description).toContain("help tools");

  // help tools describe the available CLI subcommands
  expect(tools.jira_help!.description).toContain("search-issues");
  expect(tools.jira_help!.description).toContain("create-issue");
  expect(tools.confluence_help!.description).toContain("get-page");

  await client.close();
});

test("bash mode: bash tool executes server commands", async () => {
  const { client, fakes } = createClientWithFakes(
    [{ serverName: "jira", tools: ALPHA_TOOLS, result: { issues: ["JIRA-1"] } }],
    { mode: "bash" },
  );
  await connectTestClient(client, fakes);

  const tools = await client.getTools();
  const result = await tools.bash!.execute({ command: "jira search-issues --query test" });

  expect(result).toContain("JIRA-1");
  expect(fakes[0]!.callToolCalls).toEqual([
    { name: "search_issues", args: { query: "test" } },
  ]);

  await client.close();
});

test("bash mode: help tool returns its description", async () => {
  const { client, fakes } = createClientWithFakes(
    [{ serverName: "jira", tools: ALPHA_TOOLS }],
    { mode: "bash" },
  );
  await connectTestClient(client, fakes);

  const tools = await client.getTools();
  const helpResult = await tools.jira_help!.execute({});

  // Help tool should return its own description
  expect(helpResult).toContain("search-issues");
  expect(helpResult).toContain("create-issue");
  expect(helpResult).toBe(tools.jira_help!.description);

  await client.close();
});

test("bash mode: with pre-existing Bash instance", async () => {
  const { Bash, defineCommand } = await import("just-bash");

  const myCommand = defineCommand("hello", async () => ({
    stdout: "Hello, world!",
    stderr: "",
    exitCode: 0,
  }));
  const existingBash = new Bash({ customCommands: [myCommand] });

  const { client, fakes } = createClientWithFakes(
    [{ serverName: "jira", tools: ALPHA_TOOLS }],
    { mode: "bash", bash: { bash: existingBash } },
  );
  await connectTestClient(client, fakes);

  // Should reuse the existing bash instance
  expect(client.bash).toBe(existingBash);

  // Existing command should still work
  const helloResult = await existingBash.exec("hello");
  expect(helloResult.stdout).toBe("Hello, world!");

  // MCP command should also work
  const jiraHelp = await existingBash.exec("jira --help");
  expect(jiraHelp.exitCode).toBe(0);
  expect(jiraHelp.stdout).toContain("search-issues");

  await client.close();
});

test("bash mode: client.bash accessor returns the Bash instance", async () => {
  const { client, fakes } = createClientWithFakes(
    [{ serverName: "jira", tools: ALPHA_TOOLS }],
    { mode: "bash" },
  );
  await connectTestClient(client, fakes);

  expect(client.bash).not.toBeNull();
  const result = await client.bash!.exec("jira --help");
  expect(result.exitCode).toBe(0);

  await client.close();
});

test("bash mode: client.bash is null in compressed mode", async () => {
  const { client, fakes } = createClientWithFakes(
    [{ serverName: "jira", tools: ALPHA_TOOLS }],
  );
  await connectTestClient(client, fakes);

  expect(client.bash).toBeNull();

  await client.close();
});

// ---------------------------------------------------------------------------
// Tests: Help tool description is identical between cli and bash modes
// ---------------------------------------------------------------------------

test("cli and bash modes produce identical help tool descriptions", async () => {
  // Create two clients with identical server configs but different modes
  const { client: bashClient, fakes: bashFakes } = createClientWithFakes(
    [{ serverName: "jira", tools: ALPHA_TOOLS }],
    { mode: "bash" },
  );
  await connectTestClient(bashClient, bashFakes);

  const bashTools = await bashClient.getTools();

  // For CLI mode, we can't fully test without a real bridge, but we can test
  // the help tool builder directly with the same parameters
  const { buildHelpToolDescription, sanitizeCliName } = await import("../src/cli_tools.js");
  const cliName = sanitizeCliName("jira");
  const cliDescription = buildHelpToolDescription(
    cliName,
    "the jira toolset",
    ALPHA_TOOLS,
    true, // onPath = true, same as bash mode
  );

  expect(bashTools.jira_help!.description).toBe(cliDescription);

  await bashClient.close();
});

// ---------------------------------------------------------------------------
// Tests: Escape hatches
// ---------------------------------------------------------------------------

test("getRuntime returns the correct runtime by server name", async () => {
  const { client, fakes } = createClientWithFakes([
    { serverName: "jira", tools: ALPHA_TOOLS },
    { serverName: "confluence", tools: BETA_TOOLS },
  ]);
  await connectTestClient(client, fakes);

  const jiraRuntime = client.getRuntime("jira");
  expect(jiraRuntime.serverName).toBe("jira");
  expect(await jiraRuntime.listToolNames()).toEqual(["create_issue", "search_issues"]);

  expect(() => client.getRuntime("unknown")).toThrow("No runtime found");

  await client.close();
});

test("serverNames and runtimes accessors", async () => {
  const { client } = createClientWithFakes([
    { serverName: "jira", tools: ALPHA_TOOLS },
    { serverName: "confluence", tools: BETA_TOOLS },
  ]);

  expect(client.serverNames).toEqual(["jira", "confluence"]);
  expect(client.runtimes).toHaveLength(2);
});

// ---------------------------------------------------------------------------
// Tests: Server resolution (via constructor)
// ---------------------------------------------------------------------------

test("resolveServersMap handles MCP config JSON string", () => {
  const client = new CompressorClient({
    servers: JSON.stringify({
      mcpServers: {
        alpha: { command: "echo", args: ["hello"] },
        beta: { command: "echo", args: ["world"] },
      },
    }),
  });

  expect(client.serverNames).toEqual(["alpha", "beta"]);
});

test("resolveServersMap handles ServersMap with mixed config formats", () => {
  const client = new CompressorClient({
    servers: {
      jira: { command: "node", args: ["jira.js"] },
      github: { type: "http" as const, url: "https://github.example.com" },
    },
  });

  expect(client.serverNames).toEqual(["jira", "github"]);
});

test("resolveServersMap handles single BackendConfig", () => {
  const client = new CompressorClient({
    servers: { type: "stdio" as const, command: "echo" },
  });

  expect(client.serverNames).toEqual(["default"]);
});

test("resolveServersMap handles URL string", () => {
  const client = new CompressorClient({
    servers: "https://my-mcp-server.example.com",
  });

  expect(client.serverNames).toEqual(["default"]);
});

test("resolveServersMap throws on empty servers map", () => {
  expect(() => new CompressorClient({ servers: {} })).toThrow("at least one server");
});

// ---------------------------------------------------------------------------
// Tests: Mode defaults
// ---------------------------------------------------------------------------

test("default mode is compressed", async () => {
  const { client, fakes } = createClientWithFakes([
    { serverName: "jira", tools: ALPHA_TOOLS },
  ]);
  await connectTestClient(client, fakes);

  const tools = await client.getTools();
  // Should have compressed tools, not help/bash tools
  expect(Object.keys(tools).sort()).toEqual([
    "jira_get_tool_schema",
    "jira_invoke_tool",
  ]);

  await client.close();
});
