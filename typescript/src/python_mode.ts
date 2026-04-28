/**
 * Convenience wrapper for the `python` transform mode.
 *
 * Connects one or more `CompressorRuntime`s, starts a single shared {@link PythonBridge} that fronts
 * all of them, and generates the Python stub file tree for each server. Returns everything the
 * consumer needs to mount the stubs into an execution environment (e.g. a sandboxed container, a
 * remote agent runtime, or just a tmpdir on the local host).
 *
 * No file I/O is performed here — the consumer is responsible for materialising the
 * `Map<filename, contents>` returned in `session.allFiles`. This keeps the library
 * environment-agnostic.
 *
 * Cleanup (`session.close()`) tears down the bridge and disconnects all runtimes.
 */

import type { CompressorRuntime } from "./runtime.js";
import type { CreateCompressorServerOptions } from "./index.js";
import { PythonBridge } from "./python_bridge.js";
import {
  DEFAULT_PACKAGE_NAME,
  generatePythonStubs,
  sanitizePythonModuleName,
} from "./python_stubs.js";
import { getPythonRuntimeAssets } from "./python_runtime_assets.js";

export interface PythonModeOptions extends CreateCompressorServerOptions {
  /** Top-level Python package name. Defaults to `tools`. */
  packageName?: string;
  /** Port for the shared loopback bridge. 0 (default) → OS-assigned. */
  bridgePort?: number;
}

export interface PythonModeServer {
  /** Sanitized server name used as the Python sub-package name. */
  serverName: string;
  /** Underlying runtime (already connected). */
  runtime: CompressorRuntime;
  /** URL the Python interpreter should be told to use as the bridge endpoint (via env var). */
  bridgeUrl: string;
  /**
   * The Python module path the LLM should be told to import for this server's tools, e.g.
   * `tools.jira`.
   */
  entryModule: string;
  /** Per-server file map. Includes the server's own `_call.py` transport module. */
  files: ReadonlyMap<string, string>;
  /** Markdown summary of the generated package and where to read detailed docs. */
  toolInventory: string;
}

export interface PythonModeSession {
  /** Top-level Python package name (mirrors `PythonModeOptions.packageName`). */
  packageName: string;
  /** Single shared bridge that fronts every server in this session. */
  bridge: PythonBridge;
  /** Loopback URL the bridge listens on. Already baked into each generated `<svc>/_call.py`. */
  bridgeUrl: string;
  servers: PythonModeServer[];
  /** Markdown summary of all generated packages and where to read detailed docs. */
  toolInventory: string;
  /**
   * Combined file tree across all servers, including the shared package `__init__.py`. Use this
   * when mounting a single tree into an execution environment.
   */
  allFiles: ReadonlyMap<string, string>;
  close(): Promise<void>;
}

/**
 * Initialize one or more `CompressorRuntime`s in python mode. The returned session bundles the
 * generated Python file tree alongside the live HTTP bridges; callers decide how to materialize
 * the files and which environment variable to feed the Python interpreter.
 */
export async function initializePythonMode(options: PythonModeOptions): Promise<PythonModeSession> {
  const { createCompressorRuntime, resolveBackends } = await import("./index.js");
  const packageName = options.packageName ?? DEFAULT_PACKAGE_NAME;
  const resolvedBackends = resolveBackends(options.backend, options.serverName);

  const runtimes: CompressorRuntime[] = [];
  const runtimesByService = new Map<string, CompressorRuntime>();
  const partialServers: Array<Omit<PythonModeServer, "bridgeUrl">> = [];
  let bridge: PythonBridge | undefined;

  try {
    for (const resolved of resolvedBackends) {
      const runtime = createCompressorRuntime({
        ...options,
        backend: resolved.backend,
        serverName: resolved.serverName,
      });
      await runtime.connect();
      runtimes.push(runtime);

      const serverName = sanitizePythonModuleName(resolved.serverName ?? "tools");
      runtimesByService.set(serverName, runtime);

      const tools = await runtime.listUncompressedTools();
      const generated = generatePythonStubs(tools, { serverName, packageName });

      partialServers.push({
        serverName,
        runtime,
        entryModule: generated.entryModule,
        files: generated.files,
        toolInventory: generated.toolInventory,
      });
    }

    bridge = new PythonBridge(runtimesByService);
    await bridge.start(options.bridgePort);
    const bridgeUrl = bridge.url;

    const servers: PythonModeServer[] = partialServers.map((s) => ({ ...s, bridgeUrl }));
    const allFiles = mergeFileTrees(servers, packageName, bridgeUrl);
    const toolInventory = renderToolInventory(servers);

    const heldBridge = bridge;
    return {
      packageName,
      bridge: heldBridge,
      bridgeUrl,
      servers,
      toolInventory,
      allFiles,
      async close(): Promise<void> {
        await heldBridge.close();
        await Promise.allSettled(runtimes.map((r) => r.disconnect()));
      },
    };
  } catch (error) {
    if (bridge !== undefined) {
      await bridge.close();
    }
    await Promise.allSettled(runtimes.map((r) => r.disconnect()));
    throw error;
  }
}

function renderToolInventory(servers: ReadonlyArray<PythonModeServer>): string {
  return servers.map((server) => server.toolInventory).join("\n");
}

function mergeFileTrees(
  servers: ReadonlyArray<PythonModeServer>,
  packageName: string,
  bridgeUrl: string,
): ReadonlyMap<string, string> {
  const merged = new Map<string, string>();
  for (const server of servers) {
    // Per-server `_call.py` transport — bridge URL is baked in. No top-level `<packageName>/__init__.py`
    // is emitted, making the package a PEP 420 namespace package so multiple servers can be mounted
    // into separate PYTHONPATH directories without colliding.
    for (const [path, content] of getPythonRuntimeAssets({
      packageName,
      serverName: server.serverName,
      bridgeUrl,
    })) {
      merged.set(path, content);
    }
    for (const [path, content] of server.files) {
      merged.set(path, content);
    }
  }
  return merged;
}
