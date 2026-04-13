#!/usr/bin/env node
import { Command, Option } from "commander";

import { VERSION } from "./version.js";

import {
  clearAllOAuth,
  clearOAuth,
  initializeCliMode,
  resolveBackends,
  startCompressorServer,
  startMultipleCompressorServers,
} from "./index.js";
import type { BackendConfig } from "./types.js";

function parseBackendArg(backendArgs: string[]): BackendConfig | string {
  if (backendArgs.length === 0) {
    throw new Error("Expected a backend URL, MCP config JSON string, or stdio command.");
  }
  if (backendArgs.length === 1) {
    return backendArgs[0]!;
  }
  return {
    type: "stdio",
    command: backendArgs[0]!,
    args: backendArgs.slice(1),
  };
}

export interface ParsedCliArgs {
  backend: BackendConfig | string;
  justBash: boolean;
  cliMode: boolean;
  cliPort: string | undefined;
  compressionLevel: "low" | "medium" | "high" | "max" | undefined;
  cwd: string | undefined;
  env: Record<string, string>;
  excludeTools: string[];
  headers: Record<string, string>;
  includeTools: string[];
  logLevel: string;
  serverName: string | undefined;
  timeout: number;
  toonify: boolean;
}

function collect(value: string, previous: string[]): string[] {
  previous.push(value);
  return previous;
}

function collectKeyValue(value: string, previous: Record<string, string>): Record<string, string> {
  const eqIndex = value.indexOf("=");
  if (eqIndex === -1) {
    throw new Error(`Invalid key=value format: '${value}'. Expected KEY=VALUE.`);
  }
  const key = value.slice(0, eqIndex);
  let val = value.slice(eqIndex + 1);
  // Support ${VAR_NAME} environment variable expansion
  val = val.replace(/\$\{(\w+)\}/g, (_, name) => process.env[name] ?? "");
  previous[key] = val;
  return previous;
}

function buildProgram(options: { exitOverride?: boolean } = {}): Command {
  const program = new Command()
    .name("mcp-compressor")
    .description(
      "Run the MCP Compressor proxy server.\n\n" +
        "Connects to an MCP server (via stdio, HTTP, or SSE) and wraps it\n" +
        "with a compressed tool interface.",
    )
    .allowUnknownOption(false)
    .allowExcessArguments(true)
    .version(VERSION, "-V, --version")
    .argument(
      "[command_or_url...]",
      "The backend to wrap: either a remote MCP URL, a stdio command plus\n" +
        "arguments, or an MCP config JSON string with one or more servers.\n" +
        "Example stdio usage: bun run dist/cli.js -- uvx mcp-server-fetch",
    )
    .option("--cwd <dir>", "The working directory to use when running stdio MCP servers.")
    .option(
      "-e, --env <VAR=VALUE>",
      "Environment variables to set when running stdio MCP servers, in the\n" +
        "form VAR_NAME=VALUE. Can be used multiple times. Supports environment\n" +
        "variable expansion with ${VAR_NAME} syntax.",
      collectKeyValue,
      {},
    )
    .option(
      "-H, --header <NAME=VALUE>",
      "Headers to use for remote (HTTP/SSE) MCP server connections, in the\n" +
        "form Header-Name=Header-Value. Can be used multiple times. Supports\n" +
        "environment variable expansion with ${VAR_NAME} syntax.",
      collectKeyValue,
      {},
    )
    .option(
      "-t, --timeout <seconds>",
      "The timeout in seconds for connecting to the MCP server and making requests.",
      "10",
    )
    .addOption(
      new Option(
        "-c, --compression-level <level>",
        "The level of compression to apply to the tool descriptions of the wrapped MCP server.",
      )
        .choices(["low", "medium", "high", "max"])
        .default("medium"),
    )
    .option(
      "-n, --server-name <name>",
      "Optional custom name to prefix the wrapper tool names (get_tool_schema,\n" +
        "invoke_tool, list_tools). The name will be sanitized to conform to MCP\n" +
        "tool name specifications (only A-Z, a-z, 0-9, _, -, .).",
    )
    .option("-l, --log-level <level>", "The logging level.", "error")
    .option("--toonify", "Convert JSON tool responses to TOON format automatically.")
    .option(
      "--cli-mode",
      "Start in CLI mode: expose a single help MCP tool, start a local HTTP\n" +
        "bridge, and generate a shell script for interacting with the wrapped\n" +
        "server via CLI. --toonify is automatically enabled in this mode.",
    )
    .option(
      "--cli-port <port>",
      "Port for the local CLI bridge HTTP server (default: random free port).",
    )
    .option(
      "--just-bash",
      "Start in just-bash mode: expose a single 'bash' MCP tool powered by\n" +
        "just-bash, with all backend server tools available as custom commands.\n" +
        "Requires the 'just-bash' package to be installed. --toonify is\n" +
        "automatically enabled in this mode.",
    )
    .option(
      "--include-tool <tool>",
      "Wrapped server tool name to expose. Can be used multiple times.\n" +
        "If omitted, all tools are included.",
      collect,
      [],
    )
    .option(
      "--exclude-tool <tool>",
      "Wrapped server tool name to hide. Can be used multiple times.",
      collect,
      [],
    );

  if (options.exitOverride) {
    program.exitOverride();
  }
  return program;
}

