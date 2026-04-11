import { spawn } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";

import type {
  OAuthClientProvider,
  OAuthDiscoveryState,
} from "@modelcontextprotocol/sdk/client/auth.js";
import type {
  OAuthClientInformationMixed,
  OAuthClientMetadata,
  OAuthTokens,
} from "@modelcontextprotocol/sdk/shared/auth.js";

interface OAuthState {
  clientInformation?: OAuthClientInformationMixed;
  codeVerifier?: string;
  discoveryState?: OAuthDiscoveryState;
  redirectUrl?: string;
  tokens?: OAuthTokens;
}

export interface PersistentOAuthProviderOptions {
  serverUrl: string;
  redirectUrl?: string;
  onRedirect?: (url: URL) => void | Promise<void>;
  configDir?: string;
}

export async function clearAllOAuthState(
  configDir = path.join(os.homedir(), ".config", "mcp-compressor"),
  all = false,
): Promise<string[]> {
  const removed: string[] = [];
  try {
    const entries = await fs.readdir(configDir, { withFileTypes: true });
    for (const entry of entries) {
      if (entry.isFile() && entry.name.endsWith(".json")) {
        const fullPath = path.join(configDir, entry.name);
        await fs.rm(fullPath, { force: true });
        removed.push(fullPath);
      }
    }
  } catch {
    // ignore missing config dir
  }

  if (all) {
    const keyPath = path.join(configDir, ".key");
    try {
      await fs.rm(keyPath, { force: true });
      removed.push(keyPath);
    } catch {
      // ignore missing key file
    }
  }

  return removed;
}

const DEFAULT_LOOPBACK_REDIRECT_HOST = "localhost";
const DEFAULT_LOOPBACK_REDIRECT_PATH = "/callback";

export class PersistentOAuthProvider implements OAuthClientProvider {
  readonly clientMetadataUrl = undefined;
  private readonly configDir: string;
  private readonly onRedirect?: (url: URL) => void | Promise<void>;
  private pendingAuthorizationCode?: string;
  private readonly serverUrl: string;
  redirectUrl: string | URL | undefined;

  readonly clientMetadata: OAuthClientMetadata = {
    client_name: "mcp-compressor",
    grant_types: ["authorization_code", "refresh_token"],
    redirect_uris: ["http://127.0.0.1/callback"],
    response_types: ["code"],
    token_endpoint_auth_method: "none",
  };

  constructor(options: PersistentOAuthProviderOptions) {
    this.serverUrl = options.serverUrl;
    this.redirectUrl = options.redirectUrl;
    this.onRedirect = options.onRedirect;
    this.configDir = options.configDir ?? path.join(os.homedir(), ".config", "mcp-compressor");
    if (typeof this.redirectUrl === "string") {
      this.clientMetadata.redirect_uris = [this.redirectUrl];
    }
  }

  async prepareInteractiveRedirect(): Promise<void> {
    if (this.redirectUrl) {
      this.clientMetadata.redirect_uris = [String(this.redirectUrl)];
      return;
    }

    const state = await this.readState();
    const savedRedirectUrl = state.redirectUrl;
    const hasLegacyClientStateWithoutRedirect =
      !savedRedirectUrl && !!(state.clientInformation || state.tokens);

    let redirectUrl = savedRedirectUrl;
    if (!redirectUrl || !(await canListenOnRedirectUrl(new URL(redirectUrl)))) {
      const port = await findAvailablePort();
      redirectUrl = `http://${DEFAULT_LOOPBACK_REDIRECT_HOST}:${port}${DEFAULT_LOOPBACK_REDIRECT_PATH}`;
    }

    const redirectChanged = redirectUrl !== savedRedirectUrl;
    if (hasLegacyClientStateWithoutRedirect || redirectChanged) {
      delete state.clientInformation;
      delete state.tokens;
      delete state.codeVerifier;
    }

    state.redirectUrl = redirectUrl;
    await this.writeState(state);

    this.redirectUrl = redirectUrl;
    this.clientMetadata.redirect_uris = [redirectUrl];
  }

