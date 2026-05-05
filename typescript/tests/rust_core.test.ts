import { mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { request } from "node:http";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  compressToolListing,
  formatToolSchemaResponse,
  clearOAuthCredentials,
  generateClientArtifacts,
  listOAuthCredentials,
  startCompressedSession,
  startCompressedSessionFromMcpConfig,
  parseMcpConfig,
  parseToolArgv,
  type RustTool,
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
    commandOrUrl: process.env.PYTHON ?? "python3",
    args: [fixturePath("alpha_server.py")],
  };
}

function betaBackend() {
  return {
    name: "beta",
    commandOrUrl: process.env.PYTHON ?? "python3",
    args: [fixturePath("beta_server.py")],
  };
}

async function startRemoteAlphaUpstream(): Promise<{
  url: string;
  child: ChildProcessWithoutNullStreams;
}> {
  const child = spawn(
    "cargo",
    [
      "run",
      "-q",
      "-p",
      "mcp-compressor-core",
      "--",
      "--compression",
      "max",
      "--server-name",
      "alpha",
      "--transport",
      "streamable-http",
      "--port",
      "0",
      "--",
      process.env.PYTHON ?? "python3",
      fixturePath("alpha_server.py"),
    ],
    {
      cwd: join(process.cwd(), ".."),
      env: { ...process.env, PYTHON: process.env.PYTHON ?? "python3" },
    },
  );

  const url = await new Promise<string>((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error("timed out waiting for streamable HTTP upstream URL"));
    }, 30_000);
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => {
      const match = /listening on (http:\/\/127\.0\.0\.1:\d+\/mcp)/.exec(String(chunk));
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
      reject(new Error(`streamable HTTP upstream exited before ready: ${code}`));
    });
  });

  return { url, child };
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

const sampleTool: RustTool = {
  name: "echo",
  description: "Echo a value.",
  inputSchema: {
    type: "object",
    properties: { message: { type: "string" } },
    required: ["message"],
  },
};

describe("Rust native core wrapper", () => {
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

  it("starts a compressed session from MCP config and routes multiple backends", async () => {
    const fixtureDir = join(
      process.cwd(),
      "..",
      "crates",
      "mcp-compressor-core",
      "tests",
      "fixtures",
    );
    const python = process.env.PYTHON ?? "python3";
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
      expect(clearOAuthCredentials()).toEqual([]);
      expect(clearOAuthCredentials("missing")).toEqual([]);
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
