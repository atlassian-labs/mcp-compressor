/**
 * CompressorClient — unified entry point for consuming compressed MCP servers.
 *
 * Handles one or more backend MCP servers with a single lifecycle (`connect` / `close`).
 * The `mode` option determines what `getTools()` returns:
 *
 * - `"compressed"` (default) — get_tool_schema / invoke_tool wrappers
 * - `"cli"` — per-server help tools + HTTP bridges + shell scripts (side effect)
 * - `"bash"` — per-server help tools + a bash AI SDK tool with server commands installed
 */

import { z } from "zod";

import { BackendClient } from "./backend-client.js";
import { parseServerConfigJson, normalizeConfigServer } from "./config.js";
import { InvalidConfigurationError } from "./errors.js";
import { PersistentOAuthProvider } from "./oauth.js";
import { CompressorRuntime, type AiSdkTool } from "./runtime.js";
import type { BackendConfig, CommonProxyOptions, JsonConfigServerEntry } from "./types.js";
import { buildHelpToolDescription, sanitizeCliName } from "./cli_tools.js";

// ---------------------------------------------------------------------------
// Configuration types
// ---------------------------------------------------------------------------

/** Single server convenience — accepts the same shapes as CreateCompressorServerOptions.backend. */
export type ServerInput = BackendConfig | string;

/**
 * Map of server names to their configurations.
 *
 * Values can be:
 * - A `BackendConfig` object (`{ type: "stdio", command: ... }`)
 * - A `JsonConfigServerEntry` object (the familiar MCP config format: `{ command: ... }` or `{ url: ... }`)
 */
export type ServersMap = Record<string, BackendConfig | JsonConfigServerEntry>;

/** The exposure mode for tools. */
export type CompressorMode = "compressed" | "cli" | "bash";

export interface CompressorClientOptions extends Omit<CommonProxyOptions, "serverName"> {
  /**
   * Backend server(s) to connect to.
   *
   * Accepts one of:
   * - A `ServersMap` object: `{ jira: { command: "..." }, confluence: { url: "..." } }`
   * - A single `BackendConfig`: `{ type: "stdio", command: "..." }`
   * - A URL string: `"https://my-mcp-server.example.com"`
   * - An MCP config JSON string: `'{"mcpServers":{"jira":{...}}}'`
   */
  servers: ServersMap | BackendConfig | string;

  /**
   * How tools are exposed via `getTools()`.
   *
   * - `"compressed"` (default): Returns `get_tool_schema` and `invoke_tool` wrappers.
   * - `"cli"`: Starts HTTP bridges and generates shell scripts. Returns per-server help tools.
   * - `"bash"`: Installs server commands into a `Bash` instance. Returns per-server help tools
   *   plus a `bash` tool for executing commands.
   */
  mode?: CompressorMode;

  /** OAuth configuration directory. */
  oauthConfigDir?: string;
  /** OAuth redirect URL. */
  oauthRedirectUrl?: string;
  /** Callback invoked when an OAuth redirect is needed. */
  onOAuthRedirect?: (url: URL) => void | Promise<void>;

  /**
   * Per-server option overrides.
   *
   * Keys are server names from the `servers` map. Values override the top-level
   * `compressionLevel`, `includeTools`, `excludeTools`, and `toonify` for that server.
   */
  serverOptions?: Record<string, Partial<CommonProxyOptions>>;

  /** Options for CLI mode. */
  cli?: {
    /** Port for the HTTP bridge (0 = auto). */
    port?: number;
    /** Directory for generated CLI scripts. */
    scriptDir?: string;
  };

