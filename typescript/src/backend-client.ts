import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { auth, UnauthorizedError, type OAuthClientProvider } from '@modelcontextprotocol/sdk/client/auth.js';
import { SSEClientTransport } from '@modelcontextprotocol/sdk/client/sse.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import type { Tool } from '@modelcontextprotocol/sdk/types.js';

import { InvalidConfigurationError } from './errors.js';
import { PersistentOAuthProvider } from './oauth.js';
import type { BackendConfig } from './types.js';

function shouldRetryStaleOAuthConnectError(error: unknown, config: BackendConfig): boolean {
  if (config.type !== 'http' && config.type !== 'sse') {
    return false;
  }
  const message = error instanceof Error ? error.message : String(error);
  return (
    message.includes('Unauthorized') ||
    message.includes('invalid_grant') ||
    message.includes('invalid_client') ||
    message.includes('401')
  );
}

export class BackendClient {
  private client: Client | null = null;
  private readonly config: BackendConfig;
  private readonly oauthProvider?: OAuthClientProvider;
  private connected = false;

  constructor(config: BackendConfig, oauthProvider?: OAuthClientProvider) {
    this.config = config;
    this.oauthProvider = oauthProvider;
  }

  async connect(): Promise<void> {
    if (this.connected) {
      return;
    }

    try {
      await this.connectOnce();
    } catch (error) {
      if (!this.oauthProvider || !shouldRetryStaleOAuthConnectError(error, this.config)) {
        throw error;
      }

      await this.oauthProvider.invalidateCredentials?.('all');
      this.connected = false;
      this.client = null;

      try {
        await this.connectOnce();
      } catch (retryError) {
        throw new Error(
          `${retryError instanceof Error ? retryError.message : String(retryError)}\n\n` +
            "Cached OAuth credentials may be stale. mcp-compressor cleared cached OAuth state and retried once. If the problem persists, run 'mcp-compressor clear-oauth <backend-url>' and try again.",
        );
      }
    }
  }

  private async connectOnce(): Promise<void> {
    const client = new Client({ name: 'mcp-compressor', version: '0.2.12' });

    if (this.config.type === 'stdio') {
      await client.connect(
        new StdioClientTransport({
          command: this.config.command,
          args: this.config.args,
          cwd: this.config.cwd,
          env: this.config.env,
        }),
      );
    } else if (this.config.type === 'http') {
      await this.ensureAuthorized(this.config.url, this.config.headers);
      await client.connect(
        new StreamableHTTPClientTransport(new URL(this.config.url), {
          authProvider: this.oauthProvider,
          requestInit: { headers: this.config.headers },
        }),
      );
    } else if (this.config.type === 'sse') {
      await this.ensureAuthorized(this.config.url, this.config.headers);
      await client.connect(
        new SSEClientTransport(new URL(this.config.url), {
          authProvider: this.oauthProvider,
          requestInit: { headers: this.config.headers },
        }),
      );
    } else {
      throw new InvalidConfigurationError(`Unsupported backend type: ${String((this.config as { type?: unknown }).type)}`);
    }

    this.client = client;
    this.connected = true;
  }

  private async ensureAuthorized(url: string, headers?: Record<string, string>): Promise<void> {
    if (!(this.oauthProvider instanceof PersistentOAuthProvider)) {
      return;
    }

    await this.oauthProvider.prepareInteractiveRedirect();

    const fetchFn: typeof fetch = (input, init) => {
      const mergedHeaders = new Headers(init?.headers);
      for (const [key, value] of Object.entries(headers ?? {})) {
        mergedHeaders.set(key, value);
      }

      return fetch(input, {
        ...init,
        headers: mergedHeaders,
      });
    };

    const result = await auth(this.oauthProvider, {
      serverUrl: new URL(url),
      fetchFn,
    });

    if (result === 'REDIRECT') {
      const authorizationCode = await this.oauthProvider.consumePendingAuthorizationCode();
      const finishResult = await auth(this.oauthProvider, {
        serverUrl: new URL(url),
        authorizationCode,
        fetchFn,
      });
      if (finishResult !== 'AUTHORIZED') {
        throw new UnauthorizedError('Failed to complete OAuth authorization.');
      }
    }
  }

  async close(): Promise<void> {
    await this.client?.close();
    this.connected = false;
    this.client = null;
  }

  async listTools(): Promise<Tool[]> {
    const result = await this.requireClient().listTools();
    return result.tools;
  }

  async callTool(name: string, args: Record<string, unknown> | undefined): Promise<unknown> {
    return this.requireClient().callTool({ name, arguments: args });
  }

  async readResource(uri: string): Promise<unknown> {
    return this.requireClient().readResource({ uri });
  }

  private requireClient(): Client {
    if (!this.client) {
      throw new Error('Backend client is not connected. Call connect() first.');
    }
    return this.client;
  }
}
