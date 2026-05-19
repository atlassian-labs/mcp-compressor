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
  return typeof value === "string" ? value : JSON.stringify(value);
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
