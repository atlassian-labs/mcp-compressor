import { mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { request } from "node:http";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { tmpdir } from "node:os";
import { Bash } from "just-bash";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  CompressorClient,
  compressTools,
  installJustBashCommands,
  interpolateRecord,
  interpolateString,
  normalizeServers,
  toAISDKTools,
  toMastraTools,
  transformToolsForCliMode,
  transformToolsForCodeMode,
  transformToolsForJustBash,
  type BackendConfig,
  type JsonConfigServerEntry,
} from "../src/index.js";

import {
  compressToolListing,
  formatToolSchemaResponse,
  clearOAuthCredentials,
  generateClientArtifacts,
  listOAuthCredentials,
  rememberOAuthBackend,
  startCompressedSession,
  startCompressedSessionFromMcpConfig,
  parseMcpConfig,
  parseToolArgv,
  type ToolSpec,
} from "../src/rust_core.js";

function invokeProxy(
  bridgeUrl: string,
  token: string,
  tool: string,
  toolName: string,
  toolInput: Record<string, unknown>,
): Promise<string> {
  const body = JSON.stringify({
    tool,
    input: {
      tool_name: toolName,
      tool_input: toolInput,
    },
  });
  const url = new URL(`${bridgeUrl}/exec`);
  return new Promise((resolve, reject) => {
    const req = request(
      {
        method: "POST",
        hostname: url.hostname,
        port: url.port,
        path: url.pathname,
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
          "Content-Length": Buffer.byteLength(body),
        },
      },
      (res) => {
        let data = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => {
          data += chunk;
        });
        res.on("end", () => {
          if (res.statusCode && res.statusCode >= 200 && res.statusCode < 300) {
            resolve(data);
          } else {
            reject(new Error(`proxy returned ${res.statusCode}: ${data}`));
          }
        });
      },
    );
    req.on("error", reject);
    req.write(body);
    req.end();
  });
}

function fixturePath(name: string): string {
  return join(process.cwd(), "..", "crates", "mcp-compressor-core", "tests", "fixtures", name);
}

function alphaBackend() {
  return {
    name: "alpha",
    commandOrUrl: process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python"),
    args: [fixturePath("alpha_server.py")],
  };
}

function betaBackend() {
  return {
    name: "beta",
    commandOrUrl: process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python"),
    args: [fixturePath("beta_server.py")],
  };
}

async function startRemoteAlphaUpstream(): Promise<{
  url: string;
  child: ChildProcessWithoutNullStreams;
}> {
  const root = join(process.cwd(), "..");
  const binary = join(
    root,
    "target",
    "debug",
    process.platform === "win32" ? "mcp-compressor.exe" : "mcp-compressor",
  );
  const child = spawn(
    binary,
    [
      "--compression",
      "max",
      "--server-name",
      "alpha",
      "--transport",
      "streamable-http",
      "--port",
      "0",
      "--",
      process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python"),
      fixturePath("alpha_server.py"),
    ],
    {
      cwd: root,
      env: {
        ...process.env,
        PYTHON: process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python"),
      },
    },
  );

  const url = await new Promise<string>((resolve, reject) => {
    let stderr = "";
    let stdout = "";
    const timeout = setTimeout(() => {
      reject(
        new Error(
          `timed out waiting for streamable HTTP upstream URL\nstdout:\n${stdout}\nstderr:\n${stderr}`,
        ),
      );
    }, 60_000);
    const maybeResolveUrl = () => {
      const match = /listening on (http:\/\/127\.0\.0\.1:\d+\/mcp)/.exec(`${stdout}\n${stderr}`);
      if (match) {
        clearTimeout(timeout);
        resolve(match[1]!);
      }
    };
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += String(chunk);
      maybeResolveUrl();
    });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => {
      stderr += String(chunk);
      maybeResolveUrl();
    });
    child.on("error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    child.on("exit", (code) => {
      clearTimeout(timeout);
      reject(new Error(`streamable HTTP upstream exited before ready: ${code}`));
    });
  });

  return { url, child };
}

async function startRotatingAuthProxy(
  targetUrl: string,
  expectedStart = 1,
  allowInitialRepeats = 0,
  allowAnyRepeats = 0,
): Promise<{
  url: string;
  child: ChildProcessWithoutNullStreams;
  stderr: () => string;
}> {
  const child = spawn(
    process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python"),
    [fixturePath("rotating_auth_proxy.py")],
    {
      cwd: join(process.cwd(), ".."),
      env: {
        ...process.env,
        MCP_COMPRESSOR_AUTH_PROXY_TARGET: targetUrl,
        MCP_COMPRESSOR_AUTH_PROXY_EXPECTED_START: String(expectedStart),
        MCP_COMPRESSOR_AUTH_PROXY_ALLOW_INITIAL_REPEATS: String(allowInitialRepeats),
        MCP_COMPRESSOR_AUTH_PROXY_ALLOW_ANY_REPEATS: String(allowAnyRepeats),
      },
    },
  );

  let stderr = "";
  const url = await new Promise<string>((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error(`timed out waiting for rotating auth proxy URL\nstderr:\n${stderr}`));
    }, 30_000);
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => {
      stderr += String(chunk);
      const match = /AUTH_PROXY_URL=(http:\/\/127\.0\.0\.1:\d+)/.exec(String(chunk));
      if (match) {
        clearTimeout(timeout);
        resolve(match[1]!);
      }
    });
    child.on("error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    child.on("exit", (code) => {
      clearTimeout(timeout);
      reject(new Error(`rotating auth proxy exited before ready: ${code}`));
    });
  });

  return { url, child, stderr: () => stderr };
}

