import type { ExecutableTool } from "./adapters.js";
import {
  generateClientFromBridge,
  type GeneratedBridgeClientArtifactsResult,
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

export interface GeneratedToolTransformResult extends GeneratedBridgeClientArtifactsResult {
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
  const helpDescription = shellToolHelpDescription({
    command: serverName,
    cliName: serverName,
    tools: executableToolsToSpecs(tools),
    commandNameForTool: cliSubcommandName,
  });
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
    tools: executableToolsToSpecs(tools),
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
  tools: ReturnType<typeof executableToolsToSpecs>;
}): string {
  if (options.kind === "cli") {
    const command = options.commandName ?? `${options.outputDir}/${options.serverName}`;
    return cliHelpDescription(command, options.serverName, options.tools);
  }

  return codeHelpDescription(options);
}

function cliHelpDescription(
  command: string,
  cliName: string,
  tools: ReturnType<typeof executableToolsToSpecs>,
): string {
  return shellToolHelpDescription({
    command,
    cliName,
    tools,
    commandNameForTool: cliSubcommandName,
  });
}

function shellToolHelpDescription(options: {
  command: string;
  cliName: string;
  tools: ReturnType<typeof executableToolsToSpecs>;
  commandNameForTool(toolName: string): string;
}): string {
  return [
    `Functionality associated with the ${options.cliName} toolset is provided via the \`${options.command}\` CLI. Do not call this tool - use the CLI instead.`,
    `${options.cliName} - the ${options.cliName} toolset`,
    "",
    "When relevant, outputs from this CLI will prefer using the TOON format for more efficient representation of data.",
    "",
    "USAGE:",
    `  ${options.command} <subcommand> [options]`,
    "",
    "SUBCOMMANDS:",
    ...formatSubcommands(options.tools, options.commandNameForTool),
    "",
    `Run '${options.command} --help' in the shell for usage.`,
    `Run '${options.command} <subcommand> --help' for per-command help.`,
    `Run '${options.command} <subcommand> [options]' to invoke a tool.`,
  ].join("\n");
}

function formatSubcommands(
  tools: ReturnType<typeof executableToolsToSpecs>,
  commandNameForTool: (toolName: string) => string,
): string[] {
  const maxNameLength = Math.max(...tools.map((tool) => commandNameForTool(tool.name).length), 0);
  return tools.map((tool) => {
    const subcommand = commandNameForTool(tool.name);
    const description = compactDescription(tool.description ?? undefined);
    return `  ${subcommand.padEnd(maxNameLength + 2)}${description}`.trimEnd();
  });
}

function cliSubcommandName(toolName: string): string {
  return toolName.replaceAll("_", "-");
}

function compactDescription(description?: string): string {
  return (description ?? "").replace(/\s+/g, " ").trim();
}

function codeHelpDescription(options: {
  kind: ClientArtifactKind;
  serverName: string;
  outputDir: string;
  files: string[];
  tools: ReturnType<typeof executableToolsToSpecs>;
}): string {
  const language = options.kind === "python" ? "Python" : "TypeScript";
  const languageLower = options.kind === "python" ? "python" : "typescript";
  const moduleName =
    options.kind === "python" ? `${options.serverName}.py` : `${options.serverName}.ts`;
  const functions = formatCodeFunctions(options.kind, options.tools);
  const details =
    options.kind === "python"
      ? [
          "For details on a specific function, run:",
          "```python",
          `from ${options.serverName} import <function>`,
          "print(help(<function>))",
          "```",
        ]
      : [
          "For details on a specific function, inspect the generated TypeScript declarations or editor hover documentation.",
          `Primary declarations: ${options.outputDir}/${options.serverName}.d.ts`,
        ];
  return [
    `Functionality associated with the ${options.serverName} toolset is provided via a ${language} module. Do not call this tool - import and use the ${languageLower} functionality instead.`,
    `${options.serverName} - the ${options.serverName} toolset`,
    "",
    `${language} source code is available in ${options.outputDir}/${moduleName}`,
    "",
    "Available functions:",
    ...functions,
    "",
    ...details,
  ].join("\n");
}

function formatCodeFunctions(
  kind: ClientArtifactKind,
  tools: ReturnType<typeof executableToolsToSpecs>,
): string[] {
  const names = tools.map((tool) => (kind === "python" ? tool.name : snakeToCamel(tool.name)));
  const maxNameLength = Math.max(...names.map((name) => name.length), 0);
  return tools.map((tool, index) => {
    const name = names[index] ?? tool.name;
    return `  ${name.padEnd(maxNameLength + 2)}${compactDescription(tool.description ?? undefined)}`.trimEnd();
  });
}

function snakeToCamel(name: string): string {
  return name.replace(/[_-]([a-zA-Z0-9])/g, (_match, char: string) => char.toUpperCase());
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
