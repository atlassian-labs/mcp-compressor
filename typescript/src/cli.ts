#!/usr/bin/env node
import { Command, Option } from "commander";

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { VERSION } from "./version.js";

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

function parseClearOAuthArgs(args: string[]): { target?: string; all: boolean } | null {
  if (args[0] !== "clear-oauth") {
    return null;
  }

  const clearProgram = new Command()
    .name("mcp-compressor clear-oauth")
    .exitOverride()
    .allowExcessArguments(false)
    .argument("[backend]", "backend URL or MCP config JSON string")
    .option("--all", "also remove the encryption key");
  clearProgram.parse(args.slice(1), { from: "user" });
  const target = clearProgram.args[0] as string | undefined;
  const options = clearProgram.opts<{ all?: boolean }>();
  return { ...(target ? { target } : {}), all: options.all ?? false };
}

function candidateCoreBinaries(): string[] {
  const candidates: string[] = [];
  if (process.env.MCP_COMPRESSOR_CORE_BINARY) {
    candidates.push(process.env.MCP_COMPRESSOR_CORE_BINARY);
  }
  candidates.push("mcp-compressor-core");
  const here = dirname(fileURLToPath(import.meta.url));
  candidates.push(
    join(
      here,
      "..",
      "..",
      "target",
      "debug",
      process.platform === "win32" ? "mcp-compressor-core.exe" : "mcp-compressor-core",
    ),
  );
  candidates.push(
    join(
      process.cwd(),
      "..",
      "target",
      "debug",
      process.platform === "win32" ? "mcp-compressor-core.exe" : "mcp-compressor-core",
    ),
  );
  return candidates;
}

function translateArgsForRust(args: string[]): string[] {
  const clearOAuth = parseClearOAuthArgs(args);
  if (clearOAuth) {
    return clearOAuth.target ? ["clear-oauth", clearOAuth.target] : ["clear-oauth"];
  }
  return args;
}

async function runRustCoreCli(args: string[]): Promise<number> {
  for (const binary of candidateCoreBinaries()) {
    if (binary !== "mcp-compressor-core" && !existsSync(binary)) {
      continue;
    }
    const child = spawn(binary, translateArgsForRust(args), { stdio: "inherit" });
    return await new Promise((resolve, reject) => {
      child.on("error", (error: NodeJS.ErrnoException) => {
        if (error.code === "ENOENT") {
          resolve(127);
          return;
        }
        reject(error);
      });
      child.on("exit", (code, signal) => {
        if (signal) {
          resolve(1);
          return;
        }
        resolve(code ?? 0);
      });
    });
  }
  console.error(
    "mcp-compressor-core binary was not found. Build it with `cargo build -p mcp-compressor-core` or set MCP_COMPRESSOR_CORE_BINARY.",
  );
  return 127;
}

async function main(): Promise<void> {
  const exitCode = await runRustCoreCli(process.argv.slice(2));
  process.exitCode = exitCode;
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? (error.stack ?? error.message) : String(error));
  process.exitCode = 1;
});
