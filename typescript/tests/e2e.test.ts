import { test, expect } from "vitest";
import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

import {
  createCompressorRuntime,
  createMultiCompressorServer,
  initializeCliMode,
  resolveBackends,
} from "../src/index.js";
import { BackendClient } from "../src/backend-client.js";
import type { StdioBackendConfig } from "../src/types.js";

function pythonServerPath(name: string): string {
  return path.resolve("..", "tests", name);
}

function pythonBackend(name: string): StdioBackendConfig {
  return process.env.MCP_COMPRESSOR_E2E_PYTHON
    ? {
        type: "stdio",
        command: process.env.MCP_COMPRESSOR_E2E_PYTHON,
        args: [pythonServerPath(name)],
      }
    : { type: "stdio", command: "uv", args: ["run", "python", pythonServerPath(name)] };
}

function singleServerConfigJson(): string {
  const alpha = pythonBackend("e2e_server_alpha.py");
  return JSON.stringify({
    mcpServers: {
      alpha: {
        command: alpha.command,
        args: alpha.args,
      },
    },
  });
}

function multiServerConfigJson(): string {
  const alpha = pythonBackend("e2e_server_alpha.py");
  const beta = pythonBackend("e2e_server_beta.py");
  return JSON.stringify({
    mcpServers: {
      alpha: {
        command: alpha.command,
        args: alpha.args,
      },
      beta: {
        command: beta.command,
        args: beta.args,
      },
    },
  });
}

test("TypeScript single-server direct backend proxy works with Python FastMCP e2e server", async () => {
  const runtime = createCompressorRuntime({
    backend: pythonBackend("e2e_server_alpha.py"),
    compressionLevel: "max",
    serverName: "alpha",
  });

  await runtime.connect();
  try {
    expect(await runtime.listToolNames()).toEqual(["alpha_add", "alpha_echo", "alpha_object"]);
    expect(await runtime.getToolSchema("alpha_echo").then((tool) => JSON.stringify(tool))).toMatch(
      /alpha_echo/,
    );
    expect(await runtime.invokeTool("alpha_echo", { message: "hello" })).toMatch(/alpha:hello/);

    const toolset = runtime.getFunctionToolset();
    expect(Object.keys(toolset).sort()).toEqual([
      "alpha_get_tool_schema",
      "alpha_invoke_tool",
      "alpha_list_tools",
    ]);
    expect(await toolset.alpha_list_tools()).toMatch(/alpha_add/);
  } finally {
    await runtime.disconnect();
  }
});

test("TypeScript single-server MCP config supports filters and toonify with Python FastMCP e2e server", async () => {
  const resolved = resolveBackends(singleServerConfigJson())[0]!;
  expect(resolved.serverName).toBe("alpha");

  const runtime = createCompressorRuntime({
    backend: singleServerConfigJson(),
    compressionLevel: "low",
    includeTools: ["alpha_object", "alpha_echo"],
    excludeTools: ["alpha_echo"],
    toonify: true,
  });

  await runtime.connect();
  try {
    expect(await runtime.listToolNames()).toEqual(["alpha_object"]);
    expect(await runtime.invokeTool("alpha_object", {})).toMatch(/server: alpha/);
    await expect(() => runtime.getToolSchema("alpha_echo")).rejects.toThrow(
      /Available tools: alpha_object/,
    );
  } finally {
    await runtime.disconnect();
  }
});

test("TypeScript BackendClient can read Python FastMCP resources directly", async () => {
  const backendClient = new BackendClient(pythonBackend("e2e_server_alpha.py"));
  await backendClient.connect();
  try {
    const resource = await backendClient.readResource("e2e://alpha-resource");
    expect(JSON.stringify(resource)).toMatch(/alpha resource/);
  } finally {
    await backendClient.disconnect();
  }
});

