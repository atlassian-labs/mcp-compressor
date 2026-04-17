import { BackendClient } from "./backend-client.js";
import { VERSION } from "./version.js";
import { parseServerConfigJson } from "./config.js";
import { InvalidConfigurationError } from "./errors.js";
import { clearAllOAuthState, PersistentOAuthProvider } from "./oauth.js";
import { CompressorRuntime, UNCOMPRESSED_RESOURCE_URI } from "./runtime.js";
import { CompressorServer } from "./server.js";
import type { BackendConfig, CommonProxyOptions, StartOptions } from "./types.js";
import { FastMCP } from "fastmcp";
import { z } from "zod";

export * from "./backend-client.js";
export * from "./client.js";
export * from "./config.js";
export * from "./errors.js";
export * from "./oauth.js";
export * from "./runtime.js";
export * from "./server.js";
export * from "./cli_mode.js";
export * from "./types.js";

export interface CreateCompressorServerOptions extends CommonProxyOptions {
  backend: BackendConfig | string;
  oauthConfigDir?: string;
  oauthRedirectUrl?: string;
  onOAuthRedirect?: (url: URL) => void | Promise<void>;
  /** Enable CLI mode: starts a local HTTP bridge on connect and generates a shell script for bash access. */
  cliMode?: boolean;
  /** CLI command name (e.g. "atlassian"). Defaults to serverName or "mcp". Only used when cliMode is true. */
  cliName?: string;
  /** Port for the local CLI bridge HTTP server. Defaults to a random free port. Only used when cliMode is true. */
  cliPort?: number;
  /** Directory where the CLI script is written. Auto-detected if not set. Only used when cliMode is true. */
  scriptDir?: string;
}

/** Options for creating a compressor server from a multi-server MCP config JSON string. */
export interface CreateMultiCompressorServerOptions extends Omit<CommonProxyOptions, "serverName"> {
  backends: Array<{ backend: BackendConfig; serverName: string }>;
  oauthConfigDir?: string;
  oauthRedirectUrl?: string;
  onOAuthRedirect?: (url: URL) => void | Promise<void>;
}

/**
 * Resolve a backend into one or more `{ backend, serverName }` entries.
 *
 * Accepts a `BackendConfig` object, a remote URL string, or a JSON string containing one or
 * more servers in the `{ mcpServers: { ... } }` format.  Always returns an array.
 */
export function resolveBackends(
  backend: BackendConfig | string,
  serverName?: string,
): Array<{ backend: BackendConfig; serverName?: string }> {
  if (typeof backend !== "string") {
    return [{ backend, serverName }];
  }

  const parsed = parseServerConfigJson(backend);
  if (parsed) {
    return parsed.map((entry) => ({
      backend: entry.backend,
      serverName: serverName ? `${serverName}_${entry.serverName}` : entry.serverName,
    }));
  }

  if (backend.startsWith("http://") || backend.startsWith("https://")) {
    return [{ backend: { type: "http", url: backend }, serverName }];
  }

  throw new InvalidConfigurationError(
    "String backend values must be a remote URL or an MCP config JSON string.",
  );
}

export function createOAuthProviderForBackend(
  backend: BackendConfig,
  options: Pick<
    CreateCompressorServerOptions,
    "oauthConfigDir" | "oauthRedirectUrl" | "onOAuthRedirect"
  > = {},
): PersistentOAuthProvider | undefined {
  return backend.type === "http" || backend.type === "sse"
    ? new PersistentOAuthProvider({
        serverUrl: backend.url,
        configDir: options.oauthConfigDir,
        redirectUrl: options.oauthRedirectUrl,
        onRedirect: options.onOAuthRedirect,
      })
    : undefined;
}

export async function clearOAuth(
  backend: BackendConfig | string,
  options: Pick<CreateCompressorServerOptions, "oauthConfigDir"> = {},
): Promise<boolean> {
  const resolved = resolveBackends(backend)[0]!;
  const provider = createOAuthProviderForBackend(resolved.backend, options);
  if (!provider) {
    return false;
  }
  await provider.clear();
  return true;
}

export async function clearAllOAuth(
  options: Pick<CreateCompressorServerOptions, "oauthConfigDir"> & { all?: boolean } = {},
): Promise<string[]> {
  return clearAllOAuthState(options.oauthConfigDir, options.all ?? false);
}

export function createCompressorRuntime(options: CreateCompressorServerOptions): CompressorRuntime {
  const resolved = resolveBackends(options.backend, options.serverName)[0]!;
  const oauthProvider = createOAuthProviderForBackend(resolved.backend, options);

  const backendClient = new BackendClient(resolved.backend, oauthProvider);
  return new CompressorRuntime({
    backendClient,
    compressionLevel: options.compressionLevel,
    excludeTools: options.excludeTools,
    includeTools: options.includeTools,
    serverName: resolved.serverName,
    toonify: options.toonify,
    ...(options.cliMode
      ? {
          cli: {
            cliMode: true,
            cliName: options.cliName,
            cliPort: options.cliPort,
            scriptDir: options.scriptDir,
          },
        }
      : {}),
  });
}

