import { createRequire } from "node:module";

export interface NativeToolSpec {
  name: string;
  description?: string | null;
  input_schema: Record<string, unknown>;
}

export interface NativeCore {
  compressToolListingJson(level: string, toolsJson: string): string;
  formatToolSchemaResponseJson(toolJson: string): string;
  maybeToonifyOutputJson(output: string): string;
  parseToolArgvJson(toolJson: string, argvJson: string): string;
  generateClientArtifactsJson(kind: string, configJson: string): string;
  generateClientArtifactFilesJson(kind: string, configJson: string): string;
  normalizeServersJson(serversJson: string): string;
  parseMcpConfigJson(configJson: string): string;
  rememberOauthBackendJson(backendUri: string, backendName: string, storeDir: string): void;
  listOauthCredentialsJson(): string;
  clearOauthCredentialsJson(target?: string | null): string;
  startCompressedSessionJson(
    configJson: string,
    backendsJson: string,
  ): Promise<NativeCompressedSession>;
  startCompressedSessionWithProviderBackendsJson(
    configJson: string,
    backendsJson: string,
    providersJson: string,
  ): Promise<NativeCompressedSession>;
  startCompressedSessionFromMcpConfigJson(
    configJson: string,
    mcpConfigJson: string,
  ): Promise<NativeCompressedSession>;
}

export interface NativeCompressedSession {
  infoJson(): string;
  close(): void;
  updateAuthProviderHeadersJson(providerIndex: number, headersJson: string): void;
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
