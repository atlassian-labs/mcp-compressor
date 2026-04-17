import type { Tool } from "@modelcontextprotocol/sdk/types.js";
import { z } from "zod";

import { CliBridge } from "./cli_bridge.js";
import { generateCliScript, removeCliScriptEntry } from "./cli_script.js";
import { buildHelpToolDescription, sanitizeCliName } from "./cli_tools.js";
import { ToolNotFoundError } from "./errors.js";
import { formatCliToolResult, formatToolDescription, formatToolResult } from "./formatting.js";
import type { BackendToolClient, CommonProxyOptions, CompressionLevel } from "./types.js";

export const UNCOMPRESSED_RESOURCE_URI = "compressor://uncompressed-tools";

/** CLI mode configuration for CompressorRuntime — starts a local HTTP bridge and generates a shell script. */
export interface RuntimeCliConfig {
  /** Enable CLI mode: exposes a single help tool and starts a local bridge for bash access. */
  cliMode: true;
  /** CLI command name (e.g. "atlassian"). Defaults to serverName or "mcp". */
  cliName?: string;
  /** Port for the local HTTP bridge. Defaults to a random free port. */
  cliPort?: number;
  /** Directory where the CLI script is written. Auto-detected if not set. */
  scriptDir?: string;
}

export interface CompressorRuntimeOptions extends CommonProxyOptions {
  backendClient: BackendToolClient;
  /** CLI mode options. When set, the runtime starts a CLI bridge on connect and exposes a single help tool. */
  cli?: RuntimeCliConfig;
}

export type WrapperToolHandler = (input?: Record<string, unknown>) => Promise<string>;

/**
 * A tool definition structurally compatible with the AI SDK `Tool` type.
 *
 * This avoids a direct dependency on the `ai` package while producing objects that satisfy the
 * `Tool` / `VercelTool` type expected by AI SDK consumers (e.g. Mastra's `ToolsInput`).
 *
 * Uses `any` for the generic defaults to match the AI SDK's own `Tool<any, any>` convention,
 * ensuring structural compatibility without import-time coupling.
 */
// biome-lint: using `any` intentionally — mirrors AI SDK's Tool<any, any> signature
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export interface AiSdkTool<TParams = any, TResult = any> {
  type?: "function";
  description: string;
  parameters: z.ZodType<TParams>;
  execute: (args: TParams, options?: unknown) => PromiseLike<TResult>;
}

export class CompressorRuntime {
  readonly backendClient: BackendToolClient;
  private readonly compressionLevel: CompressionLevel;
  private readonly excludeTools: Set<string>;
  private readonly includeTools: Set<string> | null;
  readonly serverName?: string;
  private readonly toonify: boolean;
  private toolCache = new Map<string, Tool>();
  private toolListCache: Tool[] | null = null;

  // CLI mode state
  private readonly cliOptions?: RuntimeCliConfig;
  private cliBridge: CliBridge | null = null;
  /** Path to the generated CLI script, or null if not in CLI mode or not yet connected. */
  cliScriptPath: string | null = null;
  /** Whether the CLI script directory is on the system PATH. */
  cliOnPath = false;
  /** URL of the local CLI bridge HTTP server, or null if not in CLI mode or not yet connected. */
  cliBridgeUrl: string | null = null;
  private cliSessionPid: number | null = null;

  constructor(options: CompressorRuntimeOptions) {
    this.backendClient = options.backendClient;
    this.compressionLevel = options.compressionLevel ?? "medium";
    this.includeTools =
      options.includeTools && options.includeTools.length > 0
        ? new Set(options.includeTools)
        : null;
    this.excludeTools = new Set(options.excludeTools ?? []);
    this.serverName = options.serverName;
    this.toonify = options.toonify ?? false;
    this.cliOptions = options.cli;
  }

  private get isCliMode(): boolean {
    return this.cliOptions?.cliMode === true;
  }

  private get cliName(): string {
    return sanitizeCliName(this.cliOptions?.cliName ?? this.serverName ?? "mcp");
  }

  private get serverDescription(): string {
    return this.serverName ? `the ${this.serverName} toolset` : "this toolset";
  }