  /** Options for bash mode. */
  bash?: {
    /**
     * An existing Bash instance to register commands into.
     * If not provided, a new one is created.
     */
    bash?: import("just-bash").Bash;
    /** Options passed to the Bash constructor when creating a new instance (ignored when `bash` is provided). */
    bashOptions?: Record<string, unknown>;
  };
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

export interface CliScriptInfo {
  serverName: string;
  cliName: string;
  scriptPath: string | null;
  bridgeUrl: string;
  onPath: boolean;
}

// ---------------------------------------------------------------------------
// CompressorClient
// ---------------------------------------------------------------------------

export class CompressorClient {
  private readonly runtimeEntries: Array<{
    serverName: string;
    runtime: CompressorRuntime;
  }>;
  private connected = false;
  private closed = false;
  private readonly mode: CompressorMode;

  // Lazily populated for cli mode
  private cliScripts: CliScriptInfo[] | null = null;

  // Lazily populated for bash mode
  private bashInstance: import("just-bash").Bash | null = null;
  private bashOwned = false; // true if we created the Bash instance (and should not clean it up for the consumer)

  constructor(private readonly options: CompressorClientOptions) {
    this.mode = options.mode ?? "compressed";

    const resolved = resolveServersMap(options.servers);
    const compressionLevel = options.compressionLevel ?? "medium";
    const toonify = options.toonify ?? (this.mode === "cli" || this.mode === "bash");

    this.runtimeEntries = resolved.map(({ backend, serverName }) => {
      const perServer = options.serverOptions?.[serverName];
      // Don't create an OAuth provider when the backend already has an Authorization
      // header pre-configured — the static header should be used as-is.
      const hasAuthHeader =
        (backend.type === "http" || backend.type === "sse") &&
        backend.headers &&
        Object.keys(backend.headers).some((k) => k.toLowerCase() === "authorization");
      const oauthProvider =
        (backend.type === "http" || backend.type === "sse") && !hasAuthHeader
          ? new PersistentOAuthProvider({
              serverUrl: backend.url,
              configDir: options.oauthConfigDir,
              redirectUrl: options.oauthRedirectUrl,
              onRedirect: options.onOAuthRedirect,
            })
          : undefined;
      const backendClient = new BackendClient(backend, oauthProvider);

      const runtime = new CompressorRuntime({
        backendClient,
        compressionLevel: perServer?.compressionLevel ?? compressionLevel,
        excludeTools: perServer?.excludeTools ?? options.excludeTools,
        includeTools: perServer?.includeTools ?? options.includeTools,
        serverName: perServer?.serverName ?? serverName,
        toonify: perServer?.toonify ?? toonify,
      });

      return { serverName, runtime };
    });
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  /** Connect to all backend servers and cache their tool lists. */
  async connect(): Promise<void> {
    if (this.connected) {
      return;
    }
    if (this.closed) {
      throw new Error("CompressorClient has been closed and cannot be reconnected.");
    }

    await Promise.all(this.runtimeEntries.map(({ runtime }) => runtime.connect()));
    this.connected = true;

    // Eagerly initialize mode-specific resources so getTools() can be synchronous-feeling
    if (this.mode === "cli") {
      await this.initCliMode();
    } else if (this.mode === "bash") {
      await this.initBashMode();
    }
  }

  /** Disconnect all backends, tear down CLI bridges and scripts. */
  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    this.connected = false;

    await Promise.allSettled(this.runtimeEntries.map(({ runtime }) => runtime.disconnect()));
  }

  // ---------------------------------------------------------------------------
  // Unified getTools()
  // ---------------------------------------------------------------------------

  /**
   * Get AI SDK tools for all connected servers.
   *
   * The shape of the returned tools depends on the `mode`:
   *
   * - `"compressed"`: `{ <server>_get_tool_schema, <server>_invoke_tool, ... }`
   * - `"cli"`: `{ <server>_help, ... }` — per-server help tools
   * - `"bash"`: `{ bash, <server>_help, ... }` — bash tool + per-server help tools
   */
  async getTools(): Promise<Record<string, AiSdkTool>> {
    this.requireConnected();

    switch (this.mode) {
      case "compressed":
        return this.buildCompressedTools();
      case "cli":
        return this.buildHelpTools();
      case "bash":
        return this.buildBashTools();
      default:
        throw new Error(`Unknown mode: ${this.mode}`);
    }
  }

