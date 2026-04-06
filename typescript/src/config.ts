import { InvalidConfigurationError } from './errors.js';
import type { BackendConfig, JsonConfigServerEntry, MCPConfigShape } from './types.js';

export function parseSingleServerConfigJson(input: string): { backend: BackendConfig; serverName: string } | null {
  const trimmed = input.trim();
  if (!trimmed.startsWith('{')) {
    return null;
  }

  let parsed: MCPConfigShape;
  try {
    parsed = JSON.parse(trimmed) as MCPConfigShape;
  } catch (error) {
    throw new InvalidConfigurationError(`Invalid MCP config JSON: ${(error as Error).message}`);
  }

  const names = Object.keys(parsed.mcpServers ?? {});
  if (names.length !== 1) {
    throw new InvalidConfigurationError('MCP config JSON must contain exactly one server in mcpServers.');
  }

  const serverName = names[0]!;
  return { backend: normalizeConfigServer(parsed.mcpServers[serverName]!), serverName };
}

export function normalizeConfigServer(entry: JsonConfigServerEntry): BackendConfig {
  if (entry.command) {
    return {
      type: 'stdio',
      command: entry.command,
      args: entry.args,
      cwd: entry.cwd,
      env: entry.env,
    };
  }

  if (!entry.url) {
    throw new InvalidConfigurationError('Single-server MCP config must contain either command or url.');
  }

  return {
    type: entry.transport === 'sse' ? 'sse' : 'http',
    url: entry.url,
    headers: entry.headers,
  };
}
