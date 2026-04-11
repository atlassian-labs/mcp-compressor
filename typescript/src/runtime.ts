import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import { ToolNotFoundError } from "./errors.js";
import { formatCliToolResult, formatToolDescription, formatToolResult } from "./formatting.js";
import type { BackendToolClient, CommonProxyOptions, CompressionLevel } from "./types.js";

export const UNCOMPRESSED_RESOURCE_URI = "compressor://uncompressed-tools";

export interface CompressorRuntimeOptions extends CommonProxyOptions {
  backendClient: BackendToolClient;
}

export type WrapperToolHandler = (input?: Record<string, unknown>) => Promise<string>;

export class CompressorRuntime {
  readonly backendClient: BackendToolClient;
  private readonly compressionLevel: CompressionLevel;
  private readonly excludeTools: Set<string>;
  private readonly includeTools: Set<string> | null;
  private readonly serverName?: string;
  private readonly toonify: boolean;
  private toolCache = new Map<string, Tool>();
  private toolListCache: Tool[] | null = null;

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
  }

  async connect(): Promise<void> {
    await this.backendClient.connect();
    await this.refreshTools();
  }

  async close(): Promise<void> {
    await this.backendClient.close();
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
