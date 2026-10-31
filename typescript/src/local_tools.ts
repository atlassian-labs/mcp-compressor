import { shrinkfitTools, type HostShrinkfitOptions } from "./host_shrinkfit.js";
import {
  compressToolListing,
  formatToolSchemaResponse,
  normalizeHostToolResult,
  type ToolSpec,
} from "./rust_core.js";
import type { CompressionLevel } from "./types.js";
import type { ExecutableTool } from "./adapters.js";
import { stringifyToolResult } from "./tool_specs.js";

export interface LocalTool<TInput = Record<string, unknown>, TResult = unknown> {
  title?: string | null;
  annotations?: Record<string, unknown> | null;
  description?: string;
  inputSchema: unknown;
  execute: (input: TInput) => TResult | Promise<TResult>;
}

export interface CompressToolsOptions extends HostShrinkfitOptions {
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

const TOOL_INPUT_SCHEMA_DESCRIPTION =
  "JSON object matching the selected tool's input schema. Use get_tool_schema for the selected tool_name before invoking if required fields are unknown.";

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function requiredFieldNames(schema: Record<string, unknown>): string[] {
  const required = schema.required;
  if (!Array.isArray(required)) return [];
  return required.filter((field): field is string => typeof field === "string");
}

function validateRequiredToolInput(spec: ToolSpec, toolInput: unknown): void {
  const required = requiredFieldNames(spec.inputSchema);
  if (required.length === 0) return;

  const input = isRecord(toolInput) ? toolInput : {};
  const missing = required.filter((field) => !Object.hasOwn(input, field));
  if (missing.length === 0) return;

  throw new Error(
    `Tool \`${spec.name}\` is missing required tool_input fields: ${missing.join(", ")}. ` +
      "tool_input must be a JSON object matching the selected tool's input schema. " +
      `Call get_tool_schema with tool_name=\`${spec.name}\` and retry with tool_input matching this schema:\n` +
      JSON.stringify(spec.inputSchema, null, 2),
  );
}

function normalizeResult(value: unknown, toonify: boolean): string {
  if (!toonify) return stringifyToolResult(value);
  // Same conversion the proxy and host transforms apply.
  return normalizeHostToolResult(value, true);
}

export function compressTools(
  tools: Record<string, LocalTool>,
  options: CompressToolsOptions = {},
): Record<string, ExecutableTool> {
  const compressionLevel = options.compressionLevel ?? "medium";
  const specs: ToolSpec[] = Object.entries(tools).map(([name, tool]) => ({
    name,
    title: tool.title,
    annotations: tool.annotations,
    description: tool.description,
    inputSchema: asJsonSchema(tool.inputSchema, options.schemaAdapter),
  }));
  if (options.shrinkfit !== undefined) {
    const executable = Object.fromEntries(
      specs.map((spec) => [
        spec.name,
        {
          ...tools[spec.name]!,
          name: spec.name,
          execute: async (input: Record<string, unknown>) => tools[spec.name]!.execute(input),
          inputSchema: spec.inputSchema,
        },
      ]),
    );
    const shrunk = shrinkfitTools(executable, options);
    return compressTools(shrunk.tools, {
      ...options,
      shrinkfit: undefined,
      toonify: false,
      schemaAdapter: undefined,
    });
  }
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
          description: TOOL_INPUT_SCHEMA_DESCRIPTION,
          properties: {},
          additionalProperties: true,
        },
      },
      required: ["tool_name", "tool_input"],
    },
    execute: async (input = {}) => {
      const toolName = String(input.tool_name ?? "");
      const spec = specByName.get(toolName);
      const tool = toolByName.get(toolName);
      if (!spec || !tool) throw new Error(`Tool not found: ${toolName}`);
      const toolInput = input.tool_input ?? {};
      validateRequiredToolInput(spec, toolInput);
      const output = await tool.execute(toolInput as never);
      return normalizeResult(output, options.toonify ?? false);
    },
  };

  return result;
}
