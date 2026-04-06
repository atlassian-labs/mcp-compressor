import { BackendClient } from './backend-client.js';
import { parseSingleServerConfigJson } from './config.js';
import { InvalidConfigurationError } from './errors.js';
import { PersistentOAuthProvider } from './oauth.js';
import { CompressorRuntime } from './runtime.js';
import { CompressorServer } from './server.js';
import type { BackendConfig, CommonProxyOptions, StartOptions } from './types.js';

export * from './backend-client.js';
export * from './config.js';
export * from './errors.js';
export * from './oauth.js';
export * from './runtime.js';
export * from './server.js';
export * from './cli_mode.js';
export * from './types.js';

export interface CreateCompressorServerOptions extends CommonProxyOptions {
  backend: BackendConfig | string;
  oauthConfigDir?: string;
  oauthRedirectUrl?: string;
  onOAuthRedirect?: (url: URL) => void | Promise<void>;
}

export function resolveBackend(
  backend: BackendConfig | string,
  serverName?: string,
): { backend: BackendConfig; serverName?: string } {
  if (typeof backend !== 'string') {
    return { backend, serverName };
  }

  const parsed = parseSingleServerConfigJson(backend);
  if (parsed) {
    return { backend: parsed.backend, serverName: serverName ?? parsed.serverName };
  }

  if (backend.startsWith('http://') || backend.startsWith('https://')) {
    return { backend: { type: 'http', url: backend }, serverName };
  }

  throw new InvalidConfigurationError('String backend values must be a remote URL or a single-server MCP config JSON string.');
}

export function createOAuthProviderForBackend(
  backend: BackendConfig,
  options: Pick<CreateCompressorServerOptions, 'oauthConfigDir' | 'oauthRedirectUrl' | 'onOAuthRedirect'> = {},
): PersistentOAuthProvider | undefined {
  return backend.type === 'http' || backend.type === 'sse'
    ? new PersistentOAuthProvider({
        serverUrl: backend.url,
        configDir: options.oauthConfigDir,
        redirectUrl: options.oauthRedirectUrl,
        onRedirect: options.onOAuthRedirect,
      })
    : undefined;
}

export async function clearOAuth(
  backend: BackendConfig | string,
  options: Pick<CreateCompressorServerOptions, 'oauthConfigDir'> = {},
): Promise<boolean> {
  const resolved = resolveBackend(backend);
  const provider = createOAuthProviderForBackend(resolved.backend, options);
  if (!provider) {
    return false;
  }
  await provider.clear();
  return true;
}

export function createCompressorRuntime(options: CreateCompressorServerOptions): CompressorRuntime {
  const resolved = resolveBackend(options.backend, options.serverName);
  const oauthProvider = createOAuthProviderForBackend(resolved.backend, options);

  const backendClient = new BackendClient(resolved.backend, oauthProvider);
  return new CompressorRuntime({
    backendClient,
    compressionLevel: options.compressionLevel,
    excludeTools: options.excludeTools,
    includeTools: options.includeTools,
    serverName: resolved.serverName,
    toonify: options.toonify,
  });
}

export async function initializeCompressorRuntime(
  options: CreateCompressorServerOptions,
): Promise<CompressorRuntime> {
  const runtime = createCompressorRuntime(options);
  await runtime.connect();
  return runtime;
}

export async function initializeCompressedFunctionToolset(
  options: CreateCompressorServerOptions,
): Promise<{
  runtime: CompressorRuntime;
  toolset: ReturnType<CompressorRuntime['getFunctionToolset']>;
}> {
  const runtime = await initializeCompressorRuntime(options);
  return {
    runtime,
    toolset: runtime.getFunctionToolset(),
  };
}

export function createCompressorServer(options: CreateCompressorServerOptions): CompressorServer {
  const resolved = resolveBackend(options.backend, options.serverName);
  const oauthProvider = createOAuthProviderForBackend(resolved.backend, options);

  const backendClient = new BackendClient(resolved.backend, oauthProvider);
  return new CompressorServer({
    backendClient,
    compressionLevel: options.compressionLevel,
    excludeTools: options.excludeTools,
    includeTools: options.includeTools,
    serverName: resolved.serverName,
    toonify: options.toonify,
  });
}

export async function startCompressorServer(
  options: CreateCompressorServerOptions & { start?: StartOptions },
): Promise<CompressorServer> {
  const server = createCompressorServer(options);
  await server.start(options.start);
  return server;
}
