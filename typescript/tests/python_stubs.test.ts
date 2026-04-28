import { test, expect } from "vitest";

import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import {
  DEFAULT_PACKAGE_NAME,
  generatePythonStubs,
  jsonSchemaToPythonType,
  sanitizePythonIdentifier,
  sanitizePythonModuleName,
} from "../src/python_stubs.js";

const SEARCH_TOOL: Tool = {
  name: "search-issues",
  description: "Search Jira issues using JQL.",
  inputSchema: {
    type: "object",
    properties: {
      jql: { type: "string", description: "The JQL query." },
      max_results: { type: "integer", description: "How many results to return." },
      fields: { type: "array", items: { type: "string" } },
      include_archived: { type: "boolean" },
    },
    required: ["jql"],
  },
} as Tool;

const ENUM_TOOL: Tool = {
  name: "set-status",
  description: "Set the status of an issue.",
  inputSchema: {
    type: "object",
    properties: {
      issue_id: { type: "string" },
      status: { enum: ["open", "in_progress", "done"] },
    },
    required: ["issue_id", "status"],
  },
} as Tool;

test("sanitizePythonIdentifier handles non-identifier characters and keywords", () => {
  expect(sanitizePythonIdentifier("foo-bar")).toBe("foo_bar");
  expect(sanitizePythonIdentifier("class")).toBe("class_");
  expect(sanitizePythonIdentifier("123abc")).toBe("_123abc");
  expect(sanitizePythonIdentifier("")).toBe("_unnamed");
  expect(sanitizePythonIdentifier("  spaces  ")).toBe("__spaces__");
});

test("sanitizePythonModuleName lowercases and sanitises", () => {
  expect(sanitizePythonModuleName("Jira-MCP")).toBe("jira_mcp");
  expect(sanitizePythonModuleName("Atlassian.Confluence")).toBe("atlassian_confluence");
});

test("jsonSchemaToPythonType maps primitives, arrays, enums and unions", () => {
  expect(jsonSchemaToPythonType({ type: "string" })).toBe("str");
  expect(jsonSchemaToPythonType({ type: "integer" })).toBe("int");
  expect(jsonSchemaToPythonType({ type: "number" })).toBe("float");
  expect(jsonSchemaToPythonType({ type: "boolean" })).toBe("bool");
  expect(jsonSchemaToPythonType({ type: "array", items: { type: "string" } })).toBe("list[str]");
  expect(jsonSchemaToPythonType({ type: "object" })).toBe("dict[str, Any]");
  expect(jsonSchemaToPythonType({ enum: ["a", "b"] })).toBe('Literal["a", "b"]');
  expect(jsonSchemaToPythonType({ type: ["string", "null"] })).toBe("str");
  expect(jsonSchemaToPythonType({})).toBe("Any");
  expect(jsonSchemaToPythonType(null)).toBe("Any");
});

test("generatePythonStubs produces the expected file tree", () => {
  const result = generatePythonStubs([SEARCH_TOOL, ENUM_TOOL], { serverName: "jira" });

  expect(result.entryModule).toBe(`${DEFAULT_PACKAGE_NAME}.jira`);
  const paths = [...result.files.keys()].sort();
  expect(paths).toEqual([
    `${DEFAULT_PACKAGE_NAME}/jira/SKILL.md`,
    `${DEFAULT_PACKAGE_NAME}/jira/__init__.py`,
    `${DEFAULT_PACKAGE_NAME}/jira/search_issues.py`,
    `${DEFAULT_PACKAGE_NAME}/jira/set_status.py`,
  ]);
});

test("generatePythonStubs creates a service SKILL.md", () => {
  const result = generatePythonStubs([SEARCH_TOOL, ENUM_TOOL], { serverName: "jira" });
  const skill = result.files.get(`${DEFAULT_PACKAGE_NAME}/jira/SKILL.md`)!;

  expect(skill).toContain("# jira");
  expect(skill).toContain("Use this namespace for tools exposed by the `jira` service.");
  expect(skill).toContain("from tools.jira import search_issues, set_status");
  expect(skill).toContain("## Available functions");
  expect(skill).toContain("### `search_issues(jql: str");
  expect(skill).toContain("Search Jira issues using JQL.");
  expect(skill).toContain("- `jql` — The JQL query.");
});