  async connect(): Promise<void> {
    await this.backendClient.connect();
    await this.refreshTools();

    if (this.isCliMode) {
      await this.startCliBridge(this.cliOptions?.cliPort, this.cliOptions?.scriptDir);
    }
  }

  /**
   * Start the CLI bridge HTTP server and generate the CLI shell script.
   *
   * This is called automatically during `connect()` when `cli.cliMode` is set.  It can also be
   * called directly after `connect()` to lazily enable CLI mode on a runtime that was not
   * originally created with `cliMode: true`.
   *
   * Returns info about the generated script and bridge.
   */
  async startCliBridge(
    port?: number,
    scriptDir?: string,
  ): Promise<{ scriptPath: string | null; onPath: boolean; bridgeUrl: string }> {
    if (this.cliBridge) {
      return {
        scriptPath: this.cliScriptPath,
        onPath: this.cliOnPath,
        bridgeUrl: this.cliBridgeUrl!,
      };
    }

    this.cliBridge = new CliBridge(this, this.cliName);
    const bridgePort = await this.cliBridge.start(port ?? 0);
    this.cliBridgeUrl = this.cliBridge.url;
    this.cliSessionPid = process.ppid;
    const generated = await generateCliScript(
      this.cliName,
      bridgePort,
      this.cliSessionPid,
      scriptDir,
    );
    this.cliScriptPath = generated.scriptPath;
    this.cliOnPath = generated.onPath;

    return {
      scriptPath: this.cliScriptPath,
      onPath: this.cliOnPath,
      bridgeUrl: this.cliBridgeUrl,
    };
  }

  async disconnect(): Promise<void> {
    if (this.cliBridge) {
      await this.cliBridge.close();
      this.cliBridge = null;
    }
    if (this.cliSessionPid !== null) {
      await removeCliScriptEntry(this.cliName, this.cliSessionPid, this.cliOptions?.scriptDir);
      this.cliSessionPid = null;
    }
    await this.backendClient.disconnect();
  }

  async refreshTools(): Promise<void> {
    const tools = await this.backendClient.listTools();
    const filtered = tools.filter((tool) => this.shouldIncludeTool(tool.name));
    this.toolListCache = filtered;
    this.toolCache = new Map(filtered.map((tool) => [tool.name, tool]));
  }

  async getToolSchema(toolName: string): Promise<Tool> {
    return this.getBackendTool(toolName);
  }

  async invokeTool(
    toolName: string,
    toolInput: Record<string, unknown> | undefined,
  ): Promise<string> {
    await this.ensureTools();
    try {
      const result = await this.backendClient.callTool(toolName, toolInput);
      return formatToolResult(result, this.toonify);
    } catch (error) {
      const schema = await this.getToolSchema(toolName);
      throw new Error(
        `${(error as Error).message}\n\nUpstream schema:\n${JSON.stringify(schema, null, 2)}`,
      );
    }
  }

  async invokeToolForCli(
    toolName: string,
    toolInput: Record<string, unknown> | undefined,
  ): Promise<string> {
    await this.ensureTools();
    try {
      const result = await this.backendClient.callTool(toolName, toolInput);
      return formatCliToolResult(result, this.toonify);
    } catch (error) {
      const schema = await this.getToolSchema(toolName);
      throw new Error(
        `${(error as Error).message}\n\nUpstream schema:\n${JSON.stringify(schema, null, 2)}`,
      );
    }
  }

  async listToolNames(): Promise<string[]> {
    await this.ensureTools();
    return [...this.toolCache.keys()].sort();
  }

  async listUncompressedTools(): Promise<Tool[]> {
    await this.ensureTools();
    return [...this.toolCache.values()];
  }

  async buildCompressedDescription(): Promise<string> {
    await this.ensureTools();
    return (this.toolListCache ?? [])
      .map((tool) => formatToolDescription(tool, this.compressionLevel))
      .join("\n");
  }

