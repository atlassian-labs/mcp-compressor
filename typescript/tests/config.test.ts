import { expect, test } from "vitest";
import { interpolateString, parseServerConfigJson } from "../src/config.js";

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