function parseCliArgsWithOptions(
  argv: string[],
  parseOptions: { exitOverride?: boolean } = {},
): ParsedCliArgs {
  const program = buildProgram(parseOptions);
  program.parse(argv, { from: "user" });
  const parsedOptions = program.opts<{
    justBash?: boolean;
    cliMode?: boolean;
    cliPort?: string;
    compressionLevel?: ParsedCliArgs["compressionLevel"];
    cwd?: string;
    env?: Record<string, string>;
    excludeTool?: string[];
    header?: Record<string, string>;
    includeTool?: string[];
    logLevel: string;
    serverName?: string;
    timeout: string;
    toonify?: boolean;
  }>();
  const backend = parseBackendArg(program.args);
  const justBash = parsedOptions.justBash ?? false;
  const cliMode = parsedOptions.cliMode ?? false;
  const toonify = (parsedOptions.toonify ?? false) || cliMode || justBash;

  return {
    backend,
    justBash,
    cliMode,
    cliPort: parsedOptions.cliPort,
    compressionLevel: parsedOptions.compressionLevel,
    cwd: parsedOptions.cwd,
    env: parsedOptions.env ?? {},
    excludeTools: parsedOptions.excludeTool ?? [],
    headers: parsedOptions.header ?? {},
    includeTools: parsedOptions.includeTool ?? [],
    logLevel: parsedOptions.logLevel,
    serverName: parsedOptions.serverName,
    timeout: Number.parseFloat(parsedOptions.timeout),
    toonify,
  };
}

export function parseCliArgs(argv: string[]): ParsedCliArgs {
  return parseCliArgsWithOptions(argv, { exitOverride: true });
}

