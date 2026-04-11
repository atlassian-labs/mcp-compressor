import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import type { CompressorRuntime } from "./runtime.js";
import { formatTopLevelHelp, sanitizeCliName } from "./cli_tools.js";
import { CliBridge } from "./cli_bridge.js";
import { generateCliScript, removeCliScriptEntry } from "./cli_script.js";
import type { CreateCompressorServerOptions } from "./index.js";
import type { BackendConfig } from "./types.js";

export interface CliModeOptions extends CreateCompressorServerOptions {
  cliName?: string;
  cliPort?: number;
  scriptDir?: string;
}

export interface CliModeScript {
  bridge: CliBridge;
  bridgeUrl: string;
  cliName: string;
  helpText: string;
  onPath: boolean;
  runtime: CompressorRuntime;
  scriptPath: string;
  tools: Tool[];
}

export interface CliModeSession extends Omit<CliModeScript, "bridge"> {
  runtimes: CompressorRuntime[];
  scripts: CliModeScript[];
  close(): Promise<void>;
}

async function initializeSingleCliScript(
  options: CliModeOptions,
  backend: BackendConfig,
  cliName: string,
  serverName: string | undefined,
  sessionPid: number,
): Promise<CliModeScript> {
  const { initializeCompressorRuntime } = await import("./index.js");
  const runtime = await initializeCompressorRuntime({
    ...options,
    backend,
    serverName,
  });

  const bridge = new CliBridge(runtime, cliName);
  const bridgePort = await bridge.start(options.cliPort ?? 0);
  const generated = await generateCliScript(cliName, bridgePort, sessionPid, options.scriptDir);
  const tools = await runtime.listUncompressedTools();

  return {
    bridge,
    bridgeUrl: bridge.url,
    cliName,
    helpText: formatTopLevelHelp(cliName, tools),
    onPath: generated.onPath,
    runtime,
    scriptPath: generated.scriptPath,
    tools,
  };
}

export async function initializeCliMode(options: CliModeOptions): Promise<CliModeSession> {
  const { resolveAllBackends } = await import("./index.js");
  const resolvedBackends = resolveAllBackends(options.backend, options.serverName);
  const sessionPid = process.ppid;
  const scripts: CliModeScript[] = [];

  try {
    for (const resolved of resolvedBackends) {
      const cliName = sanitizeCliName(
        resolvedBackends.length === 1
          ? (options.cliName ?? resolved.serverName ?? "mcp")
          : (resolved.serverName ?? options.cliName ?? "mcp"),
      );
      scripts.push(
        await initializeSingleCliScript(
          options,
          resolved.backend,
          cliName,
          resolved.serverName,
          sessionPid,
        ),
      );
    }

    const primary = scripts[0]!;
    const runtimes = scripts.map((script) => script.runtime);
    return {
      bridgeUrl: primary.bridgeUrl,
      cliName: primary.cliName,
      helpText: primary.helpText,
      onPath: primary.onPath,
      runtime: primary.runtime,
      runtimes,
      scripts,
      scriptPath: primary.scriptPath,
      tools: primary.tools,
      async close(): Promise<void> {
        await Promise.allSettled(scripts.map((script) => script.bridge.close()));
        await Promise.allSettled(runtimes.map((runtime) => runtime.close()));
        await Promise.allSettled(
          scripts.map((script) =>
            removeCliScriptEntry(script.cliName, sessionPid, options.scriptDir),
          ),
        );
      },
    };
  } catch (error) {
    await Promise.allSettled(scripts.map((script) => script.bridge.close()));
    await Promise.allSettled(scripts.map((script) => script.runtime.close()));
    await Promise.allSettled(
      scripts.map((script) => removeCliScriptEntry(script.cliName, sessionPid, options.scriptDir)),
    );
    throw error;
  }
}
