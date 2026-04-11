import { test, expect } from "vitest";

import { PersistentOAuthProvider } from "../src/oauth.js";

test("PersistentOAuthProvider prepares a localhost loopback redirect and consumes pending authorization codes", async () => {
  const provider = new PersistentOAuthProvider({
    serverUrl: "https://example.com/mcp",
    onRedirect: async () => {
      (provider as unknown as { pendingAuthorizationCode?: string }).pendingAuthorizationCode =
        "code-123";
    },
  });

  await provider.prepareInteractiveRedirect();
  expect(String(provider.redirectUrl)).toMatch(/^http:\/\/localhost:\d+\/callback$/);

  await provider.redirectToAuthorization(new URL("https://example.com/authorize?client_id=test"));
  expect(await provider.consumePendingAuthorizationCode()).toBe("code-123");
  await expect(() => provider.consumePendingAuthorizationCode()).rejects.toThrow(
    /No pending OAuth authorization code/,
  );
});
