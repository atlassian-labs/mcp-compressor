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
import type { ClientArtifactKind } from "./rust_core.js";
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
  tools: Record<string, ExecutableTool>,
  options: TransformToolsForJustBashOptions,
): JustBashTransformResult {
  const serverName = normalizeServerName(options.serverName);
  const registrations = createJustBashCommandRegistrations(
    Object.entries(tools).map(([name, tool]) => justBashSource(serverName, name, tool)),
  );
  installJustBashRegistrations(options.bash, registrations);
  return {
    registrations,
    tools: helpTools({
      serverName,
      mode: "Just Bash",
      summary: `Backend tools have been installed as Just Bash commands for ${serverName}.`,
      lines: registrations.map((registration) => `- ${registration.commandName}`),
    }),
  };
}

export async function transformToolsForCodeMode(
  tools: Record<string, ExecutableTool>,
  options: TransformToolsForCodeModeOptions,
): Promise<GeneratedToolTransformResult> {
  const serverName = normalizeServerName(options.serverName);
  return generatedTransform(tools, {
    kind: options.language,
    serverName,
    outputDir: options.outputDir ?? "./dist",
    modeLabel: options.language === "python" ? "Python Code Mode" : "TypeScript Code Mode",
  });
}

export async function transformToolsForCliMode(
  tools: Record<string, ExecutableTool>,
  options: TransformToolsForCliModeOptions = {},
): Promise<GeneratedToolTransformResult> {
  const serverName = normalizeServerName(options.serverName);
  return generatedTransform(tools, {
    kind: "cli",
    serverName,
    outputDir: options.outputDir ?? "./dist",
    modeLabel: "CLI Mode",
  });
}

function justBashSource(
  serverName: string,
  name: string,
  tool: ExecutableTool,
): JustBashCommandSource {
  return {
    providerName: serverName,
    commandName: `${serverName}_${name}`,
    backendToolName: name,
    helpToolName: `${serverName}_help`,
    tool: executableToolToSpec(name, tool),
    invoke: async (input) => stringifyToolResult(await tool.execute(input)),
  };
}

async function generatedTransform(
  tools: Record<string, ExecutableTool>,
  options: {
    kind: ClientArtifactKind;
    serverName: string;
    outputDir: string;
    modeLabel: string;
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
  return {
    ...generated,
    tools: helpTools({
      serverName: options.serverName,
      mode: options.modeLabel,
      summary: `${options.modeLabel} generated client files for ${options.serverName}.`,
      lines: Object.keys(generated.files).map((file) => `- ${options.outputDir}/${file}`),
    }),
    close: () => bridge.close(),
  };
}

function helpTools(options: {
  serverName: string;
  mode: string;
  summary: string;
  lines: string[];
}): Record<string, ExecutableTool> {
  const name = `${options.serverName}_help`;
  return {
    [name]: {
      name,
      description: `Show help for ${options.mode} tools generated from ${options.serverName}.`,
      inputSchema: { type: "object", properties: {} },
      execute: async () => [options.summary, "", ...options.lines].join("\n"),
    },
  };
}
