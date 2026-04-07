import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import type { Tool } from '@modelcontextprotocol/sdk/types.js';

import { CliBridge } from '../src/cli_bridge.js';
import { generateCliScript, removeCliScript, removeCliScriptEntry } from '../src/cli_script.js';
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

test('generateCliScript writes a launcher script', async () => {
  const server = await startHealthServer();
  const scriptDir = await fs.mkdtemp(path.join(os.tmpdir(), 'mcp-compressor-cli-mode-'));
  const cliName = 'atlassian';
  try {
    const generated = await generateCliScript(cliName, server.port, 12345, scriptDir);
    assert.equal(generated.scriptPath, path.join(scriptDir, process.platform === 'win32' ? `${cliName}.cmd` : cliName));
    const content = await fs.readFile(generated.scriptPath, 'utf8');
    assert.match(content, /BRIDGES_JSON=/);
    if (process.platform === 'win32') {
      assert.match(content, /Get-Process -Id \$PID/);
      assert.match(content, /ContainsKey\(\$proc.Id\)/);
    } else {
      assert.match(content, /^#!\/usr\/bin\/env bash/m);
      assert.match(content, /declare -A BRIDGES/);
      assert.match(content, /ps -o ppid=/);
      assert.match(content, /curl -sS -o/);
    }
    assert.match(content, /mcp-compressor ts cli-mode script/);
  } finally {
    await stopHealthServer(server);
  }
});

test('same-name CLI sessions share a script registry and remove only their own entry', async () => {
  const server1 = await startHealthServer();
  const server2 = await startHealthServer();
  const scriptDir = await fs.mkdtemp(path.join(os.tmpdir(), 'mcp-compressor-cli-mode-shared-'));
  const cliName = 'atlassian';
  const sessionPid1 = 11111;
  const sessionPid2 = 22222;
  const scriptPath = path.join(scriptDir, process.platform === 'win32' ? `${cliName}.cmd` : cliName);

  try {
    await generateCliScript(cliName, server1.port, sessionPid1, scriptDir);
    await generateCliScript(cliName, server2.port, sessionPid2, scriptDir);

    let content = await fs.readFile(scriptPath, 'utf8');
    assert.match(content, new RegExp(`BRIDGES_JSON=.*\"${sessionPid1}\"`));
    assert.match(content, new RegExp(`BRIDGES_JSON=.*\"${sessionPid2}\"`));
    assert.match(content, new RegExp(server1.url.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
    assert.match(content, new RegExp(server2.url.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));

    await removeCliScriptEntry(cliName, sessionPid1, scriptDir);
    content = await fs.readFile(scriptPath, 'utf8');
    assert.doesNotMatch(content, new RegExp(`BRIDGES_JSON=.*\"${sessionPid1}\"`));
    assert.match(content, new RegExp(`BRIDGES_JSON=.*\"${sessionPid2}\"`));

    await removeCliScriptEntry(cliName, sessionPid2, scriptDir);
    await assert.rejects(() => fs.stat(scriptPath));
  } finally {
    await stopHealthServer(server1);
    await stopHealthServer(server2);
    await removeCliScript(scriptPath);
  }
});

async function startHealthServer(): Promise<{ port: number; server: http.Server; url: string }> {
  const server = http.createServer((request, response) => {
    if (request.url === '/health') {
      response.statusCode = 200;
      response.end('ok');
      return;
    }
    response.statusCode = 404;
    response.end('not found');
  });
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => resolve());
  });
  const address = server.address();
  if (!address || typeof address === 'string') {
    throw new Error('failed to start test health server');
  }
  return { port: address.port, server, url: `http://127.0.0.1:${address.port}` };
}

async function stopHealthServer(handle: { server: http.Server }): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    handle.server.close((error) => {
      if (error) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}
