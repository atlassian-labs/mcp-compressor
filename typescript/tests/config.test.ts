import test from 'node:test';
import assert from 'node:assert/strict';

import { parseMultiServerConfigJson, parseSingleServerConfigJson } from '../src/config.js';
import { resolveAllBackends } from '../src/index.js';

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

test('parseMultiServerConfigJson parses a single server', () => {
  const parsed = parseMultiServerConfigJson(
    '{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch"]}}}',
  );

  assert.ok(parsed);
  assert.equal(parsed.length, 1);
  assert.deepEqual(parsed[0], {
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

test('parseMultiServerConfigJson parses multiple servers', () => {
  const parsed = parseMultiServerConfigJson(
    '{"mcpServers":{"weather":{"command":"uvx","args":["mcp-weather"]},"calendar":{"command":"uvx","args":["mcp-calendar"]}}}',
  );

  assert.ok(parsed);
  assert.equal(parsed.length, 2);
  assert.equal(parsed[0]!.serverName, 'weather');
  assert.equal(parsed[1]!.serverName, 'calendar');
  assert.deepEqual(parsed[0]!.backend, {
    type: 'stdio',
    command: 'uvx',
    args: ['mcp-weather'],
    cwd: undefined,
    env: undefined,
  });
  assert.deepEqual(parsed[1]!.backend, {
    type: 'stdio',
    command: 'uvx',
    args: ['mcp-calendar'],
    cwd: undefined,
    env: undefined,
  });
});

test('parseMultiServerConfigJson returns null for non-JSON input', () => {
  assert.equal(parseMultiServerConfigJson('uvx mcp-server-fetch'), null);
  assert.equal(parseMultiServerConfigJson('https://example.com/mcp'), null);
});

test('parseMultiServerConfigJson throws for empty mcpServers', () => {
  assert.throws(
    () => parseMultiServerConfigJson('{"mcpServers":{}}'),
    /at least one server/i,
  );
});

test('resolveAllBackends returns a single entry for a plain URL', () => {
  const resolved = resolveAllBackends('https://example.com/mcp');
  assert.equal(resolved.length, 1);
  assert.deepEqual(resolved[0]!.backend, { type: 'http', url: 'https://example.com/mcp' });
});

test('resolveAllBackends returns multiple entries for a multi-server JSON string', () => {
  const resolved = resolveAllBackends(
    '{"mcpServers":{"weather":{"command":"uvx","args":["mcp-weather"]},"calendar":{"command":"uvx","args":["mcp-calendar"]}}}',
  );
  assert.equal(resolved.length, 2);
  assert.equal(resolved[0]!.serverName, 'weather');
  assert.equal(resolved[1]!.serverName, 'calendar');
});

test('resolveAllBackends applies serverName as prefix for multi-server JSON', () => {
  const resolved = resolveAllBackends(
    '{"mcpServers":{"weather":{"command":"uvx"},"calendar":{"command":"uvx"}}}',
    'myapp',
  );
  assert.equal(resolved[0]!.serverName, 'myapp_weather');
  assert.equal(resolved[1]!.serverName, 'myapp_calendar');
});
