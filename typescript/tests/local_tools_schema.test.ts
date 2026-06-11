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
  it("explains that invoke wrapper tool_input matches the selected tool schema", () => {
    expect(compressTools(tools).invoke_tool?.inputSchema).toMatchObject({
      type: "object",
      properties: {
        tool_input: {
          type: "object",
          properties: {},
          description:
            "JSON object matching the selected tool's input schema. Use get_tool_schema for the selected tool_name before invoking if required fields are unknown.",
          additionalProperties: true,
        },
      },
    });
  });

  it("rejects empty tool_input for tools with required fields before executing", async () => {
    const execute = vi.fn(async (): Promise<string> => "ok");
    const invokeTool = compressTools({
      echo: {
        ...tools.echo,
        execute,
      },
    }).invoke_tool;

    await expect(invokeTool?.execute({ tool_name: "echo", tool_input: {} })).rejects.toThrow(
      /echo.*message.*tool_input.*get_tool_schema/s,
    );
    expect(execute).not.toHaveBeenCalled();
  });
});
