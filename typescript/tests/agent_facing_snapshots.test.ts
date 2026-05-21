import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { Bash } from "just-bash";
import { describe, expect, it } from "vitest";

import { compressTools } from "../src/local_tools.js";
import {
  transformToolsForCliMode,
  transformToolsForCodeMode,
  transformToolsForJustBash,
} from "../src/transforms.js";
import type { ExecutableTool } from "../src/adapters.js";

const alphaTools: Record<string, ExecutableTool<unknown>> = {
  echo: {
    name: "echo",
    description: "Echo a message.",
    inputSchema: {
      type: "object",
      properties: { message: { type: "string", description: "Message to echo" } },
      required: ["message"],
    },
    execute: async (input = {}) => `alpha:${String((input as { message?: unknown }).message)}`,
  },
  add: {
    name: "add",
    description: "Add two integers.",
    inputSchema: {
      type: "object",
      properties: {
        a: { type: "integer", description: "Left operand" },
        b: { type: "integer", description: "Right operand" },
      },
      required: ["a", "b"],
    },
    execute: async (input = {}) =>
      Number((input as { a?: unknown }).a) + Number((input as { b?: unknown }).b),
  },
  summarize_payload: {
    name: "summarize_payload",
    description: "Summarize a structured payload.",
    inputSchema: {
      type: "object",
      properties: {
        items: {
          type: "array",
          items: { type: "string" },
          description: "Items to summarize",
        },
        metadata: { type: "object", description: "Arbitrary metadata" },
        include_details: { type: "boolean", description: "Include detailed rows" },
      },
      required: ["items"],
    },
    execute: async (input = {}) => ({
      itemCount: Array.isArray((input as { items?: unknown }).items)
        ? (input as { items: unknown[] }).items.length
        : 0,
      metadata: (input as { metadata?: unknown }).metadata,
      includeDetails: (input as { include_details?: unknown }).include_details ?? true,
    }),
  },
};

function normalizePaths(value: string, replacements: Record<string, string>): string {
  return Object.entries(replacements).reduce(
    (text, [actual, placeholder]) => text.split(actual).join(placeholder),
    value,
  );
}

function runScript(scriptPath: string, args: readonly string[]): string {
  return execFileSync(scriptPath, [...args], { encoding: "utf8" }).trimEnd();
}

function golden(relativePath: string): string {
  return readFileSync(
    join(process.cwd(), "..", "testdata", "golden", relativePath),
    "utf8",
  ).trimEnd();
}

function toStableJson(value: unknown): string {
  return JSON.stringify(value, Object.keys(value as Record<string, unknown>).sort(), 2);
}

