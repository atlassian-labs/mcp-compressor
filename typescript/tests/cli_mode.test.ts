import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import type { Tool } from '@modelcontextprotocol/sdk/types.js';

import { CliBridge } from '../src/cli_bridge.js';
import { generateCliScript, removeCliScript } from '../src/cli_script.js';
import { parseArgvToToolInput, toolNameToSubcommand } from '../src/cli_tools.js';
import { CompressorRuntime } from '../src/runtime.js';
import type { BackendToolClient } from '../src/types.js';

class FakeBackendClient implements BackendToolClient {
  callToolCalls: Array<{ name: string; args: Record<string, unknown> | undefined }> = [];

  constructor(private readonly tools: Tool[]) {}

  async connect(): Promise<void> {}
  async close(): Promise<void> {}
  async listTools(): Promise<Tool[]> {
    return this.tools;
  }
  async callTool(name: string, args: Record<string, unknown> | undefined): Promise<unknown> {
    this.callToolCalls.push({ name, args });
    return { name, args };
  }
}

const TOOL: Tool = {
  name: 'searchConfluence',
  description: 'Search confluence content. Returns matching documents.',
  inputSchema: {
    type: 'object',
    properties: {
      query: { type: 'string' },
      limit: { type: 'integer' },
      dry_run: { type: 'boolean' },
    },
    required: ['query'],
  },
} as Tool;

test('toolNameToSubcommand and parseArgvToToolInput support CLI flags', () => {
  assert.equal(toolNameToSubcommand('searchConfluence'), 'search-confluence');
  assert.deepEqual(parseArgvToToolInput(['--query', 'oauth', '--limit', '3', '--dry-run'], TOOL), {
    query: 'oauth',
    limit: 3,
    dry_run: true,
  });
  assert.deepEqual(parseArgvToToolInput(['--json', '{"query":"oauth"}'], TOOL), { query: 'oauth' });
});

test('CliBridge serves shell-friendly help and invokes tools', async () => {
  const backendClient = new FakeBackendClient([TOOL]);
  const runtime = new CompressorRuntime({ backendClient, compressionLevel: 'max', serverName: 'atlassian' });
  await runtime.connect();

  const bridge = new CliBridge(runtime, 'atlassian');
  await bridge.start(0);

  const topLevelHelp = await fetch(`${bridge.url}/help`);
  assert.equal(topLevelHelp.status, 200);
  assert.match(await topLevelHelp.text(), /search-confluence/);

  const toolHelp = await fetch(`${bridge.url}/tools/search-confluence/help`);
  assert.equal(toolHelp.status, 200);
  assert.match(await toolHelp.text(), /--query/);

  const form = new URLSearchParams();
  form.append('argv', '--query');
  form.append('argv', 'oauth');
  form.append('argv', '--limit');
  form.append('argv', '2');
  const invokeResponse = await fetch(`${bridge.url}/tools/search-confluence`, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded;charset=UTF-8' },
    body: form,
  });
  assert.equal(invokeResponse.status, 200);
  assert.match(await invokeResponse.text(), /searchConfluence/);
  assert.deepEqual(backendClient.callToolCalls, [{ name: 'searchConfluence', args: { query: 'oauth', limit: 2 } }]);

  await bridge.close();
  await runtime.close();
});

test('generateCliScript writes and removes a local script', async () => {
  const scriptDir = await fs.mkdtemp(path.join(os.tmpdir(), 'mcp-compressor-cli-mode-'));
  const cliName = 'atlassian';
  const generated = await generateCliScript(cliName, 'http://127.0.0.1:43210', scriptDir);
  assert.equal(generated.scriptPath, path.join(scriptDir, process.platform === 'win32' ? `${cliName}.cmd` : cliName));
  const content = await fs.readFile(generated.scriptPath, 'utf8');
  if (process.platform === 'win32') {
    assert.match(content, /node -e/);
    assert.match(content, / -- %\*/);
  } else {
    assert.match(content, /^#!\/usr\/bin\/env bash/m);
    assert.match(content, /curl -fsS "\$BRIDGE_URL\/help"/);
    assert.match(content, /curl -fsS -X POST "\$BRIDGE_URL\/tools\/\$subcommand"/);
    assert.match(content, /--data-urlencode "argv=\$arg"/);
  }
  assert.match(content, /mcp-compressor ts cli-mode script/);
  await removeCliScript(generated.scriptPath);
  await assert.rejects(() => fs.stat(generated.scriptPath));
});