test("TypeScript multi-server proxy works with Python FastMCP e2e servers", async () => {
  const resolved = resolveBackends(multiServerConfigJson(), "suite");
  expect(resolved.map((entry) => entry.serverName)).toEqual(["suite_alpha", "suite_beta"]);

  const server = createMultiCompressorServer({
    backends: resolved.map((entry) => ({ backend: entry.backend, serverName: entry.serverName! })),
    compressionLevel: "max",
    toonify: true,
  });

  await server.connectAll();
  try {
    const alphaRuntime = server.runtimes[0]!;
    const betaRuntime = server.runtimes[1]!;

    expect(await alphaRuntime.listToolNames()).toEqual(["alpha_add", "alpha_echo", "alpha_object"]);
    expect(await betaRuntime.listToolNames()).toEqual([
      "beta_echo",
      "beta_multiply",
      "beta_object",
    ]);

    expect(await alphaRuntime.invokeTool("alpha_add", { a: 2, b: 5 })).toMatch(
      /result: 7|text,"7"/,
    );
    expect(await betaRuntime.invokeTool("beta_multiply", { a: 3, b: 4 })).toMatch(
      /result: 12|text,"12"/,
    );
    expect(await alphaRuntime.invokeTool("alpha_object", {})).toMatch(/server: alpha/);

    const alphaToolset = alphaRuntime.getFunctionToolset();
    const betaToolset = betaRuntime.getFunctionToolset();
    expect(Object.keys(alphaToolset).sort()).toEqual([
      "suite_alpha_get_tool_schema",
      "suite_alpha_invoke_tool",
      "suite_alpha_list_tools",
    ]);
    expect(Object.keys(betaToolset).sort()).toEqual([
      "suite_beta_get_tool_schema",
      "suite_beta_invoke_tool",
      "suite_beta_list_tools",
    ]);
    expect(await alphaToolset.suite_alpha_get_tool_schema({ tool_name: "alpha_echo" })).toMatch(
      /alpha_echo/,
    );
    expect(await betaToolset.suite_beta_list_tools()).toMatch(/beta_multiply/);
  } finally {
    await server.closeAll();
  }
});

test("TypeScript single-server CLI mode works with Python FastMCP MCP config", async () => {
  const session = await initializeCliMode({
    backend: singleServerConfigJson(),
    cliPort: 0,
    compressionLevel: "low",
    scriptDir: path.resolve(".."),
    toonify: true,
  });

  try {
    expect(session.cliName).toBe("alpha");
    expect(session.runtimes.length).toBe(1);

    const runtime = session.runtimes[0]!;
    const tools = await runtime.listUncompressedTools();
    expect(tools.map((tool) => tool.name).sort()).toEqual([
      "alpha_add",
      "alpha_echo",
      "alpha_object",
    ]);

    const aiSdkTools = await runtime.getAiSdkTools();
    expect(Object.keys(aiSdkTools)).toEqual(["alpha_help"]);
    expect(aiSdkTools.alpha_help!.description).toMatch(/alpha-add/);

    const script = session.scripts[0]!;
    const invokeResponse = await fetch(`${script.bridgeUrl}/exec`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ argv: ["alpha-add", "--a", "8", "--b", "9"] }),
    });
    expect(invokeResponse.status).toBe(200);
    expect(await invokeResponse.text()).toBe("17");

    try {
      await execFileAsync(script.scriptPath!, ["alpha-echo", "hello"]);
      expect.unreachable("Expected execFileAsync to throw");
    } catch (error: unknown) {
      expect(
        error && typeof error === "object" && "stdout" in error && "stderr" in error,
      ).toBeTruthy();
      const output = `${String((error as { stdout: unknown }).stdout)}${String((error as { stderr: unknown }).stderr)}`;
      expect(output).toMatch(/Unknown option: hello/);
      expect(output).toMatch(/Usage: alpha alpha-echo \[options\]/);
      expect(output).toMatch(/--message/);
    }
  } finally {
    await session.close();
  }
});

