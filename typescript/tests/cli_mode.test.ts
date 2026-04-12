import { test, expect } from "vitest";
import fs from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";

import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import { CliBridge } from "../src/cli_bridge.js";
import { generateCliScript, removeCliScript, removeCliScriptEntry } from "../src/cli_script.js";
import { parseArgvToToolInput, toolNameToSubcommand } from "../src/cli_tools.js";
import { parseCliArgs } from "../src/cli.js";
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
    return { name, args };
  }
}

const TOOL: Tool = {
  name: "searchConfluence",
  description: "Search confluence content. Returns matching documents.",
  inputSchema: {
    type: "object",
    properties: {
      query: { type: "string" },
      limit: { type: "integer" },
      dry_run: { type: "boolean" },
    },
    required: ["query"],
  },
} as Tool;

test("toolNameToSubcommand and parseArgvToToolInput support CLI flags", () => {
  expect(toolNameToSubcommand("searchConfluence")).toBe("search-confluence");
  expect(parseArgvToToolInput(["--query", "oauth", "--limit", "3", "--dry-run"], TOOL)).toEqual({
    query: "oauth",
    limit: 3,
    dry_run: true,
  });
  expect(parseArgvToToolInput(["--json", '{"query":"oauth"}'], TOOL)).toEqual({ query: "oauth" });
});

test("CliBridge serves shell-friendly help and invokes tools", async () => {
  const backendClient = new FakeBackendClient([TOOL]);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: "max",
    serverName: "atlassian",
  });
  await runtime.connect();

  const bridge = new CliBridge(runtime, "atlassian");
  await bridge.start(0);

  const topLevelHelp = await fetch(`${bridge.url}/help`);
  expect(topLevelHelp.status).toBe(200);
  expect(await topLevelHelp.text()).toMatch(/search-confluence/);

  const toolHelp = await fetch(`${bridge.url}/tools/search-confluence/help`);
  expect(toolHelp.status).toBe(200);
  expect(await toolHelp.text()).toMatch(/--query/);

  const form = new URLSearchParams();
  form.append("argv", "--query");
  form.append("argv", "oauth");
  form.append("argv", "--limit");
  form.append("argv", "2");
  const invokeResponse = await fetch(`${bridge.url}/tools/search-confluence`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded;charset=UTF-8" },
    body: form,
  });
  expect(invokeResponse.status).toBe(200);
  expect(await invokeResponse.text()).toMatch(/searchConfluence/);
  expect(backendClient.callToolCalls).toEqual([
    { name: "searchConfluence", args: { query: "oauth", limit: 2 } },
  ]);

  const invalidResponse = await fetch(`${bridge.url}/tools/search-confluence`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded;charset=UTF-8" },
    body: new URLSearchParams([["argv", "hello"]]),
  });
  expect(invalidResponse.status).toBe(400);
  const invalidText = await invalidResponse.text();
  expect(invalidText).toMatch(/Unknown option: hello/);
  expect(invalidText).toMatch(/Usage: atlassian search-confluence \[options\]/);
  expect(invalidText).toMatch(/--query/);

  await bridge.close();
  await runtime.disconnect();
});

