import {
  generateClientArtifactFiles,
  generateClientArtifacts,
  type ClientArtifactKind,
  type ClientGeneratorConfig,
  type ToolSpec,
} from "./rust_core.js";

export interface GeneratedClientArtifactsResult {
  paths: string[];
  files: Record<string, string>;
  environment: Record<string, string>;
}

export interface GenerateClientFromBridgeOptions {
  kind: ClientArtifactKind;
  name: string;
  bridgeUrl: string;
  token: string;
  tools: ToolSpec[];
  outputDir: string;
  sessionPid?: number;
}

export function generateClientFromBridge(
  options: GenerateClientFromBridgeOptions,
): GeneratedClientArtifactsResult {
  const config: ClientGeneratorConfig = {
    cliName: options.name,
    bridgeUrl: options.bridgeUrl,
    token: options.token,
    tools: options.tools,
    outputDir: options.outputDir,
    sessionPid: options.sessionPid ?? 0,
  };
  return {
    paths: generateClientArtifacts(options.kind, config),
    files: generateClientArtifactFiles(options.kind, config),
    environment: generatedClientEnvironment(options.kind, options.outputDir),
  };
}

export function generatedClientEnvironment(
  kind: ClientArtifactKind,
  outputDir: string,
): Record<string, string> {
  if (kind === "python") return { PYTHONPATH: outputDir };
  if (kind === "cli") return { PATH: `${outputDir}:$PATH` };
  return {};
}