describe("agent-facing alpha snapshots", () => {
  it("snapshots CLI command help and invocation output for host-owned tools", async () => {
    const outputDir = mkdtempSync(join(tmpdir(), "mcp-alpha-cli-snapshot-"));
    const transform = await transformToolsForCliMode(alphaTools, {
      serverName: "alpha",
      outputDir,
    });
    try {
      const scriptPath = join(outputDir, "alpha");
      expect(runScript(scriptPath, ["--help"])).toBe(golden("agent-facing/cli/alpha-help.txt"));
      expect(runScript(scriptPath, ["echo", "--help"])).toBe(
        golden("agent-facing/cli/alpha-echo-help.txt"),
      );
    } finally {
      transform.close();
    }
  });

  it("snapshots CLI and Just Bash help tool descriptions", async () => {
    const outputDir = mkdtempSync(join(tmpdir(), "mcp-alpha-cli-help-snapshot-"));
    const cli = await transformToolsForCliMode(alphaTools, { serverName: "alpha", outputDir });
    const bash = new Bash({ customCommands: [] });
    const justBash = transformToolsForJustBash(alphaTools, { serverName: "alpha", bash });
    try {
      const cliDescription = normalizePaths(cli.tools.alpha_help?.description ?? "", {
        [outputDir]: "<cli-dir>",
      });
      expect(cliDescription).toBe(golden("agent-facing/cli/alpha-help-tool-description.txt"));
      expect(justBash.tools.alpha_help?.description).toBe(cli.tools.alpha_help?.description);
      expect(justBash.tools.alpha_help?.description).toBe(
        golden("agent-facing/cli/alpha-help-tool-description.txt"),
      );

      const bashResult = await bash.exec("alpha echo --message snapshot");
      expect(bashResult.stdout.trimEnd()).toBe(golden("agent-facing/cli/alpha-echo-output.txt"));
    } finally {
      cli.close();
    }
  });

  it("snapshots camelCase tool and property names as kebab-case CLI affordances", async () => {
    const outputDir = mkdtempSync(join(tmpdir(), "mcp-atlassian-cli-snapshot-"));
    const transform = await transformToolsForCliMode(
      {
        searchJiraIssuesUsingJql: {
          name: "searchJiraIssuesUsingJql",
          description: "Search issues with JQL",
          inputSchema: {
            type: "object",
            properties: {
              cloudId: { type: "string", description: "Cloud ID" },
              jql: { type: "string", description: "JQL query" },
              maxResults: { type: "number", description: "Max results" },
              nextPageToken: { type: "string", description: "Page token" },
            },
            required: ["cloudId", "jql"],
          },
          execute: async (): Promise<unknown> => "ok",
        },
      },
      { serverName: "atlassian", outputDir },
    );
    try {
      const scriptPath = join(outputDir, "atlassian");
      expect(runScript(scriptPath, ["--help"])).toBe(
        golden("agent-facing/atlassian-like/atlassian-help.txt"),
      );
      expect(runScript(scriptPath, ["search-jira-issues-using-jql", "--help"])).toBe(
        golden("agent-facing/atlassian-like/search-jira-issues-using-jql-help.txt"),
      );
    } finally {
      transform.close();
    }
  });

  it("snapshots Python and TypeScript code-mode help descriptions", async () => {
    const pythonDir = mkdtempSync(join(tmpdir(), "mcp-alpha-python-snapshot-"));
    const tsDir = mkdtempSync(join(tmpdir(), "mcp-alpha-ts-snapshot-"));
    const python = await transformToolsForCodeMode(alphaTools, {
      serverName: "alpha",
      language: "python",
      outputDir: pythonDir,
    });
    const typescript = await transformToolsForCodeMode(alphaTools, {
      serverName: "alpha",
      language: "typescript",
      outputDir: tsDir,
    });
    try {
      expect(
        normalizePaths(python.tools.alpha_help?.description ?? "", { [pythonDir]: "<python-dir>" }),
      ).toBe(golden("agent-facing/code/alpha-python-help-tool-description.txt"));
      expect(
        normalizePaths(typescript.tools.alpha_help?.description ?? "", { [tsDir]: "<ts-dir>" }),
      ).toBe(golden("agent-facing/code/alpha-typescript-help-tool-description.txt"));

      expect(readFileSync(join(pythonDir, "alpha.py"), "utf8")).toContain('"""Echo a message."""');
      expect(readFileSync(join(tsDir, "alpha.d.ts"), "utf8")).toContain("Echo a message.");
    } finally {
      python.close();
      typescript.close();
    }
  });

  it("snapshots Atlassian-like Python code signatures", async () => {
    const pythonDir = mkdtempSync(join(tmpdir(), "mcp-atlassian-python-snapshot-"));
    const transform = await transformToolsForCodeMode(
      {
        atlassianUserInfo: {
          name: "atlassianUserInfo",
          description: "Get current user info",
          inputSchema: { type: "object", properties: {} },
          execute: async (): Promise<unknown> => "ok",
        },
        searchJiraIssuesUsingJql: {
          name: "searchJiraIssuesUsingJql",
          description: "Search issues with JQL",
          inputSchema: {
            type: "object",
            properties: {
              cloudId: { type: "string", description: "Cloud ID" },
              jql: { type: "string", description: "JQL query" },
              maxResults: { type: "number", description: "Max results" },
              fields: { type: "array", items: { type: "string" }, description: "Fields" },
            },
            required: ["cloudId", "jql"],
          },
          execute: async (): Promise<unknown> => "ok",
        },
      },
      { serverName: "atlassian", language: "python", outputDir: pythonDir },
    );
    try {
      expect(
        normalizePaths(transform.tools.atlassian_help?.description ?? "", {
          [pythonDir]: "<python-dir>",
        }),
      ).toBe(golden("agent-facing/code/atlassian-python-help-tool-description.txt"));
      const source = readFileSync(join(pythonDir, "atlassian.py"), "utf8");
      expect(source).toContain("def atlassian_user_info() -> str:");
      expect(source).toContain(
        "def search_jira_issues_using_jql(cloud_id, jql, max_results=None, fields=None) -> str:",
      );
      expect(source).toContain(JSON.stringify("cloudId") + ": cloud_id");
      expect(source).not.toContain("def atlassianUserInfo(");
    } finally {
      transform.close();
    }
  });

  it("snapshots standard compressed tool descriptions and responses", async () => {
    const compressed = compressTools(alphaTools, {
      compressionLevel: "medium",
      namePrefix: "alpha",
    });
    expect(
      toStableJson(
        Object.fromEntries(
          Object.entries(compressed).map(([name, tool]) => [name, tool.description]),
        ),
      ),
    ).toBe(golden("agent-facing/compressed/alpha-tool-descriptions.json"));

    await expect(compressed.alpha_get_tool_schema?.execute({ tool_name: "echo" })).resolves.toBe(
      golden("agent-facing/compressed/alpha-get-schema-echo.txt"),
    );

    await expect(
      compressed.alpha_invoke_tool?.execute({
        tool_name: "echo",
        tool_input: { message: "snapshot" },
      }),
    ).resolves.toBe(golden("agent-facing/compressed/alpha-invoke-echo.txt"));
  });
});
