import { InvalidConfigurationError } from "./errors.js";
import type { BackendConfig, JsonConfigServerEntry, MCPConfigShape } from "./types.js";

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
  if (entry.command) {
    return {
      type: "stdio",
      command: entry.command,
      args: entry.args,
      cwd: entry.cwd,
      env: entry.env,
    };
  }

  if (!entry.url) {
    throw new InvalidConfigurationError("Server config must contain either command or url.");
  }

  return {
    type: entry.transport === "sse" ? "sse" : "http",
    url: entry.url.toString(),
    headers: entry.headers,
  };
}