export async function initializeCompressorRuntime(
  options: CreateCompressorServerOptions,
): Promise<CompressorRuntime> {
  const runtime = createCompressorRuntime(options);
  await runtime.connect();
  return runtime;
}

export async function initializeCompressedFunctionToolset(
  options: CreateCompressorServerOptions,
): Promise<{
  runtime: CompressorRuntime;
  toolset: ReturnType<CompressorRuntime["getFunctionToolset"]>;
}> {
  const runtime = await initializeCompressorRuntime(options);
  return {
    runtime,
    toolset: runtime.getFunctionToolset(),
  };
}

export function createCompressorServer(options: CreateCompressorServerOptions): CompressorServer {
  const resolved = resolveBackends(options.backend, options.serverName)[0]!;
  const oauthProvider = createOAuthProviderForBackend(resolved.backend, options);

  const backendClient = new BackendClient(resolved.backend, oauthProvider);
  return new CompressorServer({
    backendClient,
    compressionLevel: options.compressionLevel,
    excludeTools: options.excludeTools,
    includeTools: options.includeTools,
    serverName: resolved.serverName,
    toonify: options.toonify,
  });
}

export async function startCompressorServer(
  options: CreateCompressorServerOptions & { start?: StartOptions },
): Promise<CompressorServer> {
  const server = createCompressorServer(options);
  await server.start(options.start);
  return server;
}

/**
 * Create a `CompressorServer`-like object that aggregates multiple backend MCP servers.
 *
 * Each backend gets its own `CompressorRuntime` with a server-name prefix.  All compressed
 * wrapper tools are added to a single shared FastMCP server, so the caller sees one combined
 * compressed interface.
 */
export function createMultiCompressorServer(
  options: CreateMultiCompressorServerOptions,
): MultiCompressorServer {
  return new MultiCompressorServer(options);
}

export async function startMultipleCompressorServers(
  options: CreateMultiCompressorServerOptions & { start?: StartOptions },
): Promise<MultiCompressorServer> {
  const server = createMultiCompressorServer(options);
  await server.start(options.start);
  return server;
}

/** A compressor server that wraps multiple MCP backends under a single FastMCP instance. */
export class MultiCompressorServer {
  readonly runtimes: ReadonlyArray<CompressorRuntime>;
  readonly server: FastMCP;

  constructor(options: CreateMultiCompressorServerOptions) {
    this.server = new FastMCP({
      name: "MCP Compressor TS",
      version: VERSION,
      instructions: "A compressed MCP proxy server (multi-backend).",
    });

    const compressionLevel = options.compressionLevel ?? "medium";
    const toonify = options.toonify ?? false;

    const runtimes: CompressorRuntime[] = [];

    for (const { backend, serverName } of options.backends) {
      const oauthProvider = createOAuthProviderForBackend(backend, options);
      const backendClient = new BackendClient(backend, oauthProvider);
      const runtime = new CompressorRuntime({
        backendClient,
        compressionLevel,
        excludeTools: options.excludeTools,
        includeTools: options.includeTools,
        serverName,
        toonify,
      });
      runtimes.push(runtime);

      const prefixName = (name: string) => `${serverName}_${name}`;
      const resourceUri = UNCOMPRESSED_RESOURCE_URI.replace(
        "compressor://",
        `compressor://${serverName}/`,
      );

      this.server.addTool({
        name: prefixName("get_tool_schema"),
        description: "Return the full upstream schema for one backend tool.",
        parameters: z.object({ tool_name: z.string() }),
        execute: async ({ tool_name }) =>
          JSON.stringify(await runtime.getToolSchema(tool_name), null, 2),
      });

      this.server.addTool({
        name: prefixName("invoke_tool"),
        description: "Invoke one backend tool by name with a JSON object input.",
        parameters: z.object({
          tool_name: z.string(),
          tool_input: z.record(z.string(), z.unknown()).optional(),
        }),
        execute: async ({ tool_name, tool_input }) => runtime.invokeTool(tool_name, tool_input),
      });

      if (compressionLevel === "max") {
        this.server.addTool({
          name: prefixName("list_tools"),
          description: "List backend tool names.",
          execute: async () => JSON.stringify(await runtime.listToolNames(), null, 2),
        });
      }

      this.server.addResource({
        uri: resourceUri,
        name: `${serverName}-uncompressed-tools`,
        mimeType: "application/json",
        load: async () => ({
          text: JSON.stringify(await runtime.listUncompressedTools(), null, 2),
        }),
      });
    }

    this.runtimes = runtimes;
  }

  async connectAll(): Promise<void> {
    await Promise.all(this.runtimes.map((r) => r.connect()));
  }

  async closeAll(): Promise<void> {
    await Promise.all(this.runtimes.map((r) => r.disconnect()));
  }

  async start(options: StartOptions = {}): Promise<void> {
    await this.connectAll();
    await this.server.start({
      transportType: options.transportType ?? "stdio",
      ...(options.httpStream ? options.httpStream : {}),
    });
  }
}
