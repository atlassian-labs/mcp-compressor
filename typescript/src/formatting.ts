import { encode } from "@toon-format/toon";
import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import type { CompressionLevel } from "./types.js";

export function formatToolDescription(tool: Tool, compressionLevel: CompressionLevel): string {
  const description = tool.description ?? "";
  const inputSchema = tool.inputSchema as { properties?: Record<string, unknown> } | undefined;
  const parameterNames = Object.keys(inputSchema?.properties ?? {});

  const params = `(${parameterNames.join(", ")})`;
  const descSuffix = (desc: string) => (desc ? `: ${desc}` : "");

  switch (compressionLevel) {
    case "low":
      return `<tool>${tool.name}${params}${descSuffix(description)}</tool>`;
    case "medium": {
      const firstLine = description.split("\n")[0] ?? description;
      const firstSentence = firstLine.split(".")[0]?.trim() ?? firstLine;
      return `<tool>${tool.name}${params}${descSuffix(firstSentence)}</tool>`;
    }
    case "high":
      return `<tool>${tool.name}${params}</tool>`;
    case "max":
      return `<tool>${tool.name}</tool>`;
  }
}

function maybeEncodeJsonValue(value: unknown): string | null {
  if (!Array.isArray(value) && (typeof value !== "object" || value === null)) {
    return null;
  }
  return encode(value as object | object[]);
}

function maybeToonifyJsonText(text: string): string {
  try {
    const parsed = JSON.parse(text) as unknown;
    return maybeEncodeJsonValue(parsed) ?? text;
  } catch {
    return text;
  }
}

export function maybeToonifyText(text: string, enabled: boolean): string {
  if (!enabled) {
    return text;
  }
  return maybeToonifyJsonText(text);
}

function formatContentBlocks(
  result: { content: Array<Record<string, unknown>> },
  enabled: boolean,
): { output: string; changed: boolean } {
  let changed = false;
  const parts = result.content.map((block) => {
    if (block.type === "text" && typeof block.text === "string") {
      const convertedText = enabled ? maybeToonifyJsonText(block.text) : block.text;
      if (convertedText !== block.text) {
        changed = true;
      }
      return convertedText;
    }
    return `[${String(block.type ?? "content")} content]`;
  });
  return { output: parts.join("\n"), changed };
}

export function formatCliToolResult(result: unknown, enabled: boolean): string {
  if (
    result &&
    typeof result === "object" &&
    Array.isArray((result as { content?: unknown }).content)
  ) {
    const { output } = formatContentBlocks(
      result as { content: Array<Record<string, unknown>> },
      enabled,
    );
    return output;
  }
  return formatToolResult(result, enabled);
}

export function formatToolResult(result: unknown, enabled: boolean): string {
  if (!enabled) {
    return JSON.stringify(result, null, 2);
  }

  if (
    result &&
    typeof result === "object" &&
    Array.isArray((result as { content?: unknown }).content)
  ) {
    const toolResult = result as { content: Array<Record<string, unknown>> };
    const { changed } = formatContentBlocks(toolResult, enabled);
    if (changed) {
      const content = toolResult.content.map((block) => {
        if (block.type === "text" && typeof block.text === "string") {
          return { ...block, text: maybeToonifyJsonText(block.text) };
        }
        return block;
      });
      return JSON.stringify({ ...(result as Record<string, unknown>), content }, null, 2);
    }
  }

  const encoded = maybeEncodeJsonValue(result);
  if (encoded !== null) {
    return encoded;
  }

  return JSON.stringify(result, null, 2);
}
