import {
  generateClientArtifacts,
  normalizeSdkServers,
  startCompressedSession,
  startCompressedSessionFromMcpConfig,
} from "./rust_core.js";
import type { BackendConfig as LegacyBackendConfig, JsonConfigServerEntry } from "./types.js";
import type { CompressedSession, CompressedSessionInfo } from "./rust_core.js";

export type NativeCompressorMode = "compressed" | "cli" | "bash" | "python" | "typescript";
export type NativeServerConfig = LegacyBackendConfig | JsonConfigServerEntry | string;
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

export function normalizeServers(servers: NativeServersInput): NormalizedBackendConfig[] | string {
  if (typeof servers === "string") {
    const trimmed = servers.trim();
    if (trimmed.startsWith("{")) {
      return servers;
    }
    return [{ name: "default", commandOrUrl: servers }];
  }
  if (isLegacyBackendConfig(servers)) {
    return [legacyBackendToNative("default", servers)];
  }
  return normalizeSdkServers(servers);
}

function isLegacyBackendConfig(value: unknown): value is LegacyBackendConfig {
  return (
    typeof value === "object" && value !== null && "type" in value && typeof value.type === "string"
  );
}

function legacyBackendToNative(
  name: string,
  backend: LegacyBackendConfig,
): NormalizedBackendConfig {
  if (backend.type === "stdio") {
    return { name, commandOrUrl: backend.command, args: backend.args ?? [] };
  }
  const args: string[] = [];
  if (backend.headers) {
    for (const [key, value] of Object.entries(backend.headers)) {
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
  ) {}

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
}

export class CompressorClient {
  private session: CompressedSession | null = null;
  private readonly mode: NativeCompressorMode;

  constructor(private readonly options: CompressorClientOptions) {
    this.mode = options.mode ?? "compressed";
  }

  async connect(): Promise<CompressorProxy> {
    if (this.session) {
      return new CompressorProxy(this.session, this.defaultServer());
    }
    const normalized = normalizeServers(this.options.servers);
    const config = {
      compressionLevel: this.options.compressionLevel ?? "medium",
      serverName: this.options.serverName ?? null,
      includeTools: this.options.includeTools ?? [],
      excludeTools: this.options.excludeTools ?? [],
      toonify: this.options.toonify ?? false,
      transformMode: transformMode(this.mode),
    };
    this.session =
      typeof normalized === "string"
        ? await startCompressedSessionFromMcpConfig(config, normalized)
        : await startCompressedSession(config, normalized);
    return new CompressorProxy(this.session, this.defaultServer());
  }

  async close(): Promise<void> {
    this.session?.close();
    this.session = null;
  }

  private defaultServer(): string | null {
    const normalized = normalizeServers(this.options.servers);
    if (typeof normalized === "string") return null;
    return normalized.length === 1 ? normalized[0]!.name : null;
  }
}
