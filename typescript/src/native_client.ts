import {
  normalizeSdkServers,
  startCompressedSession,
  startCompressedSessionFromMcpConfig,
} from "./rust_core.js";
import type { BackendConfig as LegacyBackendConfig, JsonConfigServerEntry } from "./types.js";
import type { CompressedSession, CompressedSessionInfo } from "./rust_core.js";

export type NativeCompressorMode = "compressed" | "cli" | "bash" | "python" | "typescript";
export type NativeServerConfig = LegacyBackendConfig | JsonConfigServerEntry | string;
export type NativeServersInput = Record<string, NativeServerConfig> | LegacyBackendConfig | string;

export interface NativeCompressorClientOptions {
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

export class NativeCompressorProxy {
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

  async invokeWrapper(
    wrapperTool: string,
    toolInput: Record<string, unknown>,
  ): Promise<ProxyResponse> {
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

  schema(_tool: string, options: { server?: string } = {}): Record<string, unknown> {
    const server = options.server ?? this.defaultServer;
    const invokeTool = `${server ? `${server}_` : ""}invoke_tool`;
    const wrapper = this.tools.find((tool) => tool.name === invokeTool);
    if (!wrapper) {
      throw new Error(`No compressed invoke wrapper found for ${server ?? "default"}`);
    }
    return wrapper.inputSchema;
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
    this.session.close();
  }
}

export class NativeCompressorClient {
  private session: CompressedSession | null = null;
  private readonly mode: NativeCompressorMode;

  constructor(private readonly options: NativeCompressorClientOptions) {
    this.mode = options.mode ?? "compressed";
  }

  async connect(): Promise<NativeCompressorProxy> {
    if (this.session) {
      return new NativeCompressorProxy(this.session, this.defaultServer());
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
    return new NativeCompressorProxy(this.session, this.defaultServer());
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
