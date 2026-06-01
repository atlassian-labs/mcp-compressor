import { describe, expect, it, vi } from "vitest";

vi.mock("../src/rust_core.js", () => ({
  compressToolListing: () => "",
  formatToolSchemaResponse: () => "",
}));

import { compressTools } from "../src/local_tools.js";

const tools = {
  echo: {
    description: "Echo a message.",
    inputSchema: {
      type: "object",
      properties: {
        message: { type: "string" },
      },
      required: ["message"],
    },
    execute: async (): Promise<string> => "ok",
  },
};

describe("local compressed tool schemas", () => {
  it("exposes invoke wrapper tool_input as an explicit open object schema", () => {
    expect(compressTools(tools).invoke_tool?.inputSchema).toMatchObject({
      type: "object",
      properties: {
        tool_input: {
          type: "object",
          properties: {},
          additionalProperties: true,
        },
      },
    });
  });
});
