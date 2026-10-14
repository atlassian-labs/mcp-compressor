import { shrinkfitTools, type HostShrinkfitOptions } from "./host_shrinkfit.js";
import type { ExecutableTool } from "./adapters.js";
import {
  createJustBashCommandRegistrations,
  installJustBashRegistrations,
  type JustBashCommandRegistration,
  type JustBashCommandSource,
} from "./just_bash_commands.js";
import { startLocalToolBridge } from "./local_tool_bridge.js";
import {
  buildHostTransformPlan,
  normalizeHostToolResult,
  type ClientArtifactKind,
} from "./rust_core.js";
import { executableToolToSpec, executableToolsToSpecs, normalizeServerName } from "./tool_specs.js";

export interface TransformToolOptions extends HostShrinkfitOptions {
  serverName?: string;
}

export interface PlanToolsForJustBashOptions extends TransformToolOptions {
  /**
   * Convert JSON/CSV/YAML tool results to TOON before writing them to stdout.
   * Defaults to ``false`` so Just Bash output matches the generated CLI byte for
   * byte (and stays pipeable into tools such as ``jq``).
   */
  toonify?: boolean;
}

export interface TransformToolsForJustBashOptions extends PlanToolsForJustBashOptions {
  bash: unknown;
}

export interface JustBashTransformResult {
  close(): void;
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

export interface GeneratedToolTransformResult {
  paths: string[];
  files: Record<string, string>;
  environment: Record<string, string>;
  bridgeUrl: string;
  token: string;
  tools: Record<string, ExecutableTool>;
  close(): void;
}

export function planToolsForJustBash(
  tools: Record<string, ExecutableTool<unknown>>,
  options: PlanToolsForJustBashOptions = {},
): JustBashTransformResult {
  const serverName = normalizeServerName(options.serverName);
  const shrinking = shrinkfitTools(tools, options);
  tools = shrinking.tools;
  const toolSpecs = executableToolsToSpecs(tools);
  const plan = buildHostTransformPlan({ kind: "just-bash", serverName, tools: toolSpecs });
  const registrations = createJustBashCommandRegistrations(
    (plan.justBash?.commands ?? []).map((command) =>
      justBashSource(
        serverName,
        command.commandName,
        command.backendToolName,
        tools,
        options.shrinkfit === undefined && (options.toonify ?? false),
      ),
    ),
  );
  return {
    registrations,
    close: () => shrinking.close(),
    tools: helpTools({
      name: plan.helpToolName,
      description: plan.helpDescription,
      output: plan.helpDescription,
    }),
  };
}

export function transformToolsForJustBash(
  tools: Record<string, ExecutableTool<unknown>>,
  options: TransformToolsForJustBashOptions,
): JustBashTransformResult {
  const result = planToolsForJustBash(tools, options);
  installJustBashRegistrations(options.bash, result.registrations);
  return result;
}

export async function transformToolsForCodeMode(
  tools: Record<string, ExecutableTool<unknown>>,
  options: TransformToolsForCodeModeOptions,
): Promise<GeneratedToolTransformResult> {
  const serverName = normalizeServerName(options.serverName);
  return generatedTransform(tools, {
    shrinkfit: options.shrinkfit,
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
  return generatedTransform(tools, {
    shrinkfit: options.shrinkfit,
    kind: "cli",
    serverName,
    outputDir: options.outputDir,
    commandName: serverName,
  });
}

function justBashSource(
  serverName: string,
  commandName: string,
  backendToolName: string,
  tools: Record<string, ExecutableTool<unknown>>,
  toonify: boolean,
): JustBashCommandSource {
  const tool = tools[backendToolName];
  if (tool === undefined) {
    throw new Error(`missing Just Bash backend tool: ${backendToolName}`);
  }
  return {
    providerName: serverName,
    commandName,
    backendToolName,
    helpToolName: `${serverName}_help`,
    tool: executableToolToSpec(backendToolName, tool),
    invoke: async (input) => normalizeHostToolResult(await tool.execute(input), toonify),
  };
}

async function generatedTransform(
  tools: Record<string, ExecutableTool<unknown>>,
  options: {
    shrinkfit?: number;
    kind: ClientArtifactKind;
    serverName: string;
    outputDir?: string;
    commandName?: string;
  },
): Promise<GeneratedToolTransformResult> {
  const shrinking = shrinkfitTools(tools, options);
  tools = shrinking.tools;
  const bridge = await startLocalToolBridge(tools);
  const plan = buildHostTransformPlan({
    kind: options.kind,
    serverName: options.serverName,
    tools: executableToolsToSpecs(tools),
    outputDir: options.outputDir,
    commandName: options.commandName,
    bridgeUrl: bridge.bridgeUrl,
    token: bridge.token,
  });
  return {
    paths: plan.paths,
    files: plan.files,
    environment: plan.environment,
    bridgeUrl: bridge.bridgeUrl,
    token: bridge.token,
    tools: helpTools({
      name: plan.helpToolName,
      description: plan.helpDescription,
      output: plan.helpDescription,
    }),
    close: () => {
      shrinking.close();
      bridge.close();
    },
  };
}

function helpTools(options: {
  name: string;
  description: string;
  output: string;
}): Record<string, ExecutableTool> {
  return {
    [options.name]: {
      name: options.name,
      description: options.description,
      inputSchema: { type: "object", properties: {} },
      execute: async () => options.output,
    },
  };
}
