import { expect, test } from "vitest";
import { parseServerConfigJson } from "../src/config.js";

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
