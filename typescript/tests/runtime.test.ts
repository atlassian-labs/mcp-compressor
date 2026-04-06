import test from 'node:test';
import assert from 'node:assert/strict';
import type { Tool } from '@modelcontextprotocol/sdk/types.js';

import { CompressorRuntime } from '../src/runtime.js';
import type { BackendToolClient } from '../src/types.js';

class FakeBackendClient implements BackendToolClient {
  connectCalls = 0;
  closeCalls = 0;
  listToolsCalls = 0;
  callToolCalls: Array<{ name: string; args: Record<string, unknown> | undefined }> = [];

  constructor(
    private readonly tools: Tool[],
    private readonly result: unknown = { ok: true },
  ) {}

  async connect(): Promise<void> {
    this.connectCalls += 1;
  }

  async close(): Promise<void> {
    this.closeCalls += 1;
  }

  async listTools(): Promise<Tool[]> {
    this.listToolsCalls += 1;
    return this.tools;
  }

  async callTool(name: string, args: Record<string, unknown> | undefined): Promise<unknown> {
    this.callToolCalls.push({ name, args });
    return this.result;
  }
}

const SAMPLE_TOOLS: Tool[] = [
  {
    name: 'search_docs',
    description: 'Search documentation. Returns matching pages.',
    inputSchema: {
      type: 'object',
      properties: {
        query: { type: 'string' },
      },
    },
  } as Tool,
  {
    name: 'create_ticket',
    description: 'Create a ticket.',
    inputSchema: {
      type: 'object',
      properties: {
        summary: { type: 'string' },
      },
    },
  } as Tool,
];

test('CompressorRuntime exposes wrapper operations in-process', async () => {
  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: 'max',
    includeTools: ['search_docs'],
    serverName: 'docs',
  });

  await runtime.connect();

  assert.equal(backendClient.connectCalls, 1);
  assert.deepEqual(await runtime.listToolNames(), ['search_docs']);
  assert.equal((await runtime.getToolSchema('search_docs')).name, 'search_docs');
  assert.match(await runtime.buildCompressedDescription(), /search_docs/);
  assert.equal(
    await runtime.invokeTool('search_docs', { query: 'oauth' }),
    JSON.stringify({ ok: true }, null, 2),
  );
  assert.deepEqual(backendClient.callToolCalls, [{ name: 'search_docs', args: { query: 'oauth' } }]);

  await runtime.close();
  assert.equal(backendClient.closeCalls, 1);
});

test('CompressorRuntime function toolset mirrors wrapper tools', async () => {
  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    compressionLevel: 'max',
    serverName: 'docs',
  });
  await runtime.connect();

  const toolset = runtime.getFunctionToolset();
  assert.deepEqual(Object.keys(toolset).sort(), ['docs_get_tool_schema', 'docs_invoke_tool', 'docs_list_tools']);
  assert.match(await toolset.docs_get_tool_schema({ tool_name: 'search_docs' }), /search_docs/);
  assert.match(await toolset.docs_list_tools(), /search_docs/);
  assert.equal(
    await toolset.docs_invoke_tool({ tool_name: 'search_docs', tool_input: { query: 'mcp' } }),
    JSON.stringify({ ok: true }, null, 2),
  );
});

test('CompressorRuntime treats an empty includeTools list as no include filter', async () => {
  const backendClient = new FakeBackendClient(SAMPLE_TOOLS);
  const runtime = new CompressorRuntime({
    backendClient,
    includeTools: [],
  });

  await runtime.connect();

  assert.deepEqual(await runtime.listToolNames(), ['create_ticket', 'search_docs']);
});