async function handleClearOAuth(args: string[]): Promise<boolean> {
  if (args[0] !== "clear-oauth") {
    return false;
  }

  const clearProgram = new Command()
    .name("mcp-compressor clear-oauth")
    .exitOverride()
    .allowExcessArguments(false)
    .argument("[backend]", "backend URL or MCP config JSON string")
    .option("--all", "also remove the encryption key");
  clearProgram.parse(args.slice(1), { from: "user" });
  const backend = clearProgram.args[0];
  const options = clearProgram.opts<{ all?: boolean }>();

  if (backend) {
    const cleared = await clearOAuth(backend);
    if (!cleared) {
      console.warn("No OAuth state applies to that backend.");
    }
    return true;
  }

  const removed = await clearAllOAuth({ all: options.all ?? false });
  if (removed.length > 0) {
    const removedKey = removed.some((entry) => entry.endsWith("/.key") || entry.endsWith("\\.key"));
    const removedStateFiles = removed.length - (removedKey ? 1 : 0);
    console.info(
      `Removed ${removedStateFiles} OAuth state file(s)${removedKey ? " and encryption key" : ""}.`,
    );
    console.info(
      "OAuth credentials cleared. You will be prompted to authenticate on next connection.",
    );
  } else {
    console.warn("No stored OAuth credentials found.");
  }
  return true;
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);
  if (await handleClearOAuth(args)) {
    return;
  }

  const {
    backend,
    justBash,
    cliMode,
    cliPort,
    compressionLevel,
    cwd,
    env,
    excludeTools,
    headers,
    includeTools,
    logLevel,
    serverName,
    timeout,
    toonify,
  } = parseCliArgsWithOptions(args);

  // Enrich the backend config with CLI transport options (--cwd, --env, --header, --timeout)
  function enrichBackend(b: BackendConfig | string): BackendConfig | string {
    if (typeof b === "string") {
      return b; // JSON config string — transport options go inside the JSON
    }
    if (b.type === "stdio") {
      return {
        ...b,
        ...(cwd ? { cwd } : {}),
        ...(Object.keys(env).length > 0 ? { env: { ...b.env, ...env } } : {}),
      };
    }
    // HTTP or SSE
    return {
      ...b,
      ...(Object.keys(headers).length > 0 ? { headers: { ...b.headers, ...headers } } : {}),
      ...(timeout !== 10 ? { timeoutMs: timeout * 1000 } : {}),
    };
  }

  const enrichedBackend = enrichBackend(backend);

  if (logLevel !== "error") {
    console.warn(
      `[mcp-compressor-ts] log-level ${logLevel} requested; detailed logging is not implemented yet.`,
    );
  }

  if (justBash) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    let Bash: any;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    let bashCommandsModule: any;
    try {
      ({ Bash } = await import("just-bash"));
      bashCommandsModule = await import("./bash_commands.js");
    } catch {
      throw new Error(
        "Bash mode requires the 'just-bash' package. Install it with: npm install just-bash",
      );
    }

    const resolvedBackends = resolveBackends(enrichedBackend, serverName);
    const { createCompressorRuntime } = await import("./index.js");
    const runtimes: Awaited<ReturnType<typeof createCompressorRuntime>>[] = [];
    const serverCmds: Array<{
      serverName: string;
      command: { name: string };
      tools: unknown[];
    }> = [];

    for (const resolved of resolvedBackends) {
      const runtime = createCompressorRuntime({
        backend: resolved.backend,
        compressionLevel,
        excludeTools: excludeTools.length > 0 ? excludeTools : undefined,
        includeTools: includeTools.length > 0 ? includeTools : undefined,
        serverName: resolved.serverName,
        toonify,
      });
      await runtime.connect();
      runtimes.push(runtime);

      const tools = await runtime.listUncompressedTools();
      const command = bashCommandsModule.createBashCommand(runtime, tools);
      serverCmds.push({ serverName: resolved.serverName ?? "mcp", command, tools });
    }

    const allCommands = serverCmds.map((sc) => sc.command);
    const { ReadWriteFs } = await import("just-bash");
    const bash = new Bash({
      customCommands: allCommands,
      fs: new ReadWriteFs({ root: process.cwd() }),
      cwd: "/",
      python: true,
      javascript: true,
    });
    const description = bashCommandsModule.buildBashToolDescription(serverCmds) as string;

    // Serve as a single "bash" MCP tool via FastMCP
    const { FastMCP } = await import("fastmcp");
    const { z } = await import("zod");
    const mcp = new FastMCP({ name: "mcp-compressor-bash", version: "1.0.0" });
    mcp.addTool({
      name: "bash",
      description,
      parameters: z.object({
        command: z.string().describe("The bash command to execute."),
      }),
      execute: async (args: { command: string }) => {
        const result = await bash.exec(args.command);
        if (result.exitCode !== 0) {
          return `Exit code: ${result.exitCode}\n${result.stdout}${result.stderr ? `\nSTDERR: ${result.stderr}` : ""}`;
        }
        return result.stdout || "(no output)";
      },
    });

    console.error("Bash mode active.");
    console.error(`Available commands: ${allCommands.map((c) => c.name).join(", ")}`);

    const shutdown = async () => {
      await Promise.allSettled(runtimes.map((r) => r.disconnect()));
      process.exit(0);
    };
    process.once("SIGINT", () => void shutdown());
    process.once("SIGTERM", () => void shutdown());

    await mcp.start({ transportType: "stdio" });
    return;
  }

  if (cliMode) {
    const session = await initializeCliMode({
      backend: enrichedBackend,
      cliPort: cliPort ? Number.parseInt(cliPort, 10) : undefined,
      compressionLevel,
      excludeTools: excludeTools.length > 0 ? excludeTools : undefined,
      includeTools: includeTools.length > 0 ? includeTools : undefined,
      serverName,
      toonify,
    });

    console.info("CLI mode active.");
    for (const script of session.scripts) {
      const invoke = script.onPath ? script.cliName : `./${script.cliName}`;
      console.info(`Generated CLI: ${script.scriptPath ?? "(no script)"}`);
      console.info(`Run '${invoke} --help' for usage.`);
    }

    const shutdown = async () => {
      await session.close();
      process.exit(0);
    };
    process.once("SIGINT", () => void shutdown());
    process.once("SIGTERM", () => void shutdown());

    await new Promise(() => {
      // keep the bridge/runtime process alive
    });
    return;
  }

  const resolvedBackends = resolveBackends(enrichedBackend, serverName);
  if (resolvedBackends.length > 1) {
    await startMultipleCompressorServers({
      backends: resolvedBackends.map((r) => ({ backend: r.backend, serverName: r.serverName! })),
      compressionLevel,
      excludeTools: excludeTools.length > 0 ? excludeTools : undefined,
      includeTools: includeTools.length > 0 ? includeTools : undefined,
      toonify,
      start: { transportType: "stdio" },
    });
    return;
  }

  await startCompressorServer({
    backend: enrichedBackend,
    compressionLevel,
    excludeTools: excludeTools.length > 0 ? excludeTools : undefined,
    includeTools: includeTools.length > 0 ? includeTools : undefined,
    serverName,
    start: { transportType: "stdio" },
    toonify,
  });
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? (error.stack ?? error.message) : String(error));
  process.exitCode = 1;
});
