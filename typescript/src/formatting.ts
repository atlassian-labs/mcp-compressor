import { compressToolListing, type ToolSpec } from "./rust_core.js";

import type { CompressionLevel } from "./types.js";

export function formatToolDescription(tool: ToolSpec, compressionLevel: CompressionLevel): string {
  return compressToolListing(compressionLevel, [tool]);
}
