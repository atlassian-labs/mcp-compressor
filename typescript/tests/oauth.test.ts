import { test, expect } from "vitest";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { clearAllOAuth, clearOAuth } from "../src/index.js";
import { PersistentOAuthProvider } from "../src/oauth.js";

async function tempConfigDir(): Promise<string> {
  return fs.mkdtemp(path.join(os.tmpdir(), "mcp-compressor-oauth-"));
}

test("PersistentOAuthProvider persists and invalidates state selectively", async () => {
  const configDir = await tempConfigDir();
  const provider = new PersistentOAuthProvider({
    serverUrl: "https://example.com/mcp",
    configDir,
  });

  await provider.saveTokens({ access_token: "abc", token_type: "Bearer" });
  await provider.saveCodeVerifier("verifier");
  await provider.saveDiscoveryState({
    authorizationServerUrl: "https://issuer.example.com",
    authorizationServerMetadata: {
      issuer: "https://issuer.example.com",
      authorization_endpoint: "https://issuer.example.com/auth",
      token_endpoint: "https://issuer.example.com/token",
      response_types_supported: ["code"],
    },
    resourceMetadata: {
      resource: "https://example.com/mcp",
      authorization_servers: ["https://issuer.example.com"],
    },
    resourceMetadataUrl: "https://example.com/.well-known/oauth-protected-resource",
  });

  expect((await provider.tokens())?.access_token).toBe("abc");
  expect(await provider.codeVerifier()).toBe("verifier");
  expect((await provider.discoveryState())?.authorizationServerMetadata?.token_endpoint).toBe(
    "https://issuer.example.com/token",
  );

  await provider.invalidateCredentials("tokens");
  expect(await provider.tokens()).toBe(undefined);
  expect(await provider.codeVerifier()).toBe("verifier");

  await provider.invalidateCredentials("discovery");
  expect(await provider.discoveryState()).toBe(undefined);
});

test("prepareInteractiveRedirect persists and reuses a stable redirect URL", async () => {
  const configDir = await tempConfigDir();
  const provider1 = new PersistentOAuthProvider({
    serverUrl: "https://example.com/mcp",
    configDir,
  });

  await provider1.prepareInteractiveRedirect();
  const firstRedirectUrl = String(provider1.redirectUrl);
  expect(firstRedirectUrl).toMatch(/^http:\/\/localhost:\d+\/callback$/);

  const provider2 = new PersistentOAuthProvider({
    serverUrl: "https://example.com/mcp",
    configDir,
  });
  await provider2.prepareInteractiveRedirect();
  expect(String(provider2.redirectUrl)).toBe(firstRedirectUrl);
});

test("clearOAuth clears persisted OAuth state for remote backends", async () => {
  const configDir = await tempConfigDir();
  const provider = new PersistentOAuthProvider({
    serverUrl: "https://example.com/mcp",
    configDir,
  });
  await provider.saveTokens({ access_token: "abc", token_type: "Bearer" });

  const cleared = await clearOAuth(
    { type: "http", url: "https://example.com/mcp" },
    { oauthConfigDir: configDir },
  );
  expect(cleared).toBe(true);

  const reloaded = new PersistentOAuthProvider({
    serverUrl: "https://example.com/mcp",
    configDir,
  });
  expect(await reloaded.tokens()).toBe(undefined);
});

test("clearOAuth is a no-op for stdio backends", async () => {
  const cleared = await clearOAuth({ type: "stdio", command: "uvx" });
  expect(cleared).toBe(false);
});

test("clearAllOAuth clears all persisted OAuth state without a backend", async () => {
  const configDir = await tempConfigDir();
  const provider1 = new PersistentOAuthProvider({
    serverUrl: "https://example.com/mcp",
    configDir,
  });
  const provider2 = new PersistentOAuthProvider({
    serverUrl: "https://example.org/mcp",
    configDir,
  });
  await provider1.saveTokens({ access_token: "abc", token_type: "Bearer" });
  await provider2.saveTokens({ access_token: "def", token_type: "Bearer" });

  const removed = await clearAllOAuth({ oauthConfigDir: configDir });
  expect(removed.length).toBe(2);
  expect(await provider1.tokens()).toBe(undefined);
  expect(await provider2.tokens()).toBe(undefined);
});

test("clearAllOAuth with --all semantics also removes the encryption key", async () => {
  const configDir = await tempConfigDir();
  const provider = new PersistentOAuthProvider({
    serverUrl: "https://example.com/mcp",
    configDir,
  });
  await provider.saveTokens({ access_token: "abc", token_type: "Bearer" });
  await provider.prepareInteractiveRedirect();

  const removed = await clearAllOAuth({ oauthConfigDir: configDir, all: true });
  expect(removed.join("\n")).toMatch(/\.key/);
});