test("generateCliScript writes a launcher script", async () => {
  const server = await startHealthServer();
  const scriptDir = await fs.mkdtemp(path.join(os.tmpdir(), "mcp-compressor-cli-mode-"));
  const cliName = "atlassian";
  try {
    const generated = await generateCliScript(cliName, server.port, 12345, scriptDir);
    expect(generated.scriptPath).toBe(
      path.join(scriptDir, process.platform === "win32" ? `${cliName}.cmd` : cliName),
    );
    const content = await fs.readFile(generated.scriptPath, "utf8");
    expect(content).toMatch(/BRIDGES_JSON=/);
    if (process.platform === "win32") {
      expect(content).toMatch(/Get-Process -Id \$PID/);
      expect(content).toMatch(/ContainsKey\(\$proc.Id\)/);
    } else {
      expect(content).toMatch(/^#!\/usr\/bin\/env bash/m);
      expect(content).toMatch(/declare -A BRIDGES/);
      expect(content).toMatch(/ps -o ppid=/);
      expect(content).toMatch(/curl -sS -o/);
    }
    expect(content).toMatch(/mcp-compressor ts cli-mode script/);
  } finally {
    await stopHealthServer(server);
  }
});

test("same-name CLI sessions share a script registry and remove only their own entry", async () => {
  const server1 = await startHealthServer();
  const server2 = await startHealthServer();
  const scriptDir = await fs.mkdtemp(path.join(os.tmpdir(), "mcp-compressor-cli-mode-shared-"));
  const cliName = "atlassian";
  const sessionPid1 = 11111;
  const sessionPid2 = 22222;
  const scriptPath = path.join(
    scriptDir,
    process.platform === "win32" ? `${cliName}.cmd` : cliName,
  );

  try {
    await generateCliScript(cliName, server1.port, sessionPid1, scriptDir);
    await generateCliScript(cliName, server2.port, sessionPid2, scriptDir);

    let content = await fs.readFile(scriptPath, "utf8");
    expect(content).toMatch(new RegExp(`BRIDGES_JSON=.*"${sessionPid1}"`));
    expect(content).toMatch(new RegExp(`BRIDGES_JSON=.*"${sessionPid2}"`));
    expect(content).toMatch(new RegExp(server1.url.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
    expect(content).toMatch(new RegExp(server2.url.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));

    await removeCliScriptEntry(cliName, sessionPid1, scriptDir);
    content = await fs.readFile(scriptPath, "utf8");
    expect(content).not.toMatch(new RegExp(`BRIDGES_JSON=.*"${sessionPid1}"`));
    expect(content).toMatch(new RegExp(`BRIDGES_JSON=.*"${sessionPid2}"`));

    await removeCliScriptEntry(cliName, sessionPid2, scriptDir);
    await expect(() => fs.stat(scriptPath)).rejects.toThrow();
  } finally {
    await stopHealthServer(server1);
    await stopHealthServer(server2);
    await removeCliScript(scriptPath);
  }
});

async function startHealthServer(): Promise<{ port: number; server: http.Server; url: string }> {
  const server = http.createServer((request, response) => {
    if (request.url === "/health") {
      response.statusCode = 200;
      response.end("ok");
      return;
    }
    response.statusCode = 404;
    response.end("not found");
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("failed to start test health server");
  }
  return { port: address.port, server, url: `http://127.0.0.1:${address.port}` };
}

async function stopHealthServer(handle: { server: http.Server }): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    handle.server.close((error) => {
      if (error) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}

test("parseCliArgs supports flag-first CLI mode backend JSON", () => {
  const parsed = parseCliArgs(["--cli-mode", '{"mcpServers":{"alpha":{"command":"uvx"}}}']);
  expect(parsed.cliMode).toBe(true);
  expect(parsed.toonify).toBe(true);
  expect(parsed.backend).toBe('{"mcpServers":{"alpha":{"command":"uvx"}}}');
});

test("parseCliArgs supports backend-first CLI mode backend JSON", () => {
  const parsed = parseCliArgs(['{"mcpServers":{"alpha":{"command":"uvx"}}}', "--cli-mode"]);
  expect(parsed.cliMode).toBe(true);
  expect(parsed.backend).toBe('{"mcpServers":{"alpha":{"command":"uvx"}}}');
});

test("parseCliArgs supports separator for stdio backend commands", () => {
  const parsed = parseCliArgs([
    "--cli-mode",
    "--server-name",
    "fetch",
    "--",
    "uvx",
    "mcp-server-fetch",
  ]);
  expect(parsed.cliMode).toBe(true);
  expect(parsed.serverName).toBe("fetch");
  expect(parsed.backend).toEqual({ type: "stdio", command: "uvx", args: ["mcp-server-fetch"] });
});
