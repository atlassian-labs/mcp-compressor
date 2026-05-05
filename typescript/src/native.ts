import { createRequire } from "node:module";

export interface NativeRustTool {
  name: string;
  description?: string | null;
  input_schema: Record<string, unknown>;
}

export interface NativeCore {
  compressToolListingJson(level: string, toolsJson: string): string;
  formatToolSchemaResponseJson(toolJson: string): string;
  parseToolArgvJson(toolJson: string, argvJson: string): string;
  generateClientArtifactsJson(kind: string, configJson: string): string;
  parseMcpConfigJson(configJson: string): string;
  listOauthCredentialsJson(): string;
  clearOauthCredentialsJson(target?: string | null): string;
  startCompressedSessionJson(
    configJson: string,
    backendsJson: string,
  ): Promise<NativeCompressedSession>;
  startCompressedSessionFromMcpConfigJson(
    configJson: string,
    mcpConfigJson: string,
  ): Promise<NativeCompressedSession>;
}

export interface NativeCompressedSession {
  infoJson(): string;
}

const require = createRequire(import.meta.url);

export function loadNativeCore(): NativeCore {
  try {
    return require("../native/index.js") as NativeCore;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(
      `Rust native addon is not available. Run \`bun run build:native\` before using Rust-backed helpers. Cause: ${message}`,
    );
  }
}
