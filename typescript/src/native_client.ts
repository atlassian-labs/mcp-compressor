import type { ExecutableTool } from "./adapters.js";

import {
  generateClientArtifacts,
  normalizeSdkServers,
  startCompressedSession,
  startCompressedSessionWithAuthProviders,
  startCompressedSessionFromMcpConfig,
} from "./rust_core.js";
import type { BackendConfig as LegacyBackendConfig, JsonConfigServerEntry } from "./types.js";
import type { CompressedSession, CompressedSessionInfo } from "./rust_core.js";

export type NativeCompressorMode = "compressed" | "cli" | "bash" | "python" | "typescript";
export type AuthProvider = () => Record<string, string> | Promise<Record<string, string>>;
export type NativeServerObjectConfig = (LegacyBackendConfig | JsonConfigServerEntry) & {
  authProvider?: AuthProvider;
  auth_provider?: AuthProvider;
};
export type NativeServerConfig = NativeServerObjectConfig | string;
export type NativeServersInput = Record<string, NativeServerConfig> | LegacyBackendConfig | string;

export interface CompressorClientOptions {
  servers: NativeServersInput;
  mode?: NativeCompressorMode;
  compressionLevel?: string;
  serverName?: string | null;
  includeTools?: string[];
  excludeTools?: string[];
  toonify?: boolean;
}

export interface ProxyTool {
  name: string;
  description?: string | null;
  inputSchema: Record<string, unknown>;
}

export interface ProxyResponse {
  text: string;
}

export interface JustBashCommand {
  commandName: string;
  backendToolName: string;
  description?: string | null;
  inputSchema: Record<string, unknown>;
  invokeToolName: string;
}

export interface JustBashProvider {
  providerName: string;
  helpToolName: string;
  tools: JustBashCommand[];
}

export type GeneratedClientKind = "cli" | "python" | "typescript";

export interface NormalizedBackendConfig {
  name: string;
  commandOrUrl: string;
  args?: string[];
}

export interface GeneratedCodeClient {
  language: "python" | "typescript";
  outputDir: string;
  files: string[];
  environment: Record<string, string>;
}

function providerFromConfig(config: Record<string, unknown>): AuthProvider | undefined {
  const provider = config.authProvider ?? config.auth_provider;
  if (provider === undefined) {
    return undefined;
  }
  if (typeof provider !== "function") {
    throw new TypeError("authProvider must be a function");
  }
  return provider as AuthProvider;
}

async function resolveAuthHeaders(
  config: Record<string, unknown>,
  options: { includeProvider?: boolean } = {},
): Promise<Record<string, string>> {
  const headers: Record<string, string> = {};
  const rawHeaders = config.headers;
  if (rawHeaders && typeof rawHeaders === "object" && !Array.isArray(rawHeaders)) {
    for (const [key, value] of Object.entries(rawHeaders as Record<string, unknown>)) {
      headers[key] = String(value);
    }
  }
  const provider = providerFromConfig(config);
  if ((options.includeProvider ?? true) && provider !== undefined) {
    const provided = await provider();
    for (const [key, value] of Object.entries(provided)) {
      headers[key] = String(value);
    }
  }
  return headers;
}

interface ProviderMaterializedBackend extends NormalizedBackendConfig {
  providerIndex?: number;
}

async function normalizeServersWithProviders(
  servers: NativeServersInput,
): Promise<{ backends: ProviderMaterializedBackend[]; providers: AuthProvider[] } | string> {
  if (typeof servers === "string") {
    const trimmed = servers.trim();
    if (trimmed.startsWith("{")) {
      return servers;
    }
    return { backends: [{ name: "default", commandOrUrl: servers }], providers: [] };
  }
  if (isLegacyBackendConfig(servers)) {
    return {
      backends: [await legacyBackendToNative("default", servers, { includeProvider: false })],
      providers: [],
    };
  }
  const backends: ProviderMaterializedBackend[] = [];
  const providers: AuthProvider[] = [];
  for (const [name, config] of Object.entries(servers as Record<string, NativeServerConfig>)) {
    if (typeof config === "object" && config !== null && "url" in config) {
      const provider = providerFromConfig(config as Record<string, unknown>);
      const backend: ProviderMaterializedBackend = await sdkObjectToNative(
        name,
        config as Record<string, unknown>,
        { includeProvider: true },
      );
      if (provider !== undefined) {
        backend.providerIndex = providers.length;
        providers.push(provider);
      }
      backends.push(backend);
    } else if (typeof config === "object" && config !== null && "command" in config) {
      backends.push(
        await sdkObjectToNative(name, config as Record<string, unknown>, {
          includeProvider: false,
        }),
      );
    } else {
      const normalized = normalizeSdkServers({ [name]: config });
      backends.push(normalized[0]!);
    }
  }
  return { backends, providers };
}