async function stopChild(child: ChildProcessWithoutNullStreams): Promise<void> {
  if (child.killed || child.exitCode !== null) {
    return;
  }
  await new Promise<void>((resolve) => {
    child.once("exit", () => resolve());
    child.kill("SIGTERM");
    setTimeout(() => {
      if (!child.killed) {
        child.kill("SIGKILL");
      }
      resolve();
    }, 2_000).unref();
  });
}

const sampleTool: ToolSpec = {
  name: "echo",
  description: "Echo a value.",
  inputSchema: {
    type: "object",
    properties: { message: { type: "string" } },
    required: ["message"],
  },
};

describe("Public TypeScript SDK workflow", () => {
  it("supports schema lookup with multi-server disambiguation", async () => {
    const client = new CompressorClient({
      servers: {
        alpha: {
          command: process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python"),
          args: [fixturePath("alpha_server.py")],
        },
        beta: {
          command: process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python"),
          args: [fixturePath("beta_server.py")],
        },
      },
      compressionLevel: "max",
    });

    const proxy = await client.connect();
    try {
      expect(proxy.schema("echo", { server: "alpha" }).properties).toHaveProperty("message");
      expect(() => proxy.schema("echo")).toThrow(/Multiple backend tools/);
    } finally {
      proxy.close();
      client.close();
    }
  });

  it("refreshes authProvider headers for each remote backend request", async () => {
    const upstream = await startRemoteAlphaUpstream();
    const authProxy = await startRotatingAuthProxy(upstream.url, 1, 10, 10);
    let calls = 0;
    const client = new CompressorClient({
      servers: {
        remote: {
          url: authProxy.url,
          authProvider: async () => {
            calls += 1;
            return { Authorization: `Bearer token-${calls}` };
          },
        },
      },
      compressionLevel: "max",
    });

    try {
      const proxy = await client.connect();
      try {
        const first = await proxy.invokeWrapper("remote_invoke_tool", {
          tool_name: "alpha_invoke_tool",
          tool_input: { tool_name: "echo", tool_input: { message: "one" } },
        });
        const second = await proxy.invokeWrapper("remote_invoke_tool", {
          tool_name: "alpha_invoke_tool",
          tool_input: { tool_name: "echo", tool_input: { message: "two" } },
        });
        expect(first.text).toBe("alpha:one");
        expect(second.text).toBe("alpha:two");
      } finally {
        proxy.close();
        await client.close();
      }
      expect(calls).toBeGreaterThanOrEqual(2);
    } catch (error) {
      throw new Error(
        `${error instanceof Error ? error.message : String(error)}\nAuth proxy stderr:\n${authProxy.stderr()}`,
      );
    } finally {
      await stopChild(authProxy.child);
      await stopChild(upstream.child);
    }
  });

  it("matches the documented CompressorClient quickstart", async () => {
    const client = new CompressorClient({
      servers: {
        alpha: {
          command: process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python"),
          args: [fixturePath("alpha_server.py")],
        },
      },
      compressionLevel: "max",
    });

    const proxy = await client.connect();
    try {
      expect(proxy.tools.map((tool) => tool.name)).toContain("alpha_get_tool_schema");
      expect(proxy.tools.map((tool) => tool.name)).toContain("alpha_invoke_tool");
      const schema = proxy.schema("echo");
      expect(schema.properties).toHaveProperty("message");
      const response = await proxy.invoke("echo", { message: "public-ts" });
      expect(response).toBe("alpha:public-ts");

      const executableTools = proxy.toExecutableTools();
      await expect(
        executableTools.alpha_invoke_tool?.execute({
          tool_name: "echo",
          tool_input: { message: "executable" },
        }),
      ).resolves.toBe("alpha:executable");

      const mastraTools = toMastraTools(executableTools);
      await expect(
        mastraTools.alpha_invoke_tool?.execute({
          tool_name: "echo",
          tool_input: { message: "mastra" },
        }),
      ).resolves.toBe("alpha:mastra");

      const aiTools = toAISDKTools(executableTools, {
        tool: (definition) => ({ ...definition, wrapped: true }),
      });
      expect(aiTools.alpha_invoke_tool).toMatchObject({ wrapped: true });
    } finally {
      proxy.close();
      client.close();
    }
  });
});

