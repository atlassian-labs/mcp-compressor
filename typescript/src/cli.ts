#!/usr/bin/env node
import { clearOAuth, initializeCliMode, startCompressorServer } from './index.js';
import type { BackendConfig } from './types.js';

function consumeFlag(args: string[], names: string[]): string | undefined {
  for (let i = 0; i < args.length; i += 1) {
    if (names.includes(args[i]!)) {
      const value = args[i + 1];
      args.splice(i, 2);
      return value;
    }
  }
  return undefined;
}

function consumeBooleanFlag(args: string[], names: string[]): boolean {
  const index = args.findIndex((arg) => names.includes(arg));
  if (index >= 0) {
    args.splice(index, 1);
    return true;
  }
  return false;
}

function consumeMultiFlag(args: string[], names: string[]): string[] {
  const values: string[] = [];
  for (let i = 0; i < args.length; ) {
    if (names.includes(args[i]!)) {
      values.push(args[i + 1]!);
      args.splice(i, 2);
    } else {
      i += 1;
    }
  }
  return values;
}

function extractBackendArgs(args: string[]): string[] {
  const separatorIndex = args.indexOf('--');
  if (separatorIndex >= 0) {
    const backendArgs = args.slice(separatorIndex + 1);
    args.splice(separatorIndex);
    return backendArgs;
  }
  return [...args];
}

function parseBackendArg(backendArgs: string[]): BackendConfig | string {
  if (backendArgs.length === 0) {
    throw new Error('Expected a backend URL, MCP config JSON string, or stdio command.');
  }
  if (backendArgs.length === 1) {
    return backendArgs[0]!;
  }
  return {
    type: 'stdio',
    command: backendArgs[0]!,
    args: backendArgs.slice(1),
  };
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);

  if (args[0] === 'clear-oauth') {
    const backend = args[1];
    if (!backend) {
      throw new Error('Usage: mcp-compressor clear-oauth <backend-url-or-single-server-json>');
    }
    const cleared = await clearOAuth(backend);
    if (!cleared) {
      console.warn('No OAuth state applies to that backend.');
    }
    return;
  }

  const backendArgs = extractBackendArgs(args);
  const logLevel = consumeFlag(args, ['--log-level', '-l']) ?? 'error';
  const compressionLevel = consumeFlag(args, ['--compression-level', '-c']) as
    | 'low'
    | 'medium'
    | 'high'
    | 'max'
    | undefined;
  const serverName = consumeFlag(args, ['--server-name']);
  const cliPort = consumeFlag(args, ['--cli-port']);
  const includeTools = consumeMultiFlag(args, ['--include-tool']);
  const excludeTools = consumeMultiFlag(args, ['--exclude-tool']);
  const toonifyRequested = consumeBooleanFlag(args, ['--toonify']);
  const cliMode = consumeBooleanFlag(args, ['--cli-mode']);
  const toonify = toonifyRequested || cliMode;

  if (args.length > 0 && backendArgs.length === 0) {
    backendArgs.push(...args);
  }

  const backend = parseBackendArg(backendArgs);

  if (logLevel !== 'error') {
    console.warn(`[mcp-compressor-ts] log-level ${logLevel} requested; detailed logging is not implemented yet.`);
  }

  if (cliMode) {
    const session = await initializeCliMode({
      backend,
      cliPort: cliPort ? Number.parseInt(cliPort, 10) : undefined,
      compressionLevel,
      excludeTools: excludeTools.length > 0 ? excludeTools : undefined,
      includeTools: includeTools.length > 0 ? includeTools : undefined,
      serverName,
      toonify,
    });

    const invoke = session.onPath ? session.cliName : `./${session.cliName}`;
    console.error(`CLI mode active.`);
    console.error(`Generated CLI: ${session.scriptPath}`);
    console.error(`Run '${invoke} --help' for usage.`);

    const shutdown = async () => {
      await session.close();
      process.exit(0);
    };
    process.once('SIGINT', () => void shutdown());
    process.once('SIGTERM', () => void shutdown());

    await new Promise(() => {
      // keep the bridge/runtime process alive
    });
    return;
  }

  await startCompressorServer({
    backend,
    compressionLevel,
    excludeTools: excludeTools.length > 0 ? excludeTools : undefined,
    includeTools: includeTools.length > 0 ? includeTools : undefined,
    serverName,
    start: { transportType: 'stdio' },
    toonify,
  });
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.stack ?? error.message : String(error));
  process.exitCode = 1;
});
