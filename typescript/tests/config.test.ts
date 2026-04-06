import test from 'node:test';
import assert from 'node:assert/strict';

import { parseSingleServerConfigJson } from '../src/config.js';

test('parseSingleServerConfigJson parses a single stdio server', () => {
  const parsed = parseSingleServerConfigJson(
    '{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch"]}}}',
  );

  assert.deepEqual(parsed, {
    backend: {
      type: 'stdio',
      command: 'uvx',
      args: ['mcp-server-fetch'],
      cwd: undefined,
      env: undefined,
    },
    serverName: 'fetch',
  });
});

test('parseSingleServerConfigJson rejects multiple servers', () => {
  assert.throws(
    () => parseSingleServerConfigJson('{"mcpServers":{"a":{"command":"uvx"},"b":{"command":"uvx"}}}'),
    /exactly one server/i,
  );
});
