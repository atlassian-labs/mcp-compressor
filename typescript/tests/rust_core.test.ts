import { mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  compressToolListing,
  formatToolSchemaResponse,
  generateClientArtifacts,
  parseMcpConfig,
  parseToolArgv,
  type RustTool,
} from "../src/rust_core.js";

const sampleTool: RustTool = {
  name: "echo",
  description: "Echo a value.",
  inputSchema: {
    type: "object",
    properties: { message: { type: "string" } },
    required: ["message"],
  },
};

describe("Rust native core wrapper", () => {
  it("compresses tool listings through the native addon", () => {
    expect(compressToolListing("high", [sampleTool])).toBe("<tool>echo(message)</tool>");
  });

  it("formats schema responses through the native addon", () => {
    const response = formatToolSchemaResponse(sampleTool);
    expect(response).toContain("Echo a value.");
    expect(response).toContain('"message"');
  });

  it("parses generated CLI argv through the native addon", () => {
    expect(parseToolArgv(sampleTool, ["--message", "hello"])).toEqual({ message: "hello" });
  });

  it("generates client artifacts through the native addon", () => {
    const outputDir = mkdtempSync(join(tmpdir(), "mcp-compressor-rust-core-"));
    mkdirSync(outputDir, { recursive: true });
    const cliPaths = generateClientArtifacts("cli", {
      cliName: "native-alpha",
      bridgeUrl: "http://127.0.0.1:12345",
      token: "token".repeat(16),
      tools: [sampleTool],
      sessionPid: 42,
      outputDir,
    });
    expect(cliPaths).toHaveLength(1);
    expect(readFileSync(cliPaths[0]!, "utf8")).toContain("native-alpha - the native-alpha toolset");

    const pyPaths = generateClientArtifacts("python", {
      cliName: "native-alpha",
      bridgeUrl: "http://127.0.0.1:12345",
      token: "token".repeat(16),
      tools: [sampleTool],
      sessionPid: 42,
      outputDir,
    });
    expect(pyPaths.some((path) => path.endsWith(".py"))).toBe(true);

    const tsPaths = generateClientArtifacts("typescript", {
      cliName: "native-alpha",
      bridgeUrl: "http://127.0.0.1:12345",
      token: "token".repeat(16),
      tools: [sampleTool],
      sessionPid: 42,
      outputDir,
    });
    expect(tsPaths.some((path) => path.endsWith(".ts"))).toBe(true);
    expect(tsPaths.some((path) => path.endsWith(".d.ts"))).toBe(true);
  });

  it("parses MCP config through the native addon", () => {
    expect(
      parseMcpConfig('{"mcpServers":{"my-server":{"command":"python","args":["server.py"]}}}'),
    ).toEqual([
      {
        name: "my-server",
        command: "python",
        args: ["server.py"],
        env: [],
        cli_prefix: "my-server",
      },
    ]);
  });
});
