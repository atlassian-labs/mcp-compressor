import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { clearOAuth, createOAuthProviderForBackend } from '../src/index.js';
import { PersistentOAuthProvider } from '../src/oauth.js';

async function tempConfigDir(): Promise<string> {
  return fs.mkdtemp(path.join(os.tmpdir(), 'mcp-compressor-oauth-'));
}

test('PersistentOAuthProvider persists and invalidates state selectively', async () => {
  const configDir = await tempConfigDir();
  const provider = new PersistentOAuthProvider({
    serverUrl: 'https://example.com/mcp',
    configDir,
  });

  await provider.saveTokens({ access_token: 'abc', token_type: 'Bearer' });
  await provider.saveCodeVerifier('verifier');
  await provider.saveDiscoveryState({
    authorizationServerUrl: 'https://issuer.example.com',
    authorizationServerMetadata: {
      issuer: 'https://issuer.example.com',
      authorization_endpoint: 'https://issuer.example.com/auth',
      token_endpoint: 'https://issuer.example.com/token',
      response_types_supported: ['code'],
    },
    resourceMetadata: {
      resource: 'https://example.com/mcp',
      authorization_servers: ['https://issuer.example.com'],
    },
    resourceMetadataUrl: 'https://example.com/.well-known/oauth-protected-resource',
  });

  assert.equal((await provider.tokens())?.access_token, 'abc');
  assert.equal(await provider.codeVerifier(), 'verifier');
  assert.equal(
    (await provider.discoveryState())?.authorizationServerMetadata?.token_endpoint,
    'https://issuer.example.com/token',
  );

  await provider.invalidateCredentials('tokens');
  assert.equal(await provider.tokens(), undefined);
  assert.equal(await provider.codeVerifier(), 'verifier');

  await provider.invalidateCredentials('discovery');
  assert.equal(await provider.discoveryState(), undefined);
});

test('clearOAuth clears persisted OAuth state for remote backends', async () => {
  const configDir = await tempConfigDir();
  const provider = new PersistentOAuthProvider({
    serverUrl: 'https://example.com/mcp',
    configDir,
  });
  await provider.saveTokens({ access_token: 'abc', token_type: 'Bearer' });

  const cleared = await clearOAuth({ type: 'http', url: 'https://example.com/mcp' }, { oauthConfigDir: configDir });
  assert.equal(cleared, true);

  const reloaded = new PersistentOAuthProvider({
    serverUrl: 'https://example.com/mcp',
    configDir,
  });
  assert.equal(await reloaded.tokens(), undefined);
});

test('clearOAuth is a no-op for stdio backends', async () => {
  const cleared = await clearOAuth({ type: 'stdio', command: 'uvx' });
  assert.equal(cleared, false);
});
