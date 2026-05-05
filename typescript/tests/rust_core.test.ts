import { describe, expect, it } from "vitest";

import {
  compressToolListing,
  formatToolSchemaResponse,
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
