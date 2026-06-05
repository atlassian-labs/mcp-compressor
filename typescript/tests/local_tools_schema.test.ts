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
  it("exposes invoke wrapper tool_input and tool_input_json schemas", () => {
    expect(compressTools(tools).invoke_tool?.inputSchema).toMatchObject({
      type: "object",
      properties: {
        tool_input: {
          type: "object",
          properties: {},
          additionalProperties: true,
        },
        tool_input_json: {
          type: "string",
        },
      },
      required: ["tool_name"],
    });
  });

  it("invokes tools with object or JSON-string input", async () => {
    const execute = vi.fn(async (input: unknown): Promise<unknown> => input);
    const compressed = compressTools({
      echo: {
        ...tools.echo,
        execute,
      },
    });

    await expect(
      compressed.invoke_tool?.execute({
        tool_name: "echo",
        tool_input: { message: "object" },
      }),
    ).resolves.toBe('{"message":"object"}');
    expect(execute).toHaveBeenLastCalledWith({ message: "object" });

    await expect(
      compressed.invoke_tool?.execute({
        tool_name: "echo",
        tool_input_json: '{"message":"json"}',
      }),
    ).resolves.toBe('{"message":"json"}');
    expect(execute).toHaveBeenLastCalledWith({ message: "json" });
  });

  it("reports invalid JSON-string input clearly", async () => {
    const compressed = compressTools(tools);
    await expect(
      compressed.invoke_tool?.execute({
        tool_name: "echo",
        tool_input_json: "{",
      }),
    ).rejects.toThrow("invalid tool_input_json");
  });

  it("defaults missing input to an empty object", async () => {
    const execute = vi.fn(async (input: unknown): Promise<unknown> => input);
    const compressed = compressTools({
      echo: {
        ...tools.echo,
        execute,
      },
    });
    await expect(compressed.invoke_tool?.execute({ tool_name: "echo" })).resolves.toBe("{}");
    expect(execute).toHaveBeenLastCalledWith({});
  });
});