  async clientInformation(): Promise<OAuthClientInformationMixed | undefined> {
    return (await this.readState()).clientInformation;
  }

  async saveClientInformation(clientInformation: OAuthClientInformationMixed): Promise<void> {
    const state = await this.readState();
    state.clientInformation = clientInformation;
    await this.writeState(state);
  }

  async tokens(): Promise<OAuthTokens | undefined> {
    return (await this.readState()).tokens;
  }

  async saveTokens(tokens: OAuthTokens): Promise<void> {
    const state = await this.readState();
    state.tokens = tokens;
    await this.writeState(state);
  }

  async redirectToAuthorization(authorizationUrl: URL): Promise<void> {
    if (this.onRedirect) {
      await this.onRedirect(authorizationUrl);
      return;
    }

    const callbackUrl = new URL(String(this.redirectUrl));
    this.pendingAuthorizationCode = await this.captureAuthorizationCode(
      callbackUrl,
      authorizationUrl,
    );
  }

  async consumePendingAuthorizationCode(): Promise<string> {
    if (!this.pendingAuthorizationCode) {
      throw new Error("No pending OAuth authorization code is available.");
    }
    const code = this.pendingAuthorizationCode;
    this.pendingAuthorizationCode = undefined;
    return code;
  }

  async saveCodeVerifier(codeVerifier: string): Promise<void> {
    const state = await this.readState();
    state.codeVerifier = codeVerifier;
    await this.writeState(state);
  }

  async codeVerifier(): Promise<string> {
    const verifier = (await this.readState()).codeVerifier;
    if (!verifier) {
      throw new Error("Missing saved PKCE code verifier.");
    }
    return verifier;
  }

  async discoveryState(): Promise<OAuthDiscoveryState | undefined> {
    return (await this.readState()).discoveryState;
  }

  async saveDiscoveryState(discoveryState: OAuthDiscoveryState): Promise<void> {
    const state = await this.readState();
    state.discoveryState = discoveryState;
    await this.writeState(state);
  }

  async invalidateCredentials(
    scope: "all" | "client" | "tokens" | "verifier" | "discovery",
  ): Promise<void> {
    const state = await this.readState();
    if (scope === "all") {
      await this.clear();
      return;
    }
    if (scope === "client") {
      delete state.clientInformation;
    } else if (scope === "tokens") {
      delete state.tokens;
    } else if (scope === "verifier") {
      delete state.codeVerifier;
    } else if (scope === "discovery") {
      delete state.discoveryState;
    }
    await this.writeState(state);
  }

  async clear(): Promise<void> {
    await fs.rm(await this.statePath(), { force: true });
  }

  private async statePath(): Promise<string> {
    const secret = await this.encryptionKey();
    const key = crypto.createHmac("sha256", secret).update(this.serverUrl).digest("hex");
    return path.join(this.configDir, `${key}.json`);
  }

  private keyPath(): string {
    return path.join(this.configDir, ".key");
  }

  private async readState(): Promise<OAuthState> {
    try {
      const encrypted = await fs.readFile(await this.statePath(), "utf8");
      return JSON.parse(await this.decrypt(encrypted)) as OAuthState;
    } catch {
      return {};
    }
  }

  private async writeState(state: OAuthState): Promise<void> {
    await fs.mkdir(this.configDir, { recursive: true });
    const statePath = await this.statePath();
    await fs.writeFile(statePath, await this.encrypt(JSON.stringify(state)), "utf8");
    await fs.chmod(statePath, 0o600).catch(() => undefined);
  }

  private async encryptionKey(): Promise<Buffer> {
    await fs.mkdir(this.configDir, { recursive: true });
    try {
      return await fs.readFile(this.keyPath());
    } catch {
      const key = crypto.randomBytes(32);
      await fs.writeFile(this.keyPath(), key);
      await fs.chmod(this.keyPath(), 0o600).catch(() => undefined);
      return key;
    }
  }