export async function normalizeServers(
  servers: NativeServersInput,
): Promise<NormalizedBackendConfig[] | string> {
  if (typeof servers === "string") {
    const trimmed = servers.trim();
    if (trimmed.startsWith("{")) {
      return servers;
    }
    return [{ name: "default", commandOrUrl: servers }];
  }
  if (isLegacyBackendConfig(servers)) {
    return [await legacyBackendToNative("default", servers)];
  }
  const materialized: Record<string, unknown> = {};
  for (const [name, config] of Object.entries(servers as Record<string, NativeServerConfig>)) {
    if (typeof config === "object" && config !== null && "url" in config) {
      materialized[name] = {
        ...config,
        headers: await resolveAuthHeaders(config as Record<string, unknown>),
      };
    } else {
      materialized[name] = config;
    }
  }
  return normalizeSdkServers(materialized);
}

function isLegacyBackendConfig(value: unknown): value is LegacyBackendConfig {
  return (
    typeof value === "object" && value !== null && "type" in value && typeof value.type === "string"
  );
}

async function sdkObjectToNative(
  name: string,
  config: Record<string, unknown>,
  options: { includeProvider?: boolean } = {},
): Promise<NormalizedBackendConfig> {
  if ("url" in config) {
    const args: string[] = [];
    const headers = await resolveAuthHeaders(config, options);
    for (const [key, value] of Object.entries(headers)) {
      args.push("-H", `${key}=${value}`);
    }
    if (
      Object.keys(headers).length > 0 &&
      !(Array.isArray(config.args) && config.args.includes("--auth"))
    ) {
      args.push("--auth", "explicit-headers");
    }
    if (Array.isArray(config.args)) {
      args.push(...config.args.map(String));
    }
    return { name, commandOrUrl: String(config.url), args };
  }
  if ("command" in config) {
    return {
      name,
      commandOrUrl: String(config.command),
      args: Array.isArray(config.args) ? config.args.map(String) : [],
    };
  }
  throw new Error(`server ${name} must define command or url`);
}

async function legacyBackendToNative(
  name: string,
  backend: LegacyBackendConfig,
  options: { includeProvider?: boolean } = {},
): Promise<NormalizedBackendConfig> {
  if (backend.type === "stdio") {
    return { name, commandOrUrl: backend.command, args: backend.args ?? [] };
  }
  const args: string[] = [];
  const headers = await resolveAuthHeaders(backend as unknown as Record<string, unknown>, options);
  if (Object.keys(headers).length > 0) {
    for (const [key, value] of Object.entries(headers)) {
      args.push("-H", `${key}=${value}`);
    }
    args.push("--auth", "explicit-headers");
  }
  return { name, commandOrUrl: backend.url, args };
}

function transformMode(mode: NativeCompressorMode): string | null {
  if (mode === "compressed") return null;
  if (mode === "bash") return "just-bash";
  if (mode === "python" || mode === "typescript") return "cli";
  return mode;
}

export class CompressorProxy {
  private closed = false;

  constructor(
    private readonly session: CompressedSession,
    private readonly defaultServer: string | null,
    private readonly authProviders: AuthProvider[] = [],
  ) {}

  private async refreshAuthProviders(): Promise<void> {
    await Promise.all(
      this.authProviders.map(async (provider, index) => {
        const headers = await provider();
        const materialized: Record<string, string> = {};
        for (const [key, value] of Object.entries(headers)) {
          materialized[key] = String(value);
        }
        this.session.updateAuthProviderHeaders(index, materialized);
      }),
    );
  }

  info(): CompressedSessionInfo {
    return this.session.info();
  }

  get bridgeUrl(): string {
    return this.info().bridge_url;
  }

  get token(): string {
    return this.info().token;
  }

  get tools(): ProxyTool[] {
    return this.info().frontend_tools.map((tool) => ({
      name: tool.name,
      description: tool.description,
      inputSchema: tool.input_schema,
    }));
  }

  get justBashProviders(): JustBashProvider[] {
    return this.info().just_bash_providers.map((provider) => ({
      providerName: provider.provider_name,
      helpToolName: provider.help_tool_name,
      tools: provider.tools.map((command) => ({
        commandName: command.command_name,
        backendToolName:
          command.backendToolName ??
          command.backend_tool_name ??
          command.tool_name ??
          command.command_name,
        description: command.description,
        inputSchema: command.input_schema,
        invokeToolName: command.invoke_tool_name,
      })),
    }));
  }

