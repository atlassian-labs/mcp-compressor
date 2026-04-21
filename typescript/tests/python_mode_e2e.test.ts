import { test, expect } from "vitest";
import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import { PythonBridge } from "../src/python_bridge.js";
import { CompressorRuntime } from "../src/runtime.js";
import { generatePythonStubs } from "../src/python_stubs.js";
import { getPythonRuntimeAssets } from "../src/python_runtime_assets.js";
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
    return { name, args, ok: true };
  }
}

const TOOL: Tool = {
  name: "search-issues",
  description: "Search Jira issues using JQL.",
  inputSchema: {
    type: "object",
    properties: {
      jql: { type: "string" },
      max_results: { type: "integer" },
    },
    required: ["jql"],
  },
} as Tool;

async function pythonAvailable(): Promise<string | null> {
  for (const bin of ["python3", "python"]) {
    const ok = await new Promise<boolean>((resolve) => {
      const child = spawn(bin, ["--version"], { stdio: "ignore" });
      child.on("error", () => resolve(false));
      child.on("exit", (code) => resolve(code === 0));
    });
    if (ok) return bin;
  }
  return null;
}

async function runPython(
  bin: string,
  cwd: string,
  env: NodeJS.ProcessEnv,
  code: string,
): Promise<{ stdout: string; stderr: string; code: number }> {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, ["-c", code], { cwd, env, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk: Buffer) => (stdout += chunk.toString("utf8")));
    child.stderr.on("data", (chunk: Buffer) => (stderr += chunk.toString("utf8")));
    child.on("error", reject);
    child.on("exit", (code) => resolve({ stdout, stderr, code: code ?? -1 }));
  });
}

test("Python interpreter can import generated stubs and call the bridge", async () => {
  const bin = await pythonAvailable();
  if (bin === null) {
    // No interpreter available; skip without failing the suite.
    console.warn("Skipping python-end-to-end test: no python on $PATH");
    return;
  }

  const backend = new FakeBackendClient([TOOL]);
  const runtime = new CompressorRuntime({
    backendClient: backend,
    compressionLevel: "max",
    serverName: "jira",
  });
  await runtime.connect();
  const bridge = new PythonBridge(runtime, "jira");
  await bridge.start(0);

  const root = await fs.mkdtemp(path.join(os.tmpdir(), "mcp-compressor-pythonmode-"));
  try {
    // Materialise the file tree.
    const generated = generatePythonStubs([TOOL], { serverName: "jira" });
    const allFiles = new Map<string, string>();
    for (const [k, v] of getPythonRuntimeAssets({
      packageName: "tools",
      serverName: "jira",
      bridgeUrl: bridge.url,
    })) {
      allFiles.set(k, v);
    }
    for (const [k, v] of generated.files) {
      allFiles.set(k, v);
    }
    for (const [rel, content] of allFiles) {
      const abs = path.join(root, rel);
      await fs.mkdir(path.dirname(abs), { recursive: true });
      await fs.writeFile(abs, content, "utf8");
    }

    // Sanity check that the runtime asset has the bridge URL baked in.
    expect(allFiles.get("tools/jira/_call.py")).toContain(bridge.url);
    // No top-level __init__.py — `tools` is a PEP 420 namespace package.
    expect(allFiles.has("tools/__init__.py")).toBe(false);

    // Drive the generated stub through Python, asserting:
    //  - import succeeds
    //  - HTTP POST to the bridge succeeds
    //  - returned data is the bridge's stringified tool output
    const driver = `
import asyncio, sys, json
sys.path.insert(0, ${JSON.stringify(root)})
from tools.jira import search_issues
result = asyncio.run(search_issues(jql="project = TEST", max_results=5))
print("RESULT:", json.dumps(result))
`;
    const env = {
      ...process.env,
      // No env var needed — bridge URL is baked into the generated __init__.py.
    };
    const { stdout, stderr, code } = await runPython(bin, root, env, driver);
    expect(stderr).toBe("");
    expect(code).toBe(0);
    expect(stdout).toMatch(/RESULT:/);

    // Backend should have been invoked exactly once with the merged payload.
    expect(backend.callToolCalls).toEqual([
      { name: "search-issues", args: { jql: "project = TEST", max_results: 5 } },
    ]);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
    await bridge.close();
    await runtime.disconnect();
  }
}, 15_000);