  private async encrypt(plaintext: string): Promise<string> {
    const iv = crypto.randomBytes(12);
    const cipher = crypto.createCipheriv("aes-256-gcm", await this.encryptionKey(), iv);
    const ciphertext = Buffer.concat([cipher.update(plaintext, "utf8"), cipher.final()]);
    const tag = cipher.getAuthTag();
    return Buffer.concat([iv, tag, ciphertext]).toString("base64");
  }

  private async decrypt(payload: string): Promise<string> {
    const decoded = Buffer.from(payload, "base64");
    const iv = decoded.subarray(0, 12);
    const tag = decoded.subarray(12, 28);
    const ciphertext = decoded.subarray(28);
    const decipher = crypto.createDecipheriv("aes-256-gcm", await this.encryptionKey(), iv);
    decipher.setAuthTag(tag);
    return Buffer.concat([decipher.update(ciphertext), decipher.final()]).toString("utf8");
  }

  private async captureAuthorizationCode(callbackUrl: URL, authorizationUrl: URL): Promise<string> {
    const server = http.createServer();

    const codePromise = new Promise<string>((resolve, reject) => {
      server.on("request", (request, response) => {
        const requestUrl = new URL(request.url ?? "/", callbackUrl);
        if (requestUrl.pathname !== callbackUrl.pathname) {
          response.writeHead(404).end("Not found");
          return;
        }

        const error = requestUrl.searchParams.get("error");
        const code = requestUrl.searchParams.get("code");
        if (error) {
          response.writeHead(400, { "content-type": "text/plain; charset=utf-8" });
          response.end(`OAuth authorization failed: ${error}`);
          reject(new Error(`OAuth authorization failed: ${error}`));
          return;
        }
        if (!code) {
          response.writeHead(400, { "content-type": "text/plain; charset=utf-8" });
          response.end("Missing OAuth authorization code.");
          reject(new Error("Missing OAuth authorization code."));
          return;
        }

        response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
        response.end(
          "<html><body><h1>Authorization complete</h1><p>You can return to the terminal.</p></body></html>",
        );
        resolve(code);
      });
      server.on("error", reject);
    });

    await new Promise<void>((resolve, reject) => {
      server.listen(Number(callbackUrl.port), callbackUrl.hostname, () => resolve());
      server.on("error", reject);
    });

    try {
      this.openBrowser(authorizationUrl);
      console.error(`Open this URL to authorize mcp-compressor:\n${authorizationUrl.toString()}`);
      return await codePromise;
    } finally {
      await new Promise((resolve) => server.close(() => resolve(undefined)));
    }
  }

  private openBrowser(url: URL): void {
    const href = url.toString();
    const platform = process.platform;
    if (platform === "darwin") {
      void spawn("open", [href], { detached: true, stdio: "ignore" }).unref();
      return;
    }
    if (platform === "win32") {
      void spawn("cmd", ["/c", "start", "", href], { detached: true, stdio: "ignore" }).unref();
      return;
    }
    void spawn("xdg-open", [href], { detached: true, stdio: "ignore" }).unref();
  }
}

async function canListenOnRedirectUrl(redirectUrl: URL): Promise<boolean> {
  const port = Number.parseInt(redirectUrl.port, 10);
  if (!Number.isInteger(port) || port <= 0) {
    return false;
  }
  return await new Promise<boolean>((resolve) => {
    const server = net.createServer();
    server.once("error", () => resolve(false));
    server.listen(port, redirectUrl.hostname, () => {
      server.close(() => resolve(true));
    });
  });
}

async function findAvailablePort(): Promise<number> {
  return await new Promise<number>((resolve, reject) => {
    const server = net.createServer();
    server.on("error", reject);
    server.listen(0, DEFAULT_LOOPBACK_REDIRECT_HOST, () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close(() => reject(new Error("Failed to determine an available loopback port.")));
        return;
      }
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve(address.port);
      });
    });
  });
}