  async invokeWrapper(
    wrapperTool: string,
    toolInput: Record<string, unknown>,
  ): Promise<ProxyResponse> {
    if (this.closed) {
      throw new Error("Compressor proxy is closed");
    }
    await this.refreshAuthProviders();
    const response = await fetch(`${this.bridgeUrl}/exec`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${this.token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ tool: wrapperTool, input: toolInput }),
    });
    if (!response.ok) {
      throw new Error(`Proxy invocation failed: ${response.status} ${await response.text()}`);
    }
    return { text: await response.text() };
  }

  schema(tool: string, options: { server?: string } = {}): Record<string, unknown> {
    const server = options.server ?? this.defaultServer;
    const matches = this.info().backend_tools_by_server.filter(
      (item) =>
        item.tool.name === tool &&
        (server === null ||
          server === undefined ||
          (item.server_name ?? item.serverName) === server),
    );
    if (matches.length === 1) {
      return matches[0]?.tool.input_schema ?? {};
    }
    if (matches.length === 0) {
      throw new Error(`Backend tool not found: ${tool}`);
    }
    throw new Error("Multiple backend tools matched; specify a server");
  }

  async invoke(
    tool: string,
    toolInput: Record<string, unknown> = {},
    options: { server?: string } = {},
  ): Promise<string> {
    const server = options.server ?? this.defaultServer;
    const wrapper = `${server ? `${server}_` : ""}invoke_tool`;
    return (await this.invokeWrapper(wrapper, { tool_name: tool, tool_input: toolInput })).text;
  }

  close(): void {
    this.closed = true;
    this.session.close();
  }

  toExecutableTools(): Record<string, ExecutableTool> {
    const result: Record<string, ExecutableTool> = {};
    for (const tool of this.tools) {
      result[tool.name] = {
        name: tool.name,
        description: tool.description ?? undefined,
        inputSchema: tool.inputSchema,
        execute: async (input: Record<string, unknown> = {}) =>
          (await this.invokeWrapper(tool.name, input)).text,
      };
    }
    return result;
  }

  writeClient(
    kind: GeneratedClientKind,
    outputDir: string,
    options: { name?: string } = {},
  ): string[] {
    const info = this.info();
    return generateClientArtifacts(kind, {
      cliName: options.name ?? this.defaultServer ?? "mcp",
      bridgeUrl: info.bridge_url,
      token: info.token,
      tools: info.backend_tools.map((tool) => ({
        name: tool.name,
        description: tool.description,
        inputSchema: tool.input_schema,
      })),
      outputDir,
      sessionPid: 0,
    });
  }

  writeCodeClient(options: {
    language: "python" | "typescript";
    outputDir: string;
    name?: string;
  }): GeneratedCodeClient {
    const files = this.writeClient(options.language, options.outputDir, { name: options.name });
    return {
      language: options.language,
      outputDir: options.outputDir,
      files,
      environment: options.language === "python" ? { PYTHONPATH: options.outputDir } : {},
    };
  }
}

export class CompressorClient {
  private session: CompressedSession | null = null;
  private readonly mode: NativeCompressorMode;
  private authProviders: AuthProvider[] = [];

  constructor(private readonly options: CompressorClientOptions) {
    this.mode = options.mode ?? "compressed";
  }

  async connect(): Promise<CompressorProxy> {
    if (this.session) {
      return new CompressorProxy(this.session, this.defaultServer(), this.authProviders);
    }
    const normalized = await normalizeServersWithProviders(this.options.servers);
    const config = {
      compressionLevel: this.options.compressionLevel ?? "medium",
      serverName: this.options.serverName ?? null,
      includeTools: this.options.includeTools ?? [],
      excludeTools: this.options.excludeTools ?? [],
      toonify: this.options.toonify ?? false,
      transformMode: transformMode(this.mode),
    };
    this.authProviders = typeof normalized === "string" ? [] : normalized.providers;
    this.session =
      typeof normalized === "string"
        ? await startCompressedSessionFromMcpConfig(config, normalized)
        : normalized.providers.length > 0
          ? await startCompressedSessionWithAuthProviders(
              config,
              normalized.backends,
              normalized.providers,
            )
          : await startCompressedSession(config, normalized.backends);
    return new CompressorProxy(this.session, this.defaultServer(), this.authProviders);
  }

  async close(): Promise<void> {
    this.session?.close();
    this.session = null;
  }

  private defaultServer(): string | null {
    const servers = this.options.servers;
    if (typeof servers === "string") {
      return servers.trim().startsWith("{") ? null : "default";
    }
    if (isLegacyBackendConfig(servers)) {
      return "default";
    }
    const names = Object.keys(servers as Record<string, NativeServerConfig>);
    return names.length === 1 ? names[0]! : null;
  }
}
