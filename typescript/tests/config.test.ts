import { test, expect } from "vitest";

import { parseMultiServerConfigJson, parseSingleServerConfigJson } from "../src/config.js";
import { resolveAllBackends } from "../src/index.js";

test("parseSingleServerConfigJson parses a single stdio server", () => {
  const parsed = parseSingleServerConfigJson(
    '{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch"]}}}',
  );

  expect(parsed).toEqual({
    backend: {
      type: "stdio",
      command: "uvx",
      args: ["mcp-server-fetch"],
      cwd: undefined,
      env: undefined,
    },
    serverName: "fetch",
  });
});

test("parseSingleServerConfigJson rejects multiple servers", () => {
  expect(() =>
    parseSingleServerConfigJson('{"mcpServers":{"a":{"command":"uvx"},"b":{"command":"uvx"}}}'),
  ).toThrow(/exactly one server/i);
});

test("parseMultiServerConfigJson parses a single server", () => {
  const parsed = parseMultiServerConfigJson(
    '{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch"]}}}',
  );

  expect(parsed).toBeTruthy();
  expect(parsed!.length).toBe(1);
  expect(parsed![0]).toEqual({
    backend: {
      type: "stdio",
      command: "uvx",
      args: ["mcp-server-fetch"],
      cwd: undefined,
      env: undefined,
    },
    serverName: "fetch",
  });
});

test("parseMultiServerConfigJson parses multiple servers", () => {
  const parsed = parseMultiServerConfigJson(
    '{"mcpServers":{"weather":{"command":"uvx","args":["mcp-weather"]},"calendar":{"command":"uvx","args":["mcp-calendar"]}}}',
  );

  expect(parsed).toBeTruthy();
  expect(parsed!.length).toBe(2);
  expect(parsed![0]!.serverName).toBe("weather");
  expect(parsed![1]!.serverName).toBe("calendar");
  expect(parsed![0]!.backend).toEqual({
    type: "stdio",
    command: "uvx",
    args: ["mcp-weather"],
    cwd: undefined,
    env: undefined,
  });
  expect(parsed![1]!.backend).toEqual({
    type: "stdio",
    command: "uvx",
    args: ["mcp-calendar"],
    cwd: undefined,
    env: undefined,
  });
});

test("parseMultiServerConfigJson returns null for non-JSON input", () => {
  expect(parseMultiServerConfigJson("uvx mcp-server-fetch")).toBe(null);
  expect(parseMultiServerConfigJson("https://example.com/mcp")).toBe(null);
});

test("parseMultiServerConfigJson throws for empty mcpServers", () => {
  expect(() => parseMultiServerConfigJson('{"mcpServers":{}}')).toThrow(/at least one server/i);
});

test("resolveAllBackends returns a single entry for a plain URL", () => {
  const resolved = resolveAllBackends("https://example.com/mcp");
  expect(resolved.length).toBe(1);
  expect(resolved[0]!.backend).toEqual({ type: "http", url: "https://example.com/mcp" });
});

test("resolveAllBackends returns multiple entries for a multi-server JSON string", () => {
  const resolved = resolveAllBackends(
    '{"mcpServers":{"weather":{"command":"uvx","args":["mcp-weather"]},"calendar":{"command":"uvx","args":["mcp-calendar"]}}}',
  );
  expect(resolved.length).toBe(2);
  expect(resolved[0]!.serverName).toBe("weather");
  expect(resolved[1]!.serverName).toBe("calendar");
});

test("resolveAllBackends applies serverName as prefix for multi-server JSON", () => {
  const resolved = resolveAllBackends(
    '{"mcpServers":{"weather":{"command":"uvx"},"calendar":{"command":"uvx"}}}',
    "myapp",
  );
  expect(resolved[0]!.serverName).toBe("myapp_weather");
  expect(resolved[1]!.serverName).toBe("myapp_calendar");
});