describe("local TypeScript tool compression", () => {
  it("compresses local tools into schema and invoke wrappers", async () => {
    const tools = compressTools(
      {
        weather: {
          description: "Get weather for a city.\n\nIncludes current temperature.",
          inputSchema: {
            type: "object",
            properties: { city: { type: "string" } },
            required: ["city"],
          },
          execute: async (input) => ({ city: String(input.city), temperature: 72 }),
        },
      },
      { compressionLevel: "medium" },
    );

    expect(Object.keys(tools).sort()).toEqual(["get_tool_schema", "invoke_tool"]);
    expect(tools.get_tool_schema?.description).toContain("weather(city)");
    expect(tools.get_tool_schema?.description).toContain("Get weather for a city");
    expect(tools.get_tool_schema?.description).not.toContain("Includes current temperature.");

    await expect(tools.get_tool_schema?.execute({ tool_name: "weather" })).resolves.toContain(
      "temperature",
    );
    await expect(
      tools.invoke_tool?.execute({ tool_name: "weather", tool_input: { city: "Sydney" } }),
    ).resolves.toBe('{"city":"Sydney","temperature":72}');
  });

  it("supports max compression, prefixes, and AI SDK adapters", async () => {
    const tools = compressTools(
      {
        echo: {
          description: "Echo a message.",
          inputSchema: {
            type: "object",
            properties: { message: { type: "string" } },
            required: ["message"],
          },
          execute: (input) => `echo:${String(input.message)}`,
        },
      },
      { compressionLevel: "max", namePrefix: "local" },
    );

    expect(Object.keys(tools).sort()).toEqual([
      "local_get_tool_schema",
      "local_invoke_tool",
      "local_list_tools",
    ]);
    await expect(tools.local_list_tools?.execute()).resolves.toBe("");
    await expect(
      tools.local_invoke_tool?.execute({ tool_name: "echo", tool_input: { message: "hi" } }),
    ).resolves.toBe("echo:hi");

    const aiTools = toAISDKTools(tools, { tool: (definition) => ({ ...definition, ai: true }) });
    expect(aiTools.local_invoke_tool).toMatchObject({ ai: true });
  });

  it("accepts schema adapters for non-JSON schema objects", async () => {
    const tools = compressTools(
      {
        adapted: {
          description: "Adapted schema tool.",
          inputSchema: "schema-token",
          execute: (input) => String(input.value),
        },
      },
      {
        schemaAdapter: () => ({
          type: "object",
          properties: { value: { type: "string" } },
          required: ["value"],
        }),
      },
    );

    await expect(
      tools.invoke_tool?.execute({ tool_name: "adapted", tool_input: { value: "ok" } }),
    ).resolves.toBe("ok");
  });

  it("reports missing tools clearly", async () => {
    const tools = compressTools({});
    await expect(tools.get_tool_schema?.execute({ tool_name: "missing" })).rejects.toThrow(
      "Tool not found: missing",
    );
    await expect(
      tools.invoke_tool?.execute({ tool_name: "missing", tool_input: {} }),
    ).rejects.toThrow("Tool not found: missing");
  });

  it("transforms executable tools into direct Just Bash commands plus help tools", async () => {
    const bash = new Bash({ customCommands: [] });
    const result = transformToolsForJustBash(
      {
        echo: {
          name: "echo",
          description: "Echo a message.",
          inputSchema: {
            type: "object",
            properties: { message: { type: "string" } },
            required: ["message"],
          },
          execute: async (input = {}): Promise<unknown> => ({
            rows: [{ message: String(input.message), ok: true }],
          }),
        },
      },
      { bash, serverName: "alpha" },
    );

    expect(Object.keys(result.tools)).toEqual(["alpha_help"]);
    expect(result.registrations.map((registration) => registration.commandName)).toEqual([
      "alpha_echo",
    ]);
    const commandResult = await bash.exec("alpha_echo --message bash");
    expect(commandResult.exitCode).toBe(0);
    expect(commandResult.stdout).toContain("rows[1]{message,ok}:");
    expect(commandResult.stdout).toContain("bash,true");
    expect(result.tools.alpha_help?.description).toContain(
      "Functionality associated with the alpha toolset is provided via bash commands.",
    );
    expect(result.tools.alpha_help?.description).toContain("alpha_echo");
    await expect(result.tools.alpha_help?.execute()).resolves.toBe(
      result.tools.alpha_help?.description,
    );
  });

  it("transforms executable tools into generated code clients and help tools", async () => {
    const transform = await transformToolsForCodeMode(
      {
        echo: {
          name: "echo",
          description: "Echo a message.",
          inputSchema: {
            type: "object",
            properties: { message: { type: "string" } },
            required: ["message"],
          },
          execute: async (input = {}) => `code:${String(input.message)}`,
        },
      },
      { language: "python", serverName: "alpha", outputDir: join(tmpdir(), "mcp-code-mode") },
    );
    try {
      expect(Object.keys(transform.tools)).toEqual(["alpha_help"]);
      expect(Object.keys(transform.files)).toContain("alpha.py");
      const helpTool = transform.tools.alpha_help;
      expect(helpTool?.description).toContain(
        "Functionality associated with the alpha toolset is provided via generated Python code.",
      );
      expect(helpTool?.description).toContain("Primary module:");
      expect(helpTool?.description).not.toContain("Code Mode");
      await expect(helpTool?.execute()).resolves.toBe(helpTool?.description);
      expect(transform.environment.PYTHONPATH).toContain("mcp-code-mode");
    } finally {
      transform.close();
    }
  });

  it("transforms executable tools into generated CLI clients and help tools", async () => {
    const outputDir = join(tmpdir(), "mcp-cli-mode");
    const transform = await transformToolsForCliMode(
      {
        echo: {
          name: "echo",
          description: "Echo a message.",
          inputSchema: {
            type: "object",
            properties: { message: { type: "string" } },
            required: ["message"],
          },
          execute: async (input = {}): Promise<unknown> => ({
            rows: [{ message: String(input.message), ok: true }],
          }),
        },
      },
      { serverName: "alpha", outputDir },
    );
    try {
      expect(Object.keys(transform.tools)).toEqual(["alpha_help"]);
      expect(Object.keys(transform.files)).toContain("alpha");
      const helpTool = transform.tools.alpha_help;
      expect(helpTool?.description).toContain(
        "Functionality associated with the alpha toolset is provided via the `alpha` CLI.",
      );
      expect(helpTool?.description).toContain("USAGE:");
      expect(helpTool?.description).toContain("SUBCOMMANDS:");
      expect(helpTool?.description).toContain("  echo  Echo a message.");
      expect(helpTool?.description).toContain("Run 'alpha --help' in the shell for usage.");
      expect(helpTool?.description).not.toContain("PATH hint");
      expect(helpTool?.description).not.toContain("CLI Mode");
      await expect(helpTool?.execute()).resolves.toBe(helpTool?.description);
      expect(transform.environment.PATH).toContain("mcp-cli-mode");
      const response = await fetch(`${transform.bridgeUrl}/health`);
      expect(response.status).toBe(200);
      const execResponse = await fetch(`${transform.bridgeUrl}/exec`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          authorization: `Bearer ${transform.token}`,
        },
        body: JSON.stringify({ tool: "echo", input: { message: "bridge" } }),
      });
      expect(execResponse.status).toBe(200);
      const execBody = (await execResponse.json()) as { result: string };
      expect(execBody.result).toContain('"message":"bridge"');
    } finally {
      transform.close();
    }
  });

  it("defaults CLI transform output to a user PATH directory", async () => {
    const previousHome = process.env.HOME;
    const home = mkdtempSync(join(tmpdir(), "mcp-cli-home-"));
    process.env.HOME = home;
    const transform = await transformToolsForCliMode(
      {
        echo: {
          name: "echo",
          description: "Echo a message.",
          inputSchema: { type: "object", properties: {} },
          execute: async () => "ok",
        },
      },
      { serverName: "alpha" },
    );
    try {
      const expectedDir = join(home, ".local", "bin");
      expect(transform.paths).toEqual([join(expectedDir, "alpha")]);
      expect(transform.environment.PATH).toBe(`${expectedDir}:$PATH`);
      expect(transform.tools.alpha_help?.description).toContain("`alpha` CLI");
      expect(transform.tools.alpha_help?.description).not.toContain("PATH hint");
    } finally {
      transform.close();
      if (previousHome === undefined) {
        delete process.env.HOME;
      } else {
        process.env.HOME = previousHome;
      }
    }
  });
});

