import type { CompressorRuntime } from "./runtime.js";
import { sanitizeCliName } from "./cli_tools.js";
import type { CreateCompressorServerOptions } from "./index.js";

export interface CliModeOptions extends CreateCompressorServerOptions {
  cliName?: string;
  cliPort?: number;
  scriptDir?: string;
}

export interface CliModeScript {
  bridgeUrl: string | null;
  cliName: string;
  onPath: boolean;
  runtime: CompressorRuntime;
  scriptPath: string | null;
}

export interface CliModeSession {
  cliName: string;
  runtimes: CompressorRuntime[];
  scripts: CliModeScript[];
  close(): Promise<void>;
}

/**
 * Initialize one or more CompressorRuntimes in CLI mode from a backend config.
 *
 * Each runtime connects to its backend, starts a local HTTP bridge, and writes a CLI script.
 * Cleanup (bridge teardown, script removal, backend disconnect) is handled by `session.close()`
 * which delegates to each runtime's `disconnect()`.
 */
export async function initializeCliMode(options: CliModeOptions): Promise<CliModeSession> {
  const { createCompressorRuntime, resolveBackends } = await import("./index.js");
  const resolvedBackends = resolveBackends(options.backend, options.serverName);
  const runtimes: CompressorRuntime[] = [];

  try {
    for (const resolved of resolvedBackends) {
      const cliName = sanitizeCliName(
        resolvedBackends.length === 1
          ? (options.cliName ?? resolved.serverName ?? "mcp")
          : (resolved.serverName ?? options.cliName ?? "mcp"),
      );

      const runtime = createCompressorRuntime({
        ...options,
        backend: resolved.backend,
        serverName: resolved.serverName,
        cliMode: true,
        cliName,
      });
      await runtime.connect();
      runtimes.push(runtime);
    }

    const primaryCliName = sanitizeCliName(
      resolvedBackends.length === 1
        ? (options.cliName ?? resolvedBackends[0]!.serverName ?? "mcp")
        : (resolvedBackends[0]!.serverName ?? options.cliName ?? "mcp"),
    );

    return {
      cliName: primaryCliName,
      runtimes,
      scripts: runtimes.map((runtime) => ({
        bridgeUrl: runtime.cliBridgeUrl,
        cliName: sanitizeCliName(runtime.serverName ?? "mcp"),
        onPath: runtime.cliOnPath,
        runtime,
        scriptPath: runtime.cliScriptPath,
      })),
      async close(): Promise<void> {
        await Promise.allSettled(runtimes.map((runtime) => runtime.disconnect()));
      },
    };
  } catch (error) {
    await Promise.allSettled(runtimes.map((runtime) => runtime.disconnect()));
    throw error;
  }
}
