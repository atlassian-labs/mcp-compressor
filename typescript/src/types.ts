export type CompressionLevel = "low" | "medium" | "high" | "max";

export interface StdioBackendConfig {
  type: "stdio";
  command: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
}

export interface HttpBackendConfig {
  type: "http";
  url: string;
  headers?: Record<string, string>;
  timeoutMs?: number;
}

export interface SseBackendConfig {
  type: "sse";
  url: string;
  headers?: Record<string, string>;
  timeoutMs?: number;
}

export type BackendConfig = StdioBackendConfig | HttpBackendConfig | SseBackendConfig;

export interface JsonConfigServerEntry {
  command?: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  /** May be a `URL` object or a string — `normalizeConfigServer` coerces to string. */
  url?: URL | string;
  headers?: Record<string, string>;
  transport?: "sse";
}

export interface MCPConfigShape {
  mcpServers: Record<string, JsonConfigServerEntry>;
}