  // ---------------------------------------------------------------------------
  // Mode-specific results
  // ---------------------------------------------------------------------------

  /** CLI mode: info about generated scripts. Only available after `connect()` in `"cli"` mode. */
  get scripts(): ReadonlyArray<CliScriptInfo> {
    if (this.mode !== "cli" || !this.cliScripts) {
      return [];
    }
    return this.cliScripts;
  }

  /** Bash mode: the Bash instance with server commands registered. Only available after `connect()` in `"bash"` mode. */
  get bash(): import("just-bash").Bash | null {
    return this.bashInstance;
  }

  // ---------------------------------------------------------------------------
  // Escape hatches
  // ---------------------------------------------------------------------------

  /** Get a specific runtime by server name. */
  getRuntime(serverName: string): CompressorRuntime {
    const entry = this.runtimeEntries.find((e) => e.serverName === serverName);
    if (!entry) {
      const available = this.runtimeEntries.map((e) => e.serverName);
      throw new Error(
        `No runtime found for server "${serverName}". Available: ${available.join(", ")}`,
      );
    }
    return entry.runtime;
  }

  /** All runtimes, in order. */
  get runtimes(): ReadonlyArray<CompressorRuntime> {
    return this.runtimeEntries.map((e) => e.runtime);
  }

  /** All server names, in order. */
  get serverNames(): ReadonlyArray<string> {
    return this.runtimeEntries.map((e) => e.serverName);
  }

  /** Whether the client is currently connected. */
  get isConnected(): boolean {
    return this.connected;
  }

  // ---------------------------------------------------------------------------
  // Internal: tool builders
  // ---------------------------------------------------------------------------

  private async buildCompressedTools(): Promise<Record<string, AiSdkTool>> {
    const allTools: Record<string, AiSdkTool> = {};
    for (const { runtime } of this.runtimeEntries) {
      Object.assign(allTools, await runtime.getAiSdkTools());
    }
    return allTools;
  }

  private async buildHelpTools(): Promise<Record<string, AiSdkTool>> {
    const tools: Record<string, AiSdkTool> = {};

    for (const { serverName, runtime } of this.runtimeEntries) {
      const cliName = sanitizeCliName(runtime.serverName ?? serverName);
      const backendTools = await runtime.listUncompressedTools();
      const serverDescription = runtime.serverName
        ? `the ${runtime.serverName} toolset`
        : "this toolset";

      // In CLI mode, onPath comes from the bridge result. In bash mode, commands
      // are always available directly by name (they're registered in the bash env).
      const onPath =
        this.mode === "bash"
          ? true
          : (this.cliScripts?.find((s) => s.serverName === (runtime.serverName ?? serverName))
              ?.onPath ?? true);

      const description = buildHelpToolDescription(
        cliName,
        serverDescription,
        backendTools,
        onPath,
      );

      const helpToolName = runtime.serverName ? `${runtime.serverName}_help` : "help";

      tools[helpToolName] = {
        description,
        parameters: z.object({}),
        execute: async () => description,
      };
    }

    return tools;
  }

  private async buildBashTools(): Promise<Record<string, AiSdkTool>> {
    const helpTools = await this.buildHelpTools();

    const bash = this.bashInstance!;
    const bashTool: AiSdkTool<{ command: string }, string> = {
      description:
        "Execute bash commands in a sandboxed environment. " +
        "Supports standard Unix utilities (grep, cat, jq, sed, awk, sort, find, and many more) " +
        "as well as custom commands from connected MCP servers. " +
        "See the help tools for available server commands and usage.",
      parameters: z.object({
        command: z.string().describe("The bash command to execute."),
      }),
      execute: async (args: { command: string }) => {
        const result = await bash.exec(args.command);
        if (result.exitCode !== 0) {
          return `Exit code: ${result.exitCode}\n${result.stdout}${result.stderr ? `\nSTDERR: ${result.stderr}` : ""}`;
        }
        return result.stdout || "(no output)";
      },
    };

    return { bash: bashTool, ...helpTools };
  }