test("TypeScript single-server just-bash mode works with Python FastMCP e2e server", async () => {
  const { createBashCommand, buildBashToolDescription } = await import("../src/bash_commands.js");
  const { installPipingHintPlugin } = await import("../src/just_bash_transform.js");
  const { Bash } = await import("just-bash");

  const resolved = resolveBackends(singleServerConfigJson())[0]!;
  const runtime = createCompressorRuntime({
    backend: resolved.backend,
    compressionLevel: "low",
    serverName: resolved.serverName,
    toonify: true,
  });
  await runtime.connect();

  try {
    const tools = await runtime.listUncompressedTools();
    const command = createBashCommand(runtime, tools);
    const bash = new Bash({ customCommands: [command] });
    installPipingHintPlugin(bash, [command.name]);

    // Parent command should be named after the server
    expect(command.name).toBe("alpha");

    // Invoke a subcommand: `alpha alpha-add --a 8 --b 9`
    const addResult = await bash.exec("alpha alpha-add --a 8 --b 9");
    expect(addResult.exitCode).toBe(0);
    expect(addResult.stdout).toBe("17");

    // Help on parent should list subcommands
    const helpResult = await bash.exec("alpha --help");
    expect(helpResult.exitCode).toBe(0);
    expect(helpResult.stdout).toContain("alpha-echo");
    expect(helpResult.stdout).toContain("alpha-add");

    // Help on subcommand should show options
    const subHelpResult = await bash.exec("alpha alpha-echo --help");
    expect(subHelpResult.exitCode).toBe(0);
    expect(subHelpResult.stdout).toContain("--message");

    // Tool description lists only the top-level command + TOON note.
    const description = buildBashToolDescription([{ serverName: "alpha", command, tools }]);
    expect(description).toContain("- `alpha`");
    expect(description).toContain("custom commands are installed");
    expect(description).toContain("TOON");
    // Subcommands belong in the per-server help tools, not the bash description.
    expect(description).not.toContain("alpha-add");
    expect(description).not.toContain("alpha-echo");

    // Standard bash built-ins should also work alongside MCP commands
    const echoResult = await bash.exec('echo "hello world"');
    expect(echoResult.exitCode).toBe(0);
    expect(echoResult.stdout.trim()).toBe("hello world");

    // Auto-detect: unpiped output is TOON (no leading '{').
    const objectResult = await bash.exec("alpha alpha-object");
    expect(objectResult.exitCode).toBe(0);
    expect(objectResult.stdout.trim().startsWith("{")).toBe(false);
    expect(objectResult.stdout).toContain("alpha");

    // Auto-detect: piped output is JSON (parseable by jq).
    const jqResult = await bash.exec("alpha alpha-object | jq -r .server");
    expect(jqResult.exitCode).toBe(0);
    expect(jqResult.stdout.trim()).toBe("alpha");

    // Explicit --json forces JSON regardless of pipe context.
    const jsonResult = await bash.exec("alpha alpha-object --json");
    expect(jsonResult.exitCode).toBe(0);
    expect(jsonResult.stdout.trim().startsWith("{")).toBe(true);
    expect(jsonResult.stdout).toContain('"server"');

    // Explicit --toon forces TOON regardless of pipe context.
    const toonResult = await bash.exec("alpha alpha-object --toon");
    expect(toonResult.exitCode).toBe(0);
    expect(toonResult.stdout.trim().startsWith("{")).toBe(false);
    expect(toonResult.stdout).toContain("alpha");
  } finally {
    await runtime.disconnect();
  }
});

