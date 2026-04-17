import { expect, test } from "vitest";
import { interpolateMCPConfig, interpolateString, parseServerConfigJson } from "../src/config.js";

test("parseServerConfigJson parses a single stdio server", () => {
  const parsed = parseServerConfigJson(
    '{"mcpServers":{"alpha":{"command":"uvx","args":["alpha"]}}}',
  );
  expect(parsed).toEqual([
    {
      backend: { type: "stdio", command: "uvx", args: ["alpha"] },
      serverName: "alpha",
    },
  ]);
});

test("parseServerConfigJson parses multiple servers", () => {
  const parsed = parseServerConfigJson(
    '{"mcpServers":{"alpha":{"command":"uvx","args":["alpha"]},"beta":{"url":"http://localhost"}}}',
  );
  expect(parsed).toEqual([
    {
      backend: { type: "stdio", command: "uvx", args: ["alpha"] },
      serverName: "alpha",
    },
    {
      backend: { type: "http", url: "http://localhost" },
      serverName: "beta",
    },
  ]);
});

test("parseServerConfigJson returns null for non-JSON input", () => {
  expect(parseServerConfigJson("uvx mcp-server-fetch")).toBe(null);
  expect(parseServerConfigJson("https://example.com/mcp")).toBe(null);
});

test("parseServerConfigJson throws for empty mcpServers", () => {
  expect(() => parseServerConfigJson('{"mcpServers":{}}')).toThrow(/at least one server/i);
});

test("interpolateString substitutes set environment variables", () => {
  process.env["TEST_TOKEN"] = "secret123";
  expect(interpolateString("Bearer ${TEST_TOKEN}")).toBe("Bearer secret123");
  delete process.env["TEST_TOKEN"];
});

test("interpolateString leaves unset variables as-is", () => {
  delete process.env["MISSING_VAR"];
  expect(interpolateString("${MISSING_VAR}")).toBe("${MISSING_VAR}");
  expect(interpolateString("$MISSING_VAR")).toBe("$MISSING_VAR");
});

test("interpolateString substitutes bare $VAR_NAME syntax", () => {
  process.env["BARE_TOKEN"] = "bare_value";
  expect(interpolateString("Bearer $BARE_TOKEN")).toBe("Bearer bare_value");
  delete process.env["BARE_TOKEN"];
});

test("interpolateString returns strings without placeholders unchanged", () => {
  expect(interpolateString("no placeholders here")).toBe("no placeholders here");
});

test("parseServerConfigJson interpolates headers for HTTP server", () => {
  process.env["MY_TOKEN"] = "tok_abc";
  const parsed = parseServerConfigJson(
    '{"mcpServers":{"srv":{"url":"http://localhost","headers":{"Authorization":"Bearer ${MY_TOKEN}"}}}}',
  );
  expect(parsed?.[0]?.backend).toEqual({
    type: "http",
    url: "http://localhost",
    headers: { Authorization: "Bearer tok_abc" },
  });
  delete process.env["MY_TOKEN"];
});

test("parseServerConfigJson interpolates env values for stdio server", () => {
  process.env["API_KEY"] = "key_xyz";
  const parsed = parseServerConfigJson(
    '{"mcpServers":{"srv":{"command":"uvx","env":{"API_KEY":"${API_KEY}"}}}}',
  );
  expect(parsed?.[0]?.backend).toEqual({
    type: "stdio",
    command: "uvx",
    env: { API_KEY: "key_xyz" },
  });
  delete process.env["API_KEY"];
});

test("interpolateMCPConfig interpolates headers in an HTTP server", () => {
  process.env["MCP_TOKEN"] = "tok_123";
  const result = interpolateMCPConfig({
    mcpServers: {
      myServer: { url: "https://example.com", headers: { Authorization: "Bearer $MCP_TOKEN" } },
    },
  });
  expect(result.mcpServers["myServer"]?.headers?.["Authorization"]).toBe("Bearer tok_123");
  delete process.env["MCP_TOKEN"];
});

test("interpolateMCPConfig interpolates env in a stdio server", () => {
  process.env["MY_KEY"] = "abc";
  const result = interpolateMCPConfig({
    mcpServers: { srv: { command: "uvx", env: { MY_KEY: "${MY_KEY}" } } },
  });
  expect(result.mcpServers["srv"]?.env?.["MY_KEY"]).toBe("abc");
  delete process.env["MY_KEY"];
});

test("interpolateMCPConfig interpolates url", () => {
  process.env["MCP_HOST"] = "myhost.example.com";
  const result = interpolateMCPConfig({
    mcpServers: { srv: { url: "https://${MCP_HOST}/mcp" } },
  });
  expect(result.mcpServers["srv"]?.url).toBe("https://myhost.example.com/mcp");
  delete process.env["MCP_HOST"];
});

test("interpolateMCPConfig accepts a JSON string", () => {
  process.env["MCP_TOKEN"] = "tok_json";
  const result = interpolateMCPConfig(
    '{"mcpServers":{"srv":{"url":"https://example.com","headers":{"Authorization":"Bearer $MCP_TOKEN"}}}}',
  );
  expect(result.mcpServers["srv"]?.headers?.["Authorization"]).toBe("Bearer tok_json");
  delete process.env["MCP_TOKEN"];
});

test("interpolateMCPConfig interpolates args in a stdio server", () => {
  process.env["EXTRA_ARG"] = "--turbo";
  const result = interpolateMCPConfig({
    mcpServers: { srv: { command: "uvx", args: ["run", "$EXTRA_ARG"] } },
  });
  expect(result.mcpServers["srv"]?.args).toEqual(["run", "--turbo"]);
  delete process.env["EXTRA_ARG"];
});

test("interpolateMCPConfig preserves unset placeholders", () => {
  delete process.env["UNSET_VAR"];
  const result = interpolateMCPConfig({
    mcpServers: { srv: { url: "https://example.com", headers: { X: "${UNSET_VAR}" } } },
  });
  expect(result.mcpServers["srv"]?.headers?.["X"]).toBe("${UNSET_VAR}");
});
