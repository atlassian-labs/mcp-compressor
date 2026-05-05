import { loadNativeCore, type NativeCompressedSession, type NativeRustTool } from "./native.js";

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

export interface BackendConfig {
  name: string;
  commandOrUrl: string;
  args?: string[];
}

export interface CompressedSessionConfig {
  compressionLevel: string;
  serverName?: string | null;
  includeTools?: string[];
  excludeTools?: string[];
  toonify?: boolean;
  transformMode?: string | null;
}

export interface JustBashCommandSpec {
  command_name: string;
  tool_name: string;
  description?: string | null;
  input_schema: Record<string, unknown>;
  invoke_tool_name: string;
}

export interface JustBashProviderSpec {
  provider_name: string;
  help_tool_name: string;
  tools: JustBashCommandSpec[];
}

export interface CompressedSessionInfo {
  bridge_url: string;
  token: string;
  frontend_tools: Array<{
    name: string;
    description?: string | null;
    input_schema: Record<string, unknown>;
  }>;
  just_bash_providers: JustBashProviderSpec[];
}

export class CompressedSession {
  constructor(private readonly nativeSession: NativeCompressedSession) {}

  info(): CompressedSessionInfo {
    return JSON.parse(this.nativeSession.infoJson()) as CompressedSessionInfo;
  }
}

function toNativeSessionConfig(config: CompressedSessionConfig): Record<string, unknown> {
  return {
    compression_level: config.compressionLevel,
    server_name: config.serverName ?? null,
    include_tools: config.includeTools ?? [],
    exclude_tools: config.excludeTools ?? [],
    toonify: config.toonify ?? false,
    transform_mode: config.transformMode ?? null,
  };
}

function toNativeBackendConfig(backend: BackendConfig): Record<string, unknown> {
  return {
    name: backend.name,
    command_or_url: backend.commandOrUrl,
    args: backend.args ?? [],
  };
}

export async function startCompressedSession(
  config: CompressedSessionConfig,
  backends: BackendConfig[],
): Promise<CompressedSession> {
  const session = await loadNativeCore().startCompressedSessionJson(
    stringify(toNativeSessionConfig(config)),
    stringify(backends.map(toNativeBackendConfig)),
  );
  return new CompressedSession(session);
}

export async function startCompressedSessionFromMcpConfig(
  config: CompressedSessionConfig,
  mcpConfigJson: string,
): Promise<CompressedSession> {
  const session = await loadNativeCore().startCompressedSessionFromMcpConfigJson(
    stringify(toNativeSessionConfig(config)),
    mcpConfigJson,
  );
  return new CompressedSession(session);
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
