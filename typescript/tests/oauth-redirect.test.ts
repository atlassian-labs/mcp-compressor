import test from 'node:test';
import assert from 'node:assert/strict';

import { PersistentOAuthProvider } from '../src/oauth.js';

test('PersistentOAuthProvider prepares a localhost loopback redirect and consumes pending authorization codes', async () => {
  const provider = new PersistentOAuthProvider({
    serverUrl: 'https://example.com/mcp',
    onRedirect: async () => {
      (provider as unknown as { pendingAuthorizationCode?: string }).pendingAuthorizationCode = 'code-123';
    },
  });

  await provider.prepareInteractiveRedirect();
  assert.match(String(provider.redirectUrl), /^http:\/\/localhost:\d+\/callback$/);

  await provider.redirectToAuthorization(new URL('https://example.com/authorize?client_id=test'));
  assert.equal(await provider.consumePendingAuthorizationCode(), 'code-123');
  await assert.rejects(() => provider.consumePendingAuthorizationCode(), /No pending OAuth authorization code/);
});