  getFunctionToolset(): Record<string, WrapperToolHandler> {
    const handlers: Record<string, WrapperToolHandler> = {
      [this.prefixName("get_tool_schema")]: async (input) => {
        const toolName = String(input?.tool_name ?? "");
        return JSON.stringify(await this.getToolSchema(toolName), null, 2);
      },
      [this.prefixName("invoke_tool")]: async (input) => {
        const toolName = String(input?.tool_name ?? "");
        const toolInput = (input?.tool_input as Record<string, unknown> | undefined) ?? undefined;
        return this.invokeTool(toolName, toolInput);
      },
    };

    if (this.compressionLevel === "max") {
      handlers[this.prefixName("list_tools")] = async () =>
        JSON.stringify(await this.listToolNames(), null, 2);
    }

    return handlers;
  }

  /**
   * Return the compressed wrapper tools as AI SDK-compatible tool objects.
   *
   * Each tool has a Zod `parameters` schema, a `description`, and an `execute` function — the
   * shape expected by the Vercel AI SDK `Tool` type and Mastra's `ToolsInput`.  This allows
   * consumers to use the compressed tools directly without any additional bridging code.
   *
   * In normal mode, returns `get_tool_schema` and `invoke_tool` (plus `list_tools` at max
   * compression). In CLI mode, returns a single `{serverName}_help` tool whose description
   * contains the full CLI help text — backend tools are invoked via the CLI script instead.
   */
  async getAiSdkTools(): Promise<Record<string, AiSdkTool>> {
    await this.ensureTools();

    if (this.isCliMode) {
      return this.buildCliAiSdkTools();
    }

    return this.buildStandardAiSdkTools();
  }

  private async buildCliAiSdkTools(): Promise<Record<string, AiSdkTool>> {
    const tools = await this.listUncompressedTools();
    const description = buildHelpToolDescription(
      this.cliName,
      this.serverDescription,
      tools,
      this.cliOnPath,
    );
    const helpToolName = this.prefixName("help");

    return {
      [helpToolName]: {
        description,
        parameters: z.object({}),
        execute: async () => description,
      },
    };
  }

  private async buildStandardAiSdkTools(): Promise<Record<string, AiSdkTool>> {
    const compressedDescription = await this.buildCompressedDescription();

    const tools: Record<string, AiSdkTool> = {
      [this.prefixName("get_tool_schema")]: {
        description:
          `Get the input schema for a specific tool from ${this.serverDescription}.\n\n` +
          `Available tools are:\n${compressedDescription}`,
        parameters: z.object({
          tool_name: z.string().describe("The name of the tool to get the schema for."),
        }),
        execute: async (args: { tool_name: string }) =>
          JSON.stringify(await this.getToolSchema(args.tool_name), null, 2),
      },
      [this.prefixName("invoke_tool")]: {
        description: `Invoke a tool from ${this.serverDescription}.`,
        parameters: z.object({
          tool_name: z.string().describe("The name of the tool to invoke."),
          tool_input: z
            .record(z.string(), z.unknown())
            .optional()
            .describe(
              "The input to the tool. Schemas can be retrieved using the appropriate `get_tool_schema` function.",
            ),
        }),
        execute: async (args: { tool_name: string; tool_input?: Record<string, unknown> }) =>
          this.invokeTool(args.tool_name, args.tool_input),
      },
    };

    if (this.compressionLevel === "max") {
      tools[this.prefixName("list_tools")] = {
        description: `List all available tools in ${this.serverDescription}.`,
        parameters: z.object({}),
        execute: async () => JSON.stringify(await this.listToolNames(), null, 2),
      };
    }

    return tools;
  }

  private async ensureTools(): Promise<void> {
    if (this.toolListCache === null) {
      await this.refreshTools();
    }
  }

  private async getBackendTool(toolName: string): Promise<Tool> {
    await this.ensureTools();
    const tool = this.toolCache.get(toolName);
    if (!tool) {
      throw new ToolNotFoundError(toolName, [...this.toolCache.keys()]);
    }
    return tool;
  }

  private prefixName(name: string): string {
    return this.serverName ? `${this.serverName}_${name}` : name;
  }

  private shouldIncludeTool(toolName: string): boolean {
    if (this.excludeTools.has(toolName)) {
      return false;
    }
    if (this.includeTools && !this.includeTools.has(toolName)) {
      return false;
    }
    return true;
  }
}
