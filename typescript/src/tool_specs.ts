import type { ExecutableTool } from "./adapters.js";
import type { ToolSpec } from "./rust_core.js";

export function executableToolToSpec(name: string, tool: ExecutableTool<unknown>): ToolSpec {
  return {
    name,
    description: tool.description,
    inputSchema: tool.inputSchema,
  };
}

export function executableToolsToSpecs(tools: Record<string, ExecutableTool<unknown>>): ToolSpec[] {
  return Object.entries(tools).map(([name, tool]) => executableToolToSpec(name, tool));
}

export function normalizeServerName(name: string | undefined): string {
  const value = name ?? "tools";
  const normalized = value
    .replace(/[^A-Za-z0-9_]+/gu, "_")
    .replace(/^_+|_+$/gu, "")
    .toLowerCase();
  return normalized || "tools";
}

export function stringifyToolResult(value: unknown): string {
  if (typeof value === "string") return value;
  const unwrapped = unwrapSingleResultWrapper(value);
  if (typeof unwrapped === "string") return unwrapped;
  const mcpText = stringifyMcpTextContent(unwrapped);
  if (mcpText !== undefined) return mcpText;
  return JSON.stringify(unwrapped);
}

function unwrapSingleResultWrapper(value: unknown): unknown {
  if (!value || typeof value !== "object" || Array.isArray(value)) return value;
  const entries = Object.entries(value as Record<string, unknown>);
  return entries.length === 1 && entries[0]?.[0] === "result" ? entries[0][1] : value;
}

function stringifyMcpTextContent(value: unknown): string | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const content = (value as { readonly content?: unknown }).content;
  if (!Array.isArray(content)) return undefined;
  const textParts = content.flatMap((item) => {
    if (!item || typeof item !== "object" || Array.isArray(item)) return [];
    const candidate = item as { readonly type?: unknown; readonly text?: unknown };
    return candidate.type === "text" && typeof candidate.text === "string" ? [candidate.text] : [];
  });
  return textParts.length > 0 ? textParts.join("\n") : undefined;
}

export function normalizeStructuredArgValues(
  schema: Record<string, unknown>,
  input: Record<string, unknown>,
): Record<string, unknown> {
  const properties = schema.properties;
  if (!properties || typeof properties !== "object" || Array.isArray(properties)) return input;
  const normalized: Record<string, unknown> = { ...input };
  for (const [key, propertySchema] of Object.entries(properties as Record<string, unknown>)) {
    const value = normalized[key];
    if (typeof value !== "string") continue;
    if (!expectsStructuredValue(propertySchema)) continue;
    const trimmed = value.trim();
    if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) continue;
    try {
      normalized[key] = JSON.parse(trimmed) as unknown;
    } catch {
      // Leave the original string so downstream validation/error handling can report it.
    }
  }
  return normalized;
}

function expectsStructuredValue(schema: unknown): boolean {
  if (!schema || typeof schema !== "object" || Array.isArray(schema)) return false;
  const type = (schema as { type?: unknown }).type;
  if (type === "object" || type === "array") return true;
  return Array.isArray(type) && (type.includes("object") || type.includes("array"));
}
