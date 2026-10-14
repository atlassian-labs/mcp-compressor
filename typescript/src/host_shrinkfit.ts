import { randomUUID } from "node:crypto";
import type { ExecutableTool } from "./adapters.js";
import { loadNativeCore } from "./native.js";
import { normalizeHostToolResult } from "./rust_core.js";

export interface HostShrinkfitOptions {
  /** Maximum Unicode characters, including recovery instructions. Disabled by default. */
  shrinkfit?: number;
}

/** Per-toolset recovery: no files, no backend retries, no shared cross-session cache. */
export function shrinkfitTools(
  tools: Record<string, ExecutableTool<unknown>>,
  options: HostShrinkfitOptions = {},
): { tools: Record<string, ExecutableTool<unknown>>; close(): void } {
  const limit = options.shrinkfit;
  if (limit === undefined) return { tools, close() {} };
  if (!Number.isInteger(limit) || limit <= 0 || limit > 0xffffffff) {
    throw new Error("shrinkfit must be a positive integer no greater than 4294967295");
  }
  // Avoid collisions with genuine upstream tools, including CLI kebab aliases.
  let recoveryName = "compressor_recover_response";
  const normalizeName = (name: string) => name.replace(/[^a-z0-9]/giu, "").toLowerCase();
  const names = new Set(Object.keys(tools).map(normalizeName));
  let suffix = 2;
  while (names.has(normalizeName(recoveryName)))
    recoveryName = `compressor_recover_response_${suffix++}`;
  const cache = new Map<string, { output: string; bytes: number; expires: number }>();
  const maxBytes = 64 * 1024 * 1024;
  let bytes = 0;
  let closed = false;
  function remove(id: string) {
    const entry = cache.get(id);
    if (entry) bytes -= entry.bytes;
    cache.delete(id);
  }
  function prune() {
    for (const [id, entry] of cache) if (entry.expires <= Date.now()) remove(id);
  }
  const result: Record<string, ExecutableTool<unknown>> = {};
  for (const [name, tool] of Object.entries(tools)) {
    result[name] = {
      ...tool,
      execute: async (input) => {
        if (closed) throw new Error("Shrinkfit toolset is closed");
        const value = await tool.execute(input);
        // Error and multimodal envelopes must retain their original semantics.
        if (preserveEnvelope(value)) return value;
        const output = normalizeHostToolResult(value, false);
        if (output.length <= limit || withinLimit(output, limit)) return value;
        const id = randomUUID();
        const message = `This response was automatically shrunk to limit token consumption. Only if more detail is required, call ${recoveryName} with response_id=${id} (CLI: --response-id ${id}). Recovery returns the cached original without repeating the upstream call. Cache expires after 5 minutes and may be evicted earlier.`;
        let preview: string | null;
        try {
          preview = await shrinkOutput(output, limit, message);
        } catch (error) {
          throw new Error(
            `Could not shrink response after upstream execution; do not retry a write. ${error instanceof Error ? error.message : String(error)}`,
          );
        }
        if (preview === null) return value;
        if (closed) throw new Error("Shrinkfit toolset was closed during execution");
        const size = Buffer.byteLength(output, "utf8");
        if (size > maxBytes)
          throw new Error(
            "Response exceeds shrinkfit recovery cache capacity; upstream call already executed. Do not retry a write.",
          );
        prune();
        while (cache.size >= 128 || bytes + size > maxBytes) remove(cache.keys().next().value!);
        cache.set(id, { output, bytes: size, expires: Date.now() + 300_000 });
        bytes += size;
        return preview;
      },
    };
  }
  result[recoveryName] = {
    name: recoveryName,
    description:
      "Retrieve a complete cached response only when its shortened preview lacks required information. Never repeats an upstream call. IDs are scoped to this toolset; expired or evicted IDs fail safely.",
    inputSchema: {
      type: "object",
      properties: {
        response_id: { type: "string", description: "Response ID from the truncation notice." },
      },
      required: ["response_id"],
      additionalProperties: false,
    },
    execute: async (input) => {
      prune();
      const id = (input as { response_id?: unknown } | undefined)?.response_id;
      const entry = typeof id === "string" && !closed ? cache.get(id) : undefined;
      if (!entry)
        throw new Error(
          "Recovery response unavailable (unknown, expired, evicted, or closed). The upstream tool was NOT called.",
        );
      return entry.output;
    },
  };
  return {
    tools: result,
    close() {
      closed = true;
      cache.clear();
      bytes = 0;
    },
  };
}

function preserveEnvelope(value: unknown): boolean {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const object = value as Record<string, unknown>;
  if (Object.keys(object).length === 1 && "result" in object)
    return preserveEnvelope(object.result);
  if (object.isError === true) return true;
  return (
    Array.isArray(object.content) &&
    object.content.some(
      (block) =>
        !block || typeof block !== "object" || (block as { type?: string }).type !== "text",
    )
  );
}

function withinLimit(output: string, limit: number): boolean {
  let count = 0;
  for (const _char of output) if (++count > limit) return false;
  return true;
}

// Bound active CPU jobs across toolsets (N-API itself executes these off-thread).
let activeJobs = 0;
const waiters: Array<() => void> = [];
async function shrinkOutput(
  output: string,
  limit: number,
  message: string,
): Promise<string | null> {
  if (activeJobs >= 4) await new Promise<void>((resolve) => waiters.push(resolve));
  else activeJobs++;
  try {
    return await loadNativeCore().shrinkHostOutput(output, limit, message);
  } finally {
    const next = waiters.shift();
    if (next) next();
    else activeJobs--;
  }
}
