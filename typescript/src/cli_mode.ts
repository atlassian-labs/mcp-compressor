import type { Tool } from '@modelcontextprotocol/sdk/types.js';

import type { CompressorRuntime } from './runtime.js';
import { formatTopLevelHelp, sanitizeCliName } from './cli_tools.js';
import { CliBridge } from './cli_bridge.js';
import { generateCliScript, removeCliScriptEntry } from './cli_script.js';
import type { CreateCompressorServerOptions } from './index.js';

export interface CliModeOptions extends CreateCompressorServerOptions {
  cliName?: string;
  cliPort?: number;
  scriptDir?: string;
}

export interface CliModeSession {
  cliName: string;
  bridgeUrl: string;
  helpText: string;
  onPath: boolean;
  runtime: CompressorRuntime;
  scriptPath: string;
  tools: Tool[];
  close(): Promise<void>;
}

export async function initializeCliMode(options: CliModeOptions): Promise<CliModeSession> {
  const { initializeCompressorRuntime, resolveBackend } = await import('./index.js');
  const resolved = resolveBackend(options.backend, options.serverName);
  const cliName = sanitizeCliName(options.cliName ?? resolved.serverName ?? 'mcp');
  const runtime = await initializeCompressorRuntime({
    ...options,
    backend: resolved.backend,
    serverName: resolved.serverName,
  });

  const bridge = new CliBridge(runtime, cliName);
  const bridgePort = await bridge.start(options.cliPort ?? 0);
  const sessionPid = process.ppid;

  const { scriptPath, onPath } = await generateCliScript(cliName, bridgePort, sessionPid, options.scriptDir);
  const tools = await runtime.listUncompressedTools();
  const helpText = formatTopLevelHelp(cliName, tools);

  return {
    cliName,
    bridgeUrl: bridge.url,
    helpText,
    onPath,
    runtime,
    scriptPath,
    tools,
    async close(): Promise<void> {
      await Promise.allSettled([bridge.close(), runtime.close()]);
      await removeCliScriptEntry(cliName, sessionPid, options.scriptDir);
    },
  };
}
