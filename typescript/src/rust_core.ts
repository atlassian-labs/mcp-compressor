import { loadNativeCore, type NativeCompressedSession, type NativeToolSpec } from "./native.js";

export interface ToolSpec {
  name: string;
  description?: string | null;
  inputSchema: Record<string, unknown>;
}

function toNativeTool(tool: ToolSpec): NativeToolSpec {
  return {
    name: tool.name,
    description: tool.description ?? null,
    input_schema: tool.inputSchema,
  };
}

function stringify(value: unknown): string {
  return JSON.stringify(value);
}

export function compressToolListing(level: string, tools: ToolSpec[]): string {
  return loadNativeCore().compressToolListingJson(level, stringify(tools.map(toNativeTool)));
}

export function formatToolSchemaResponse(tool: ToolSpec): string {
  return loadNativeCore().formatToolSchemaResponseJson(stringify(toNativeTool(tool)));
}

export function maybeToonifyOutput(output: string): string {
  return loadNativeCore().maybeToonifyOutputJson(output);
}

export function parseToolArgv(tool: ToolSpec, argv: string[]): Record<string, unknown> {
  return JSON.parse(
    loadNativeCore().parseToolArgvJson(stringify(toNativeTool(tool)), stringify(argv)),
  ) as Record<string, unknown>;
}

export type ClientArtifactKind = "cli" | "python" | "typescript";

export interface ClientGeneratorConfig {
  cliName: string;
  bridgeUrl: string;
  token: string;
  tools: ToolSpec[];
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

export function generateClientArtifactFiles(
  kind: ClientArtifactKind,
  config: ClientGeneratorConfig,
): Record<string, string> {
  return JSON.parse(
    loadNativeCore().generateClientArtifactFilesJson(
      kind,
      stringify(toNativeGeneratorConfig(config)),
    ),
  ) as Record<string, string>;
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
  tool_name?: string;
  backend_tool_name?: string;
  backendToolName?: string;
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
  backend_tools: Array<{
    name: string;
    description?: string | null;
    input_schema: Record<string, unknown>;
  }>;
  backend_tools_by_server: Array<{
    server_name?: string;
    serverName?: string;
    tool: {
      name: string;
      description?: string | null;
      input_schema: Record<string, unknown>;
    };
  }>;
  just_bash_providers: JustBashProviderSpec[];
}

export class CompressedSession {
  constructor(private readonly nativeSession: NativeCompressedSession) {}

  info(): CompressedSessionInfo {
    return JSON.parse(this.nativeSession.infoJson()) as CompressedSessionInfo;
  }

  close(): void {
    this.nativeSession.close();
  }

  updateAuthProviderHeaders(providerIndex: number, headers: Record<string, string>): void {
    this.nativeSession.updateAuthProviderHeadersJson(providerIndex, stringify(headers));
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

export interface ProviderBackendConfig extends BackendConfig {
  providerIndex?: number;
}

export async function startCompressedSessionWithAuthProviders(
  config: CompressedSessionConfig,
  backends: ProviderBackendConfig[],
  providers: Array<() => Record<string, string> | Promise<Record<string, string>>>,
): Promise<CompressedSession> {
  const nativeBackends = backends.map((backend) => ({
    ...toNativeBackendConfig(backend),
    provider_index: backend.providerIndex ?? null,
  }));
  const initialHeaders = await Promise.all(providers.map((provider) => provider()));
  const session = await loadNativeCore().startCompressedSessionWithProviderBackendsJson(
    stringify(toNativeSessionConfig(config)),
    stringify(nativeBackends),
    stringify(initialHeaders),
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

export function normalizeSdkServers(servers: unknown): BackendConfig[] {
  const raw = JSON.parse(loadNativeCore().normalizeServersJson(stringify(servers))) as Array<{
    name: string;
    command_or_url: string;
    args?: string[];
    oauth_app_name?: string | null;
  }>;
  return raw.map((backend) => ({
    name: backend.name,
    commandOrUrl: backend.command_or_url,
    args: backend.args ?? [],
    ...(backend.oauth_app_name == null ? {} : { oauth_app_name: backend.oauth_app_name }),
  }));
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

export function rememberOAuthBackend(
  backendUri: string,
  backendName: string,
  storeDir: string,
): void {
  loadNativeCore().rememberOauthBackendJson(backendUri, backendName, storeDir);
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

export interface HostTransformPlanConfig {
  kind: ClientArtifactKind | "just-bash";
  serverName: string;
  tools: ToolSpec[];
  outputDir?: string;
  commandName?: string;
  bridgeUrl?: string;
  token?: string;
  sessionPid?: number;
}

export interface HostTransformPlan {
  helpToolName: string;
  helpDescription: string;
  outputDir?: string;
  files: Record<string, string>;
  paths: string[];
  environment: Record<string, string>;
  justBash?: {
    providerName: string;
    commandName: string;
    helpToolName: string;
    commands: Array<{
      commandName: string;
      backendToolName: string;
      description?: string;
      inputSchema: unknown;
    }>;
  };
}

export function buildHostTransformPlan(config: HostTransformPlanConfig): HostTransformPlan {
  return JSON.parse(
    loadNativeCore().buildHostTransformPlanJson(
      stringify({
        kind: config.kind,
        serverName: config.serverName,
        tools: config.tools.map(toNativeTool),
        outputDir: config.outputDir,
        commandName: config.commandName,
        bridgeUrl: config.bridgeUrl,
        token: config.token,
        sessionPid: config.sessionPid,
      }),
    ),
  ) as HostTransformPlan;
}

export function normalizeHostToolResult(value: unknown, toonify: boolean): string {
  return loadNativeCore().normalizeHostToolResultJson(stringify(value), toonify);
}