test("TypeScript multi-server just-bash mode works with Python FastMCP e2e servers", async () => {
  const { createBashCommand, buildBashToolDescription } = await import("../src/bash_commands.js");
  const { installPipingHintPlugin } = await import("../src/just_bash_transform.js");
  const { Bash } = await import("just-bash");

  const resolvedBackends = resolveBackends(multiServerConfigJson(), "suite");
  const runtimes = [];
  const serverCmds = [];

  try {
    for (const resolved of resolvedBackends) {
      const runtime = createCompressorRuntime({
        backend: resolved.backend,
        compressionLevel: "low",
        serverName: resolved.serverName,
        toonify: true,
      });
      await runtime.connect();
      runtimes.push(runtime);

      const tools = await runtime.listUncompressedTools();
      const command = createBashCommand(runtime, tools);
      serverCmds.push({ serverName: resolved.serverName ?? "mcp", command, tools });
    }

    const allCommands = serverCmds.map((sc) => sc.command);
    const bash = new Bash({ customCommands: allCommands });
    installPipingHintPlugin(
      bash,
      allCommands.map((c) => c.name),
    );

    // Both servers should be available as parent commands with subcommands
    const addResult = await bash.exec("suite-alpha alpha-add --a 6 --b 7");
    expect(addResult.exitCode).toBe(0);
    expect(addResult.stdout).toBe("13");

    const multiplyResult = await bash.exec("suite-beta beta-multiply --a 6 --b 7");
    expect(multiplyResult.exitCode).toBe(0);
    expect(multiplyResult.stdout).toBe("42");

    // Description lists each server's top-level command + TOON note (no subcommands).
    const description = buildBashToolDescription(serverCmds);
    expect(description).toContain("- `suite-alpha`");
    expect(description).toContain("- `suite-beta`");
    expect(description).toContain("custom commands are installed");
    expect(description).toContain("TOON");
    expect(description).not.toContain("alpha-add");
    expect(description).not.toContain("beta-multiply");

    // Auto-toonification across both servers: unpiped is TOON, piped is JSON.
    for (const [server, value] of [
      ["suite-alpha", "alpha"],
      ["suite-beta", "beta"],
    ] as const) {
      // ``suite-alpha alpha-object`` / ``suite-beta beta-object``.
      const subcommand = `${value}-object`;
      const toonResult = await bash.exec(`${server} ${subcommand}`);
      expect(toonResult.exitCode).toBe(0);
      expect(toonResult.stdout.trim().startsWith("{")).toBe(false);
      expect(toonResult.stdout).toContain(value);

      const jqResult = await bash.exec(`${server} ${subcommand} | jq -r .server`);
      expect(jqResult.exitCode).toBe(0);
      expect(jqResult.stdout.trim()).toBe(value);
    }
  } finally {
    await Promise.allSettled(runtimes.map((r) => r.disconnect()));
  }
});

test("TypeScript multi-server CLI mode creates one script per Python FastMCP server", async () => {
  const session = await initializeCliMode({
    backend: multiServerConfigJson(),
    cliPort: 0,
    compressionLevel: "low",
    scriptDir: path.resolve(".."),
    toonify: true,
  });

  try {
    expect(session.runtimes.length).toBe(2);
    expect(session.scripts.map((script) => script.cliName).sort()).toEqual(["alpha", "beta"]);

    const alphaScript = session.scripts.find((script) => script.cliName === "alpha")!;
    const betaScript = session.scripts.find((script) => script.cliName === "beta")!;
    const alphaTools = await alphaScript.runtime.listUncompressedTools();
    const betaTools = await betaScript.runtime.listUncompressedTools();
    expect(alphaTools.map((tool) => tool.name).sort()).toEqual([
      "alpha_add",
      "alpha_echo",
      "alpha_object",
    ]);
    expect(betaTools.map((tool) => tool.name).sort()).toEqual([
      "beta_echo",
      "beta_multiply",
      "beta_object",
    ]);

    const alphaHelpResponse = await fetch(`${alphaScript.bridgeUrl}/help`);
    const betaHelpResponse = await fetch(`${betaScript.bridgeUrl}/help`);
    expect(alphaHelpResponse.status).toBe(200);
    expect(betaHelpResponse.status).toBe(200);
    expect(await alphaHelpResponse.text()).toMatch(/alpha-add/);
    expect(await betaHelpResponse.text()).toMatch(/beta-multiply/);

    const invokeResponse = await fetch(`${betaScript.bridgeUrl}/exec`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ argv: ["beta-multiply", "--a", "6", "--b", "7"] }),
    });
    expect(invokeResponse.status).toBe(200);
    expect(await invokeResponse.text()).toBe("42");
  } finally {
    await session.close();
  }
});
