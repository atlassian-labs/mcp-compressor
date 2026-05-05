import { loadNativeCore, type NativeRustTool } from "./native.js";

export interface RustTool {
  name: string;
  description?: string | null;
  inputSchema: Record<string, unknown>;
}

function toNativeTool(tool: RustTool): NativeRustTool {
  return {
    name: tool.name,
    description: tool.description ?? null,
    input_schema: tool.inputSchema,
  };
}

function stringify(value: unknown): string {
  return JSON.stringify(value);
}

export function compressToolListing(level: string, tools: RustTool[]): string {
  return loadNativeCore().compressToolListingJson(level, stringify(tools.map(toNativeTool)));
}

export function formatToolSchemaResponse(tool: RustTool): string {
  return loadNativeCore().formatToolSchemaResponseJson(stringify(toNativeTool(tool)));
}

export function parseToolArgv(tool: RustTool, argv: string[]): Record<string, unknown> {
  return JSON.parse(
    loadNativeCore().parseToolArgvJson(stringify(toNativeTool(tool)), stringify(argv)),
  ) as Record<string, unknown>;
}

export type ClientArtifactKind = "cli" | "python" | "typescript";

export interface ClientGeneratorConfig {
  cliName: string;
  bridgeUrl: string;
  token: string;
  tools: RustTool[];
  sessionPid: number;
  outputDir: string;
}

function toNativeGeneratorConfig(config: ClientGeneratorConfig): Record<string, unknown> {
  return {
    cli_name: config.cliName,
    bridge_url: config.bridgeUrl,
    token: config.token,
    tools: config.tools.map(toNativeTool),
    session_pid: config.sessionPid,
    output_dir: config.outputDir,
  };
}

export function generateClientArtifacts(
  kind: ClientArtifactKind,
  config: ClientGeneratorConfig,
): string[] {
  return JSON.parse(
    loadNativeCore().generateClientArtifactsJson(kind, stringify(toNativeGeneratorConfig(config))),
  ) as string[];
}

export interface ParsedMcpServer {
  name: string;
  command: string;
  args: string[];
  env: Array<[string, string]>;
  cli_prefix: string;
}

export function parseMcpConfig(configJson: string): ParsedMcpServer[] {
  return JSON.parse(loadNativeCore().parseMcpConfigJson(configJson)) as ParsedMcpServer[];
}

export interface OAuthStoreEntry {
  backend_name: string;
  backend_uri: string;
  store_dir: string;
}

export function listOAuthCredentials(): OAuthStoreEntry[] {
  return JSON.parse(loadNativeCore().listOauthCredentialsJson()) as OAuthStoreEntry[];
}

export function clearOAuthCredentials(target?: string | null): string[] {
  return JSON.parse(loadNativeCore().clearOauthCredentialsJson(target ?? null)) as string[];
}