test("generated stub renders typed signature, optionals last, and required-only payload", () => {
  const result = generatePythonStubs([SEARCH_TOOL], { serverName: "jira" });
  const source = result.files.get(`${DEFAULT_PACKAGE_NAME}/jira/search_issues.py`)!;

  // Required params come first, optionals get `| None = None`.
  expect(source).toMatch(
    /async def search_issues\(jql: str, max_results: int \| None = None, fields: list\[str\] \| None = None, include_archived: bool \| None = None\) -> Any:/,
  );

  // Required key always present in payload, optionals guarded by `is not None`.
  expect(source).toMatch(/payload\["jql"\] = jql/);
  expect(source).toMatch(/if max_results is not None:\n {8}payload\["max_results"\] = max_results/);

  // Body delegates to _call with original tool name (not the python-safe one).
  expect(source).toMatch(/return await _call\("jira", "search-issues", payload\)/);

  // Description and Args block render in the docstring.
  expect(source).toMatch(/Search Jira issues using JQL\./);
  expect(source).toMatch(/jql: The JQL query\./);
});

test("enum-typed parameter becomes a Literal", () => {
  const result = generatePythonStubs([ENUM_TOOL], { serverName: "jira" });
  const source = result.files.get(`${DEFAULT_PACKAGE_NAME}/jira/set_status.py`)!;
  expect(source).toMatch(
    /async def set_status\(issue_id: str, status: Literal\["open", "in_progress", "done"\]\) -> Any:/,
  );
});

test("__init__.py re-exports every generated function", () => {
  const result = generatePythonStubs([SEARCH_TOOL, ENUM_TOOL], { serverName: "jira" });
  const init = result.files.get(`${DEFAULT_PACKAGE_NAME}/jira/__init__.py`)!;
  expect(init).toMatch(/from \.search_issues import search_issues/);
  expect(init).toMatch(/from \.set_status import set_status/);
  expect(init).toMatch(/__all__ = \[/);
});

test("custom package name is honored", () => {
  const result = generatePythonStubs([SEARCH_TOOL], {
    serverName: "jira",
    packageName: "kotlin_client",
  });
  expect(result.entryModule).toBe("kotlin_client.jira");
  expect([...result.files.keys()]).toContain("kotlin_client/jira/search_issues.py");
});

test("server name is sanitized for use as a python module", () => {
  const result = generatePythonStubs([SEARCH_TOOL], { serverName: "Atlassian-Jira" });
  expect(result.entryModule).toBe(`${DEFAULT_PACKAGE_NAME}.atlassian_jira`);
});

test("empty tool list still produces package docs and a __init__.py", () => {
  const result = generatePythonStubs([], { serverName: "empty" });
  expect([...result.files.keys()].sort()).toEqual([
    `${DEFAULT_PACKAGE_NAME}/empty/SKILL.md`,
    `${DEFAULT_PACKAGE_NAME}/empty/__init__.py`,
  ]);
});

test("python keyword as parameter name is suffixed with underscore", () => {
  const tool: Tool = {
    name: "test",
    description: "Test tool.",
    inputSchema: {
      type: "object",
      properties: {
        class: { type: "string" },
        from: { type: "string" },
      },
      required: ["class"],
    },
  } as Tool;
  const result = generatePythonStubs([tool], { serverName: "x" });
  const source = result.files.get(`${DEFAULT_PACKAGE_NAME}/x/test.py`)!;
  expect(source).toMatch(/class_: str/);
  expect(source).toMatch(/from_: str \| None = None/);
  // Wire-format keys are preserved.
  expect(source).toMatch(/payload\["class"\] = class_/);
  expect(source).toMatch(/payload\["from"\] = from_/);
});

test("tool with no inputSchema generates a parameterless function", () => {
  const tool: Tool = {
    name: "ping",
    description: "Ping the server.",
  } as Tool;
  const result = generatePythonStubs([tool], { serverName: "x" });
  const source = result.files.get(`${DEFAULT_PACKAGE_NAME}/x/ping.py`)!;
  expect(source).toMatch(/async def ping\(\) -> Any:/);
});