describe("Rust native core wrapper", () => {
  it("exports public config helpers and types from package root", () => {
    process.env.MCP_COMPRESSOR_TEST_TOKEN = "root-token";
    try {
      expect(interpolateString("Bearer ${MCP_COMPRESSOR_TEST_TOKEN}")).toBe("Bearer root-token");
      expect(interpolateRecord({ Authorization: "Bearer $MCP_COMPRESSOR_TEST_TOKEN" })).toEqual({
        Authorization: "Bearer root-token",
      });
      const backend: BackendConfig = { type: "stdio", command: "python", args: ["server.py"] };
      const entry: JsonConfigServerEntry = { command: backend.command, args: backend.args };
      expect(entry.command).toBe("python");
    } finally {
      delete process.env.MCP_COMPRESSOR_TEST_TOKEN;
    }
  });

  it("compresses tool listings through the native addon", () => {
    expect(compressToolListing("high", [sampleTool])).toBe("<tool>echo(message)</tool>");
  });

  it("formats schema responses through the native addon", () => {
    const response = formatToolSchemaResponse(sampleTool);
    expect(response).toContain("Echo a value.");
    expect(response).toContain('"message"');
  });

  it("parses generated CLI argv through the native addon", () => {
    expect(parseToolArgv(sampleTool, ["--message", "hello"])).toEqual({ message: "hello" });
  });

  it("generates client artifacts through the native addon", () => {
    const outputDir = mkdtempSync(join(tmpdir(), "mcp-compressor-rust-core-"));
    mkdirSync(outputDir, { recursive: true });
    const cliPaths = generateClientArtifacts("cli", {
      cliName: "native-alpha",
      bridgeUrl: "http://127.0.0.1:12345",
      token: "token".repeat(16),
      tools: [sampleTool],
      sessionPid: 42,
      outputDir,
    });
    expect(cliPaths).toHaveLength(1);
    expect(readFileSync(cliPaths[0]!, "utf8")).toContain("native-alpha - the native-alpha toolset");

    const pyPaths = generateClientArtifacts("python", {
      cliName: "native-alpha",
      bridgeUrl: "http://127.0.0.1:12345",
      token: "token".repeat(16),
      tools: [sampleTool],
      sessionPid: 42,
      outputDir,
    });
    expect(pyPaths.some((path) => path.endsWith(".py"))).toBe(true);

    const tsPaths = generateClientArtifacts("typescript", {
      cliName: "native-alpha",
      bridgeUrl: "http://127.0.0.1:12345",
      token: "token".repeat(16),
      tools: [sampleTool],
      sessionPid: 42,
      outputDir,
    });
    expect(tsPaths.some((path) => path.endsWith(".ts"))).toBe(true);
    expect(tsPaths.some((path) => path.endsWith(".d.ts"))).toBe(true);
  });

  it("toonifies JSON outputs through native session config", async () => {
    const session = await startCompressedSession(
      {
        compressionLevel: "max",
        serverName: "alpha",
        toonify: true,
      },
      [alphaBackend()],
    );
    const info = session.info();
    const invokeTool = info.frontend_tools.find((tool) => tool.name.endsWith("invoke_tool"));
    expect(invokeTool).toBeDefined();
    const output = await invokeProxy(
      info.bridge_url,
      info.token,
      invokeTool!.name,
      "structured_data",
      {},
    );
    expect(output).toContain("server: alpha");
    expect(output).toContain("values");
    expect(output.trim()).not.toMatch(/^\{/);
  });

  it("applies include and exclude filters through native session config", async () => {
    const session = await startCompressedSession(
      {
        compressionLevel: "max",
        serverName: "alpha",
        includeTools: ["echo", "add"],
        excludeTools: ["add"],
      },
      [alphaBackend()],
    );
    const info = session.info();
    const invokeTool = info.frontend_tools.find((tool) => tool.name.endsWith("invoke_tool"));
    expect(info.frontend_tools.some((tool) => tool.name.endsWith("list_tools"))).toBe(true);
    expect(invokeTool).toBeDefined();
    await expect(
      invokeProxy(info.bridge_url, info.token, invokeTool!.name, "echo", { message: "filtered" }),
    ).resolves.toBe("alpha:filtered");
    await expect(
      invokeProxy(info.bridge_url, info.token, invokeTool!.name, "add", { a: 1, b: 2 }),
    ).rejects.toThrow(/tool not found|unknown tool|not found/i);
  });

  it("starts a compressed session and invokes a real backend through the proxy", async () => {
    const session = await startCompressedSession(
      {
        compressionLevel: "max",
        serverName: "alpha",
      },
      [alphaBackend()],
    );
    const info = session.info();
    expect(info.bridge_url).toMatch(/^http:\/\/127\.0\.0\.1:/);
    const invokeTool = info.frontend_tools.find((tool) => tool.name.endsWith("invoke_tool"));
    expect(invokeTool).toBeDefined();
    await expect(
      invokeProxy(info.bridge_url, info.token, invokeTool!.name, "echo", { message: "ts" }),
    ).resolves.toBe("alpha:ts");
  });

  it("provides a high-level CompressorClient for compressed tools", async () => {
    const previousPath = process.env.PATH;
    const previousBinary = process.env.MCP_COMPRESSOR_BINARY;
    process.env.PATH = "";
    process.env.MCP_COMPRESSOR_BINARY = "definitely-missing-mcp-compressor";
    try {
      const fixtureDir = join(
        process.cwd(),
        "..",
        "crates",
        "mcp-compressor-core",
        "tests",
        "fixtures",
      );
      const python = process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python");
      const client = new CompressorClient({
        servers: {
          alpha: { command: python, args: [join(fixtureDir, "alpha_server.py")] },
          beta: { command: python, args: [join(fixtureDir, "beta_server.py")] },
        },
        compressionLevel: "max",
      });
      const proxy = await client.connect();
      try {
        expect(proxy.tools.map((tool) => tool.name)).toContain("alpha_invoke_tool");
        expect(proxy.schema("echo", { server: "alpha" })).toBeDefined();
        await expect(proxy.invoke("echo", { message: "sdk" }, { server: "alpha" })).resolves.toBe(
          "alpha:sdk",
        );
        await expect(proxy.invoke("multiply", { a: 6, b: 7 }, { server: "beta" })).resolves.toBe(
          "42",
        );
      } finally {
        await client.close();
      }
    } finally {
      if (previousPath === undefined) {
        delete process.env.PATH;
      } else {
        process.env.PATH = previousPath;
      }
      if (previousBinary === undefined) {
        delete process.env.MCP_COMPRESSOR_BINARY;
      } else {
        process.env.MCP_COMPRESSOR_BINARY = previousBinary;
      }
    }
  });

  it("writes generated clients from the high-level native proxy", async () => {
    const fixtureDir = join(
      process.cwd(),
      "..",
      "crates",
      "mcp-compressor-core",
      "tests",
      "fixtures",
    );
    const python = process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python");
    const outputDir = mkdtempSync(join(tmpdir(), "mcp-compressor-generated-"));
    const client = new CompressorClient({
      servers: { alpha: { command: python, args: [join(fixtureDir, "alpha_server.py")] } },
      compressionLevel: "max",
    });
    const proxy = await client.connect();
    try {
      const cliPaths = proxy.writeClient("cli", join(outputDir, "bin"), { name: "alpha" });
      const pythonClient = proxy.writeCodeClient({
        language: "python",
        outputDir: join(outputDir, "py"),
        name: "alpha",
      });
      const typescriptClient = proxy.writeCodeClient({
        language: "typescript",
        outputDir: join(outputDir, "ts"),
        name: "alpha",
      });
      const pythonPaths = pythonClient.files;
      const tsPaths = typescriptClient.files;
      expect(pythonClient.environment).toEqual({ PYTHONPATH: join(outputDir, "py") });
      expect(typescriptClient.environment).toEqual({});
      const cliPath = cliPaths.find((path) => path.endsWith("alpha"));
      const pythonPath = pythonPaths.find((path) => path.endsWith("alpha.py"));
      const tsPath = tsPaths.find((path) => path.endsWith("alpha.ts"));
      expect(cliPath).toBeDefined();
      expect(pythonPath).toBeDefined();
      expect(tsPath).toBeDefined();
      const cliResult = await new Promise<string>((resolve, reject) => {
        const child = spawn(cliPath!, ["echo", "--message", "generated-cli"]);
        let stdout = "";
        let stderr = "";
        child.stdout.on("data", (chunk) => {
          stdout += String(chunk);
        });
        child.stderr.on("data", (chunk) => {
          stderr += String(chunk);
        });
        child.on("error", reject);
        child.on("exit", (code) => {
          if (code === 0) resolve(stdout.trim());
          else reject(new Error(stderr));
        });
      });
      expect(cliResult).toBe("alpha:generated-cli");
      expect(tsPaths.some((path) => path.endsWith("alpha.d.ts"))).toBe(true);
      const pyResult = await new Promise<string>((resolve, reject) => {
        const child = spawn(python, [
          "-c",
          `import sys; sys.path.insert(0, ${JSON.stringify(pythonPath!.replace(/\/alpha\.py$/, ""))}); import alpha; print(alpha.echo('generated'))`,
        ]);
        let stdout = "";
        let stderr = "";
        child.stdout.on("data", (chunk) => {
          stdout += String(chunk);
        });
        child.stderr.on("data", (chunk) => {
          stderr += String(chunk);
        });
        child.on("error", reject);
        child.on("exit", (code) => {
          if (code === 0) resolve(stdout.trim());
          else reject(new Error(stderr));
        });
      });
      expect(pyResult).toBe("alpha:generated");
      const tsResult = await import(tsPath!);
      await expect(tsResult.echo("generated-ts")).resolves.toBe("alpha:generated-ts");
    } finally {
      proxy.close();
    }
  });

  it("reports invalid high-level native server configs", async () => {
    const client = new CompressorClient({
      servers: { bad: { args: ["unused"] } as unknown as { command: string } },
    });
    await expect(client.connect()).rejects.toThrow(/must define command or url/);
  });

  it("reports missing high-level native wrappers", async () => {
    const fixtureDir = join(
      process.cwd(),
      "..",
      "crates",
      "mcp-compressor-core",
      "tests",
      "fixtures",
    );
    const python = process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python");
    const client = new CompressorClient({
      servers: { alpha: { command: python, args: [join(fixtureDir, "alpha_server.py")] } },
      compressionLevel: "max",
    });
    const proxy = await client.connect();
    try {
      expect(() => proxy.schema("echo", { server: "missing" })).toThrow(/Backend tool not found/);
    } finally {
      proxy.close();
    }
  });

  it("makes high-level CompressorClient lifecycle explicit", async () => {
    const fixtureDir = join(
      process.cwd(),
      "..",
      "crates",
      "mcp-compressor-core",
      "tests",
      "fixtures",
    );
    const python = process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python");
    const client = new CompressorClient({
      servers: { alpha: { command: python, args: [join(fixtureDir, "alpha_server.py")] } },
      compressionLevel: "max",
    });
    const proxy = await client.connect();
    await expect(proxy.invoke("echo", { message: "before-close" })).resolves.toBe(
      "alpha:before-close",
    );
    proxy.close();
    proxy.close();
    await expect(proxy.invoke("echo", { message: "after-close" })).rejects.toThrow();
  });

  it("defaults single-server CompressorClient invocation to that server", async () => {
    const previousPath = process.env.PATH;
    const previousBinary = process.env.MCP_COMPRESSOR_BINARY;
    process.env.PATH = "";
    process.env.MCP_COMPRESSOR_BINARY = "definitely-missing-mcp-compressor";
    try {
      const fixtureDir = join(
        process.cwd(),
        "..",
        "crates",
        "mcp-compressor-core",
        "tests",
        "fixtures",
      );
      const python = process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python");
      const client = new CompressorClient({
        servers: { alpha: { command: python, args: [join(fixtureDir, "alpha_server.py")] } },
        compressionLevel: "max",
      });
      const proxy = await client.connect();
      try {
        await expect(proxy.invoke("echo", { message: "default" })).resolves.toBe("alpha:default");
        expect(proxy.schema("echo")).toBeDefined();
      } finally {
        await client.close();
      }
    } finally {
      if (previousPath === undefined) delete process.env.PATH;
      else process.env.PATH = previousPath;
      if (previousBinary === undefined) delete process.env.MCP_COMPRESSOR_BINARY;
      else process.env.MCP_COMPRESSOR_BINARY = previousBinary;
    }
  });

  it("exposes high-level native CLI and Bash transform surfaces", async () => {
    const fixtureDir = join(
      process.cwd(),
      "..",
      "crates",
      "mcp-compressor-core",
      "tests",
      "fixtures",
    );
    const python = process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python");
    const cliClient = new CompressorClient({
      servers: { alpha: { command: python, args: [join(fixtureDir, "alpha_server.py")] } },
      mode: "cli",
      compressionLevel: "max",
    });
    const cliProxy = await cliClient.connect();
    try {
      expect(cliProxy.tools.map((tool) => tool.name)).toEqual(["alpha_help"]);
    } finally {
      await cliClient.close();
    }

    const bashClient = new CompressorClient({
      servers: {
        alpha: { command: python, args: [join(fixtureDir, "alpha_server.py")] },
        beta: { command: python, args: [join(fixtureDir, "beta_server.py")] },
      },
      mode: "bash",
      compressionLevel: "max",
    });
    const bashProxy = await bashClient.connect();
    try {
      expect(bashProxy.tools.map((tool) => tool.name)).toEqual(
        expect.arrayContaining(["bash_tool", "alpha_help", "beta_help"]),
      );
      const alphaProvider = bashProxy.justBashProviders.find(
        (provider) => provider.providerName === "alpha",
      );
      expect(alphaProvider?.helpToolName).toBe("alpha_help");
      expect(alphaProvider?.tools).toContainEqual(
        expect.objectContaining({
          commandName: "echo",
          backendToolName: "echo",
          invokeToolName: "alpha_invoke_tool",
        }),
      );
      const bash = new Bash({ customCommands: [] });
      const registrations = installJustBashCommands(bash, bashProxy);
      expect(registrations.map((registration) => registration.commandName)).toEqual(
        expect.arrayContaining(["alpha_echo", "beta_echo"]),
      );
      const result = await bash.exec("alpha_echo --message via-bash");
      expect(result.exitCode).toBe(0);
      expect(result.stdout.trim()).toBe("alpha:via-bash");
    } finally {
      await bashClient.close();
    }
  });

  it("calls authProvider each time servers are normalized", async () => {
    let calls = 0;
    const normalized = await normalizeServers({
      remote: {
        url: "https://example.test/mcp",
        headers: { "X-Static": "yes" },
        authProvider: async () => {
          calls += 1;
          return { Authorization: `Bearer token-${calls}` };
        },
      },
    });

    expect(calls).toBe(1);
    expect(Array.isArray(normalized)).toBe(true);
    if (!Array.isArray(normalized)) throw new Error("expected normalized backend list");
    expect(normalized).toHaveLength(1);
    expect(normalized[0]).toMatchObject({
      name: "remote",
      commandOrUrl: "https://example.test/mcp",
    });
    expect(normalized[0]!.args).toEqual(
      expect.arrayContaining([
        "-H",
        "Authorization=Bearer token-1",
        "X-Static=yes",
        "--auth",
        "explicit-headers",
      ]),
    );
  });

  it("normalizes high-level native server config", async () => {
    const normalized = await normalizeServers({
      remote: {
        url: "https://example.test/mcp",
        headers: { Authorization: "Bearer token" },
        oauthAppName: "Rovo Dev",
        args: ["--auth", "explicit-headers"],
      },
    });
    expect(normalized).toEqual([
      {
        name: "remote",
        commandOrUrl: "https://example.test/mcp",
        args: ["-H", "Authorization=Bearer token", "--auth", "explicit-headers"],
        oauth_app_name: "Rovo Dev",
      },
    ]);
  });

  it("lets a TypeScript agent start a compressed multi-server proxy without a compressor subprocess", async () => {
    const previousPath = process.env.PATH;
    const previousBinary = process.env.MCP_COMPRESSOR_BINARY;
    process.env.PATH = "";
    process.env.MCP_COMPRESSOR_BINARY = "definitely-missing-mcp-compressor";
    try {
      const fixtureDir = join(
        process.cwd(),
        "..",
        "crates",
        "mcp-compressor-core",
        "tests",
        "fixtures",
      );
      const python = process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python");
      const session = await startCompressedSessionFromMcpConfig(
        { compressionLevel: "max" },
        JSON.stringify({
          mcpServers: {
            alpha: { command: python, args: [join(fixtureDir, "alpha_server.py")] },
            beta: { command: python, args: [join(fixtureDir, "beta_server.py")] },
          },
        }),
      );
      try {
        const info = session.info();
        expect(info.frontend_tools.map((tool) => tool.name)).toContain("alpha_invoke_tool");
        expect(info.frontend_tools.map((tool) => tool.name)).toContain("beta_invoke_tool");
        await expect(
          invokeProxy(info.bridge_url, info.token, "alpha_invoke_tool", "echo", {
            message: "agent",
          }),
        ).resolves.toBe("alpha:agent");
        await expect(
          invokeProxy(info.bridge_url, info.token, "beta_invoke_tool", "multiply", { a: 6, b: 7 }),
        ).resolves.toBe("42");
      } finally {
        session.close();
      }
    } finally {
      if (previousPath === undefined) {
        delete process.env.PATH;
      } else {
        process.env.PATH = previousPath;
      }
      if (previousBinary === undefined) {
        delete process.env.MCP_COMPRESSOR_BINARY;
      } else {
        process.env.MCP_COMPRESSOR_BINARY = previousBinary;
      }
    }
  });

  it("starts a compressed session from MCP config and routes multiple backends", async () => {
    const fixtureDir = join(
      process.cwd(),
      "..",
      "crates",
      "mcp-compressor-core",
      "tests",
      "fixtures",
    );
    const python = process.env.PYTHON ?? join(process.cwd(), "..", ".venv", "bin", "python");
    const session = await startCompressedSessionFromMcpConfig(
      { compressionLevel: "max" },
      JSON.stringify({
        mcpServers: {
          alpha: { command: python, args: [join(fixtureDir, "alpha_server.py")] },
          beta: { command: python, args: [join(fixtureDir, "beta_server.py")] },
        },
      }),
    );
    const info = session.info();
    expect(info.frontend_tools.some((tool) => tool.name === "alpha_invoke_tool")).toBe(true);
    expect(info.frontend_tools.some((tool) => tool.name === "beta_invoke_tool")).toBe(true);
    await expect(
      invokeProxy(info.bridge_url, info.token, "alpha_invoke_tool", "add", { a: 2, b: 3 }),
    ).resolves.toBe("5");
    await expect(
      invokeProxy(info.bridge_url, info.token, "beta_invoke_tool", "multiply", { a: 4, b: 5 }),
    ).resolves.toBe("20");
  });

  it("starts a compressed session against a remote streamable HTTP backend", async () => {
    const upstream = await startRemoteAlphaUpstream();
    try {
      const session = await startCompressedSession(
        { compressionLevel: "max", serverName: "remote-alpha" },
        [
          {
            name: "remote-alpha",
            commandOrUrl: upstream.url,
            args: ["--auth", "explicit-headers"],
          },
        ],
      );
      const info = session.info();
      const invokeTool = info.frontend_tools.find((tool) => tool.name.endsWith("invoke_tool"));
      expect(invokeTool).toBeDefined();
      await expect(
        invokeProxy(info.bridge_url, info.token, invokeTool!.name, "alpha_invoke_tool", {
          tool_name: "echo",
          tool_input: { message: "remote-ts" },
        }),
      ).resolves.toBe("alpha:remote-ts");
    } finally {
      await stopChild(upstream.child);
    }
  }, 90_000);

  it("starts a CLI transform-mode session through the native addon", async () => {
    const session = await startCompressedSession(
      {
        compressionLevel: "max",
        serverName: "alpha",
        transformMode: "cli",
      },
      [alphaBackend()],
    );
    const info = session.info();
    expect(info.frontend_tools).toHaveLength(1);
    expect(info.frontend_tools[0]!.name).toMatch(/alpha_help$/);
  });

  it("starts a Just Bash transform-mode session with typed provider metadata", async () => {
    const session = await startCompressedSession(
      {
        compressionLevel: "max",
        transformMode: "just-bash",
      },
      [alphaBackend(), betaBackend()],
    );
    const info = session.info();
    expect(info.frontend_tools.some((tool) => tool.name === "bash_tool")).toBe(true);
    expect(info.frontend_tools.some((tool) => tool.name === "alpha_help")).toBe(true);
    expect(info.frontend_tools.some((tool) => tool.name === "beta_help")).toBe(true);
    expect(info.just_bash_providers.map((provider) => provider.provider_name).sort()).toEqual([
      "alpha",
      "beta",
    ]);
    const alphaProvider = info.just_bash_providers.find(
      (provider) => provider.provider_name === "alpha",
    );
    expect(alphaProvider?.help_tool_name).toBe("alpha_help");
    expect(alphaProvider?.tools.some((command) => command.command_name === "echo")).toBe(true);
    expect(
      alphaProvider?.tools.some((command) => command.invoke_tool_name === "alpha_invoke_tool"),
    ).toBe(true);
  });

  it("lists and clears OAuth credentials through the native addon", () => {
    const previousXdg = process.env.XDG_CONFIG_HOME;
    const previousHome = process.env.HOME;
    const configHome = mkdtempSync(join(tmpdir(), "mcp-compressor-oauth-"));
    process.env.XDG_CONFIG_HOME = configHome;
    process.env.HOME = configHome;
    try {
      expect(listOAuthCredentials()).toEqual([]);
      const storeDir = join(configHome, "oauth-store");
      mkdirSync(storeDir, { recursive: true });
      rememberOAuthBackend("https://example.test/mcp", "example", storeDir);
      expect(listOAuthCredentials()).toEqual([
        {
          backend_name: "example",
          backend_uri: "https://example.test/mcp",
          store_dir: storeDir,
        },
      ]);
      expect(clearOAuthCredentials("missing")).toEqual([]);
      expect(clearOAuthCredentials("example")).toEqual([storeDir]);
      expect(listOAuthCredentials()).toEqual([]);
    } finally {
      if (previousXdg === undefined) {
        delete process.env.XDG_CONFIG_HOME;
      } else {
        process.env.XDG_CONFIG_HOME = previousXdg;
      }
      if (previousHome === undefined) {
        delete process.env.HOME;
      } else {
        process.env.HOME = previousHome;
      }
    }
  });

  it("parses MCP config through the native addon", () => {
    expect(
      parseMcpConfig('{"mcpServers":{"my-server":{"command":"python","args":["server.py"]}}}'),
    ).toEqual([
      {
        name: "my-server",
        command: "python",
        args: ["server.py"],
        env: [],
        cli_prefix: "my-server",
      },
    ]);
  });
});
