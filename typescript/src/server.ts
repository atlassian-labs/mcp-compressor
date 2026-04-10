import { FastMCP } from 'fastmcp';
import { z } from 'zod';

import { CompressorRuntime, UNCOMPRESSED_RESOURCE_URI } from './runtime.js';
import type { BackendToolClient, CommonProxyOptions, StartOptions } from './types.js';

export interface CompressorServerOptions extends CommonProxyOptions {
  backendClient: BackendToolClient;
}

export class CompressorServer {
  readonly runtime: CompressorRuntime;
  readonly server: FastMCP;
  private readonly compressionLevel: CommonProxyOptions['compressionLevel'];
  private readonly serverName?: string;

  constructor(options: CompressorServerOptions) {
    this.runtime = new CompressorRuntime(options);
    this.compressionLevel = options.compressionLevel ?? 'medium';
    this.serverName = options.serverName;

    this.server = new FastMCP({
      name: 'MCP Compressor TS',
      version: '0.2.12',
      instructions: 'A compressed MCP proxy server.',
    });

    this.configureServer();
  }

  get backendClient(): BackendToolClient {
    return this.runtime.backendClient;
  }

  async connectBackend(): Promise<void> {
    await this.runtime.connect();
  }

  async close(): Promise<void> {
    await this.runtime.close();
  }

  async refreshToolCache(): Promise<void> {
    await this.runtime.refreshTools();
  }

  async start(options: StartOptions = {}): Promise<void> {
    await this.connectBackend();
    await this.server.start({
      transportType: options.transportType ?? 'stdio',
      ...(options.httpStream ? options.httpStream : {}),
    });
  }

  async getToolSchema(toolName: string): Promise<unknown> {
    return this.runtime.getToolSchema(toolName);
  }

  async invokeTool(toolName: string, toolInput: Record<string, unknown> | undefined): Promise<string> {
    return this.runtime.invokeTool(toolName, toolInput);
  }

  async listToolNames(): Promise<string[]> {
    return this.runtime.listToolNames();
  }

  async listUncompressedTools(): Promise<unknown[]> {
    return this.runtime.listUncompressedTools();
  }

  async buildCompressedDescription(): Promise<string> {
    return this.runtime.buildCompressedDescription();
  }

  private configureServer(): void {
    this.server.addTool({
      name: this.prefixName('get_tool_schema'),
      description: 'Return the full upstream schema for one backend tool.',
      parameters: z.object({ tool_name: z.string() }),
      execute: async ({ tool_name }) => JSON.stringify(await this.runtime.getToolSchema(tool_name), null, 2),
    });

    this.server.addTool({
      name: this.prefixName('invoke_tool'),
      description: 'Invoke one backend tool by name with a JSON object input.',
      parameters: z.object({
        tool_name: z.string(),
        tool_input: z.record(z.string(), z.unknown()).optional(),
      }),
      execute: async ({ tool_name, tool_input }) => this.runtime.invokeTool(tool_name, tool_input),
    });

    if (this.compressionLevel === 'max') {
      this.server.addTool({
        name: this.prefixName('list_tools'),
        description: 'List backend tool names.',
        execute: async () => JSON.stringify(await this.runtime.listToolNames(), null, 2),
      });
    }

    this.server.addResource({
      uri: this.serverName
        ? UNCOMPRESSED_RESOURCE_URI.replace('compressor://', `compressor://${this.serverName}/`)
        : UNCOMPRESSED_RESOURCE_URI,
      name: 'uncompressed-tools',
      mimeType: 'application/json',
      load: async () => ({
        text: JSON.stringify(await this.runtime.listUncompressedTools(), null, 2),
      }),
    });
  }

  private prefixName(name: string): string {
    return this.serverName ? `${this.serverName}_${name}` : name;
  }
}
