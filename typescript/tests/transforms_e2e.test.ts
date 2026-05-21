import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { Bash } from "just-bash";
import { describe, expect, it } from "vitest";

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
      properties: { message: { type: "string" } },
      required: ["message"],
    },
    execute: async (input = {}) => `alpha:${String((input as { message?: unknown }).message)}`,
  },
  summarize_payload: {
    name: "summarize_payload",
    description: "Summarize a structured payload.",
    inputSchema: {
      type: "object",
      properties: {
        items: { type: "array", items: { type: "string" } },
        metadata: { type: "object" },
        include_details: { type: "boolean" },
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

describe("host-owned transform e2e", () => {
  it("CLI transform writes a self-contained command with legacy-style structured parsing", async () => {
    const outputDir = mkdtempSync(join(tmpdir(), "mcp-cli-transform-"));
    const transform = await transformToolsForCliMode(alphaTools, {
      serverName: "alpha",
      outputDir,
    });
    try {
      const helpDescription = transform.tools.alpha_help?.description ?? "";
      expect(helpDescription).toContain(
        "Functionality associated with the alpha toolset is provided via the `alpha` CLI.",
      );
      expect(helpDescription).toContain("SUBCOMMANDS:");
      expect(helpDescription).toContain("echo               Echo a message.");
      expect(helpDescription).toContain("summarize-payload  Summarize a structured payload.");
      expect(helpDescription).not.toContain("CLI Mode");
      expect(helpDescription).not.toContain("PATH hint");

      expect(transform.paths).toHaveLength(1);
      expect(transform.files).toHaveProperty("alpha");
    } finally {
      transform.close();
    }
  });

  it("Just Bash transform uses the same top-level command and structured args", async () => {
    const bash = new Bash({ customCommands: [] });
    const transform = transformToolsForJustBash(alphaTools, { serverName: "alpha", bash });
    expect(transform.registrations.map((registration) => registration.commandName)).toEqual([
      "alpha",
    ]);
    const helpDescription = transform.tools.alpha_help?.description ?? "";
    expect(helpDescription).toContain(
      "Functionality associated with the alpha toolset is provided via the `alpha` CLI.",
    );
    expect(helpDescription).toContain("summarize-payload  Summarize a structured payload.");
    expect(helpDescription).not.toContain("Just Bash");

    const result = await bash.exec(
      'alpha summarize-payload --items one --items two --metadata \'{"source":"bash"}\' --no-include-details',
    );
    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("itemCount: 2");
    expect(result.stdout).toContain("source: bash");
    expect(result.stdout).toContain("includeDetails: false");
  });

  it("Python and TypeScript code transforms expose module/function descriptions", async () => {
    const pythonDir = mkdtempSync(join(tmpdir(), "mcp-python-transform-"));
    const tsDir = mkdtempSync(join(tmpdir(), "mcp-ts-transform-"));
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
      const pythonHelp = python.tools.alpha_help?.description ?? "";
      expect(pythonHelp).toContain("provided via a Python module");
      expect(pythonHelp).toContain(`Python source code is available in ${pythonDir}/alpha.py`);
      expect(pythonHelp).toContain("Available functions:");
      expect(pythonHelp).toContain(
        "summarize_payload(items, metadata=None, include_details=None)  Summarize a structured payload.",
      );
      expect(pythonHelp).not.toContain("Code Mode");
      expect(readFileSync(join(pythonDir, "alpha.py"), "utf8")).toContain(
        '"""Summarize a structured payload."""',
      );

      const tsHelp = typescript.tools.alpha_help?.description ?? "";
      expect(tsHelp).toContain("provided via a TypeScript module");
      expect(tsHelp).toContain(`TypeScript source code is available in ${tsDir}/alpha.ts`);
      expect(tsHelp).toContain(
        "summarizePayload(items, metadata?, include_details?)  Summarize a structured payload.",
      );
      expect(tsHelp).not.toContain("Code Mode");
      expect(readFileSync(join(tsDir, "alpha.d.ts"), "utf8")).toContain(
        "Summarize a structured payload.",
      );
    } finally {
      python.close();
      typescript.close();
    }
  });
});
