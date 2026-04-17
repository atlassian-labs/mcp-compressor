import { InvalidConfigurationError } from "./errors.js";
import type { BackendConfig, JsonConfigServerEntry, MCPConfigShape } from "./types.js";

/**
 * Interpolate environment variables in a single string using ${VAR_NAME} or $VAR_NAME syntax.
 * If a referenced variable is not set, the placeholder is left as-is (matching Python behaviour).
 */
export function interpolateString(value: string): string {
  if (!value.includes("$")) {
    return value;
  }
  return value.replace(
    /\$\{([^}]+)\}|\$([A-Za-z_][A-Za-z0-9_]*)/g,
    (match, braced?: string, bare?: string) => {
      const varName = braced ?? bare ?? "";
      const envValue = process.env[varName];
      return envValue !== undefined ? envValue : match;
    },
  );
}

export function interpolateRecord(
  record: Record<string, string> | undefined,
): Record<string, string> | undefined {
  if (!record) return record;
  const result: Record<string, string> = {};
  for (const [key, value] of Object.entries(record)) {
    result[key] = interpolateString(value);
  }
  return result;
}

/**
 * Interpolate environment variables in all string values of an MCP config object or JSON string.
 *
 * Accepts either a parsed `MCPConfigShape` object or a raw JSON string and returns an interpolated
 * copy with `${VAR_NAME}` and `$VAR_NAME` placeholders replaced by their environment variable
 * values. Unset variables are left as-is.
 *
 * @example
 * ```ts
 * const config = interpolateMCPConfig({
 *   mcpServers: {
 *     myServer: { url: "https://example.com", headers: { Authorization: "Bearer $MY_TOKEN" } },
 *   },
 * });
 * ```
 */
export function interpolateMCPConfig(config: MCPConfigShape | string): MCPConfigShape {
  const parsed: MCPConfigShape =
    typeof config === "string" ? (JSON.parse(config) as MCPConfigShape) : config;

  const interpolated: MCPConfigShape = { mcpServers: {} };
  for (const [name, entry] of Object.entries(parsed.mcpServers ?? {})) {
    interpolated.mcpServers[name] = {
      ...entry,
      ...(entry.url !== undefined ? { url: interpolateString(String(entry.url)) } : {}),
      ...(entry.args ? { args: entry.args.map(interpolateString) } : {}),
      ...(entry.env ? { env: interpolateRecord(entry.env) } : {}),
      ...(entry.headers ? { headers: interpolateRecord(entry.headers) } : {}),
    };
  }
  return interpolated;
}

/**
 * Parse an MCP config JSON string containing one or more servers.
 *
 * Returns an array of `{ backend, serverName }` entries — one per server in `mcpServers` — or
 * `null` if the input is not a JSON object string.  Throws {@link InvalidConfigurationError} for
 * malformed JSON or an empty `mcpServers` map.
 */
export function parseServerConfigJson(
  input: string,
): Array<{ backend: BackendConfig; serverName: string }> | null {
  const trimmed = input.trim();
  if (!trimmed.startsWith("{")) {
    return null;
  }

  let parsed: MCPConfigShape;
  try {
    parsed = JSON.parse(trimmed) as MCPConfigShape;
  } catch (error) {
    throw new InvalidConfigurationError(`Invalid MCP config JSON: ${(error as Error).message}`);
  }

  const names = Object.keys(parsed.mcpServers ?? {});
  if (names.length === 0) {
    throw new InvalidConfigurationError(
      "MCP config JSON must contain at least one server in mcpServers.",
    );
  }

  return names.map((serverName) => ({
    backend: normalizeConfigServer(parsed.mcpServers[serverName]!),
    serverName,
  }));
}

export function normalizeConfigServer(entry: JsonConfigServerEntry): BackendConfig {
  const interpolated = interpolateMCPConfig({ mcpServers: { _: entry } }).mcpServers["_"]!;

  if (interpolated.command) {
    return {
      type: "stdio",
      command: interpolated.command,
      args: interpolated.args,
      cwd: interpolated.cwd,
      env: interpolated.env,
    };
  }

  if (!interpolated.url) {
    throw new InvalidConfigurationError("Server config must contain either command or url.");
  }

  return {
    type: interpolated.transport === "sse" ? "sse" : "http",
    url: interpolated.url.toString(),
    headers: interpolated.headers,
  };
}
