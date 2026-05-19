import type { ExecutableTool } from "./adapters.js";
import {
  generateClientFromBridge,
  type GeneratedClientArtifactsResult,
} from "./generated_clients.js";
import {
  createJustBashCommandRegistrations,
  installJustBashRegistrations,
  type JustBashCommandRegistration,
  type JustBashCommandSource,
} from "./just_bash_commands.js";
import { startLocalToolBridge } from "./local_tool_bridge.js";
import { maybeToonifyOutput, type ClientArtifactKind } from "./rust_core.js";
import {
  executableToolToSpec,
  executableToolsToSpecs,
  normalizeServerName,
  stringifyToolResult,
} from "./tool_specs.js";

export interface TransformToolOptions {
  serverName?: string;
}

export interface TransformToolsForJustBashOptions extends TransformToolOptions {
  bash: unknown;
}

export interface JustBashTransformResult {
  tools: Record<string, ExecutableTool>;
  registrations: JustBashCommandRegistration[];
}

export type CodeTransformLanguage = "python" | "typescript";

export interface TransformToolsForCodeModeOptions extends TransformToolOptions {
  language: CodeTransformLanguage;
  outputDir?: string;
}

export interface TransformToolsForCliModeOptions extends TransformToolOptions {
  outputDir?: string;
}

export interface GeneratedToolTransformResult extends GeneratedClientArtifactsResult {
  tools: Record<string, ExecutableTool>;
  close(): void;
}

export function transformToolsForJustBash(
  tools: Record<string, ExecutableTool<unknown>>,
  options: TransformToolsForJustBashOptions,
): JustBashTransformResult {
  const serverName = normalizeServerName(options.serverName);
  const registrations = createJustBashCommandRegistrations(
    Object.entries(tools).map(([name, tool]) => justBashSource(serverName, name, tool)),
  );
  installJustBashRegistrations(options.bash, registrations);
  const helpDescription = justBashHelpDescription(serverName, registrations);
  return {
    registrations,
    tools: helpTools({
      serverName,
      description: helpDescription,
      output: helpDescription,
    }),
  };
}

export async function transformToolsForCodeMode(
  tools: Record<string, ExecutableTool<unknown>>,
  options: TransformToolsForCodeModeOptions,
): Promise<GeneratedToolTransformResult> {
  const serverName = normalizeServerName(options.serverName);
  return generatedTransform(tools, {
    kind: options.language,
    serverName,
    outputDir: options.outputDir ?? "./dist",
  });
}

export async function transformToolsForCliMode(
  tools: Record<string, ExecutableTool<unknown>>,
  options: TransformToolsForCliModeOptions = {},
): Promise<GeneratedToolTransformResult> {
  const serverName = normalizeServerName(options.serverName);
  const output =
    options.outputDir === undefined ? defaultCliOutputDir() : { outputDir: options.outputDir };
  return generatedTransform(tools, {
    kind: "cli",
    serverName,
    outputDir: output.outputDir,
    commandName: serverName,
  });
}

function justBashSource(
  serverName: string,
  name: string,
  tool: ExecutableTool<unknown>,
): JustBashCommandSource {
  return {
    providerName: serverName,
    commandName: `${serverName}_${name}`,
    backendToolName: name,
    helpToolName: `${serverName}_help`,
    tool: executableToolToSpec(name, tool),
    invoke: async (input) => maybeToonifyOutput(stringifyToolResult(await tool.execute(input))),
  };
}

async function generatedTransform(
  tools: Record<string, ExecutableTool<unknown>>,
  options: {
    kind: ClientArtifactKind;
    serverName: string;
    outputDir: string;
    commandName?: string;
  },
): Promise<GeneratedToolTransformResult> {
  const bridge = await startLocalToolBridge(tools);
  const generated = generateClientFromBridge({
    kind: options.kind,
    name: options.serverName,
    bridgeUrl: bridge.bridgeUrl,
    token: bridge.token,
    tools: executableToolsToSpecs(tools),
    outputDir: options.outputDir,
  });
  const helpDescription = generatedHelpDescription({
    kind: options.kind,
    serverName: options.serverName,
    outputDir: options.outputDir,
    files: Object.keys(generated.files),
    commandName: options.commandName,
  });
  return {
    ...generated,
    tools: helpTools({
      serverName: options.serverName,
      description: helpDescription,
      output: helpDescription,
    }),
    close: () => bridge.close(),
  };
}

function helpTools(options: {
  serverName: string;
  description: string;
  output: string;
}): Record<string, ExecutableTool> {
  const name = `${options.serverName}_help`;
  return {
    [name]: {
      name,
      description: options.description,
      inputSchema: { type: "object", properties: {} },
      execute: async () => options.output,
    },
  };
}

function generatedHelpDescription(options: {
  kind: ClientArtifactKind;
  serverName: string;
  outputDir: string;
  files: string[];
  commandName?: string;
}): string {
  if (options.kind === "cli") {
    const command = options.commandName ?? `${options.outputDir}/${options.serverName}`;
    return cliHelpDescription(command, options.serverName);
  }

  const language = options.kind === "python" ? "Python" : "TypeScript";
  const moduleName =
    options.kind === "python" ? `${options.serverName}.py` : `${options.serverName}.ts`;
  return [
    `Functionality associated with the ${options.serverName} toolset is provided via generated ${language} code. Do not call this tool - import and use the generated code instead.`,
    `${options.serverName} - the ${options.serverName} toolset`,
    "",
    `Generated files are available in ${options.outputDir}.`,
    `Primary module: ${options.outputDir}/${moduleName}`,
    "",
    "When relevant, outputs from generated clients will prefer using the TOON format for more efficient representation of data.",
    "",
    "Available generated files:",
    ...options.files.map((file) => `  - ${options.outputDir}/${file}`),
  ].join("\n");
}

function cliHelpDescription(command: string, cliName: string): string {
  return [
    `Functionality associated with the ${cliName} toolset is provided via the \`${command}\` CLI. Do not call this tool - use the CLI instead.`,
    `${cliName} - the ${cliName} toolset`,
    "",
    "When relevant, outputs from this CLI will prefer using the TOON format for more efficient representation of data.",
    "",
    "USAGE:",
    `  ${command} <subcommand> [options]`,
    "",
    "SUBCOMMANDS:",
    `  Run '${command} --help' in the shell for available subcommands.`,
    "",
    `Run '${command} --help' in the shell for usage.`,
    `Run '${command} <subcommand> --help' for per-command help.`,
    `Run '${command} <subcommand> [options]' to invoke a tool.`,
  ].join("\n");
}

function justBashHelpDescription(
  serverName: string,
  registrations: JustBashCommandRegistration[],
): string {
  return [
    `Functionality associated with the ${serverName} toolset is provided via bash commands. Do not call this tool - use the bash commands instead.`,
    `${serverName} - the ${serverName} toolset`,
    "",
    "When relevant, outputs from these commands will prefer using the TOON format for more efficient representation of data.",
    "",
    "COMMANDS:",
    ...registrations.map((registration) => `  ${registration.commandName}`),
    "",
    "Run these commands with the bash tool.",
  ].join("\n");
}

function defaultCliOutputDir(): { outputDir: string } {
  const envDir = process.env.MCP_COMPRESSOR_CLI_OUTPUT_DIR;
  if (envDir !== undefined && envDir.length > 0) {
    return { outputDir: envDir };
  }

  const home = process.env.HOME;
  if (home !== undefined && home.length > 0) {
    return { outputDir: `${home}/.local/bin` };
  }

  return { outputDir: "./dist" };
}