  // ---------------------------------------------------------------------------
  // Internal: mode initialization
  // ---------------------------------------------------------------------------

  private async initCliMode(): Promise<void> {
    this.cliScripts = [];

    for (const { serverName, runtime } of this.runtimeEntries) {
      const bridgeResult = await runtime.startCliBridge(
        this.options.cli?.port,
        this.options.cli?.scriptDir,
      );

      const cliName = sanitizeCliName(runtime.serverName ?? serverName);
      this.cliScripts.push({
        serverName: runtime.serverName ?? serverName,
        cliName,
        scriptPath: bridgeResult.scriptPath,
        bridgeUrl: bridgeResult.bridgeUrl,
        onPath: bridgeResult.onPath,
      });
    }
  }

  private async initBashMode(): Promise<void> {
    const { createBashCommand } = await import("./bash_commands.js");

    const commands: import("just-bash").Command[] = [];
    for (const { runtime } of this.runtimeEntries) {
      const tools = await runtime.listUncompressedTools();
      commands.push(createBashCommand(runtime, tools));
    }

    if (this.options.bash?.bash) {
      // Register commands into the existing Bash instance
      this.bashInstance = this.options.bash.bash;
      this.bashOwned = false;
      for (const command of commands) {
        this.bashInstance.registerCommand(command);
      }
    } else {
      // Create a new Bash instance with commands
      const { Bash, ReadWriteFs } = await import("just-bash");
      this.bashInstance = new Bash({
        customCommands: commands,
        fs: new ReadWriteFs({ root: process.cwd() }),
        cwd: "/",
        ...this.options.bash?.bashOptions,
      });
      this.bashOwned = true;
    }
  }

  // ---------------------------------------------------------------------------
  // Internal helpers
  // ---------------------------------------------------------------------------

  private requireConnected(): void {
    if (!this.connected) {
      throw new Error("CompressorClient is not connected. Call connect() first.");
    }
  }
}

// ---------------------------------------------------------------------------
// Server resolution
// ---------------------------------------------------------------------------

/**
 * Resolve the `servers` option into a flat array of `{ backend, serverName }` entries.
 *
 * Accepts:
 * - A `ServersMap` (record of name → config)
 * - A `BackendConfig` object
 * - A URL string
 * - An MCP config JSON string
 */
function resolveServersMap(
  servers: CompressorClientOptions["servers"],
): Array<{ backend: BackendConfig; serverName: string }> {
  // Case 1: A string — could be a URL or MCP config JSON
  if (typeof servers === "string") {
    const parsed = parseServerConfigJson(servers);
    if (parsed) {
      return parsed;
    }

    if (servers.startsWith("http://") || servers.startsWith("https://")) {
      return [{ backend: { type: "http", url: servers }, serverName: "default" }];
    }

    throw new InvalidConfigurationError(
      "String servers value must be a remote URL or an MCP config JSON string.",
    );
  }

  // Case 2: A BackendConfig (has a `type` field)
  if ("type" in servers && typeof servers.type === "string") {
    return [{ backend: servers as BackendConfig, serverName: "default" }];
  }

  // Case 3: A ServersMap (record of name → config)
  const entries = Object.entries(servers as ServersMap);
  if (entries.length === 0) {
    throw new InvalidConfigurationError("servers must contain at least one server.");
  }

  return entries.map(([serverName, config]) => {
    // If it already has a `type` field, it's a BackendConfig
    if ("type" in config && typeof config.type === "string") {
      return { backend: config as BackendConfig, serverName };
    }
    // Otherwise it's a JsonConfigServerEntry — normalize it
    return {
      backend: normalizeConfigServer(config as JsonConfigServerEntry),
      serverName,
    };
  });
}
