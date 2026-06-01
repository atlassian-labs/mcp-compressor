import { compressToolListing, formatToolSchemaResponse, type ToolSpec } from "./rust_core.js";
import type { CompressionLevel } from "./types.js";
import type { ExecutableTool } from "./adapters.js";
import { stringifyToolResult } from "./tool_specs.js";

export interface LocalTool<TInput = Record<string, unknown>, TResult = unknown> {
  description?: string;
  inputSchema: unknown;
  execute: (input: TInput) => TResult | Promise<TResult>;
}

export interface CompressToolsOptions {
  compressionLevel?: CompressionLevel;
  namePrefix?: string;
  toonify?: boolean;
  schemaAdapter?: (schema: unknown) => Record<string, unknown>;
}

function wrapperName(prefix: string | undefined, name: string): string {
  return prefix ? `${prefix}_${name}` : name;
}

function asJsonSchema(
  schema: unknown,
  adapter: ((schema: unknown) => Record<string, unknown>) | undefined,
): Record<string, unknown> {
  if (schema && typeof schema === "object" && !Array.isArray(schema)) {
    return schema as Record<string, unknown>;
  }
  if (adapter) {
    return adapter(schema);
  }
  throw new Error(
    "Tool inputSchema must be a JSON schema object or schemaAdapter must be provided",
  );
}

function normalizeResult(value: unknown, toonify: boolean): string {
  const json = stringifyToolResult(value);
  if (!toonify) return json;
  // Keep local compression dependency-light for now. Runtime MCP proxy paths use
  // Rust TOON support; local in-process tools return JSON-compatible strings.
  return json;
}

export function compressTools(
  tools: Record<string, LocalTool>,
  options: CompressToolsOptions = {},
): Record<string, ExecutableTool> {
  const compressionLevel = options.compressionLevel ?? "medium";
  const specs: ToolSpec[] = Object.entries(tools).map(([name, tool]) => ({
    name,
    description: tool.description,
    inputSchema: asJsonSchema(tool.inputSchema, options.schemaAdapter),
  }));
  const specByName = new Map(specs.map((spec) => [spec.name, spec]));
  const toolByName = new Map(Object.entries(tools));
  const listing = compressToolListing(compressionLevel, specs);
  const result: Record<string, ExecutableTool> = {};

  if (compressionLevel === "max") {
    result[wrapperName(options.namePrefix, "list_tools")] = {
      name: wrapperName(options.namePrefix, "list_tools"),
      description: "List backend tools available through this compressed toolset.",
      inputSchema: { type: "object", properties: {} },
      execute: async () => listing,
    };
  }

  result[wrapperName(options.namePrefix, "get_tool_schema")] = {
    name: wrapperName(options.namePrefix, "get_tool_schema"),
    description: `Get the complete schema and description for one tool. Available tools:\n${listing}`,
    inputSchema: {
      type: "object",
      properties: {
        tool_name: { type: "string", description: "Name of the tool" },
      },
      required: ["tool_name"],
    },
    execute: async (input = {}) => {
      const toolName = String(input.tool_name ?? "");
      const spec = specByName.get(toolName);
      if (!spec) throw new Error(`Tool not found: ${toolName}`);
      return formatToolSchemaResponse(spec);
    },
  };

  result[wrapperName(options.namePrefix, "invoke_tool")] = {
    name: wrapperName(options.namePrefix, "invoke_tool"),
    description: "Invoke one tool by name with JSON input.",
    inputSchema: {
      type: "object",
      properties: {
        tool_name: { type: "string", description: "Name of the tool" },
        tool_input: {
          type: "object",
          description: "JSON input for the tool",
          properties: {},
          additionalProperties: true,
        },
      },
      required: ["tool_name", "tool_input"],
    },
    execute: async (input = {}) => {
      const toolName = String(input.tool_name ?? "");
      const tool = toolByName.get(toolName);
      if (!tool) throw new Error(`Tool not found: ${toolName}`);
      const output = await tool.execute((input.tool_input ?? {}) as never);
      return normalizeResult(output, options.toonify ?? false);
    },
  };

  return result;
}
