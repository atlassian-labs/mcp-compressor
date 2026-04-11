import { test, expect } from "vitest";

import { formatCliToolResult, formatToolResult, maybeToonifyText } from "../src/formatting.js";

test("maybeToonifyText returns non-json text unchanged", () => {
  expect(maybeToonifyText("hello", true)).toBe("hello");
});

test("maybeToonifyText toonifies json text when enabled", () => {
  expect(maybeToonifyText('{"hello":"world"}', true)).toMatch(/hello/);
});

test("formatToolResult toonifies JSON text blocks inside MCP tool results", () => {
  const output = formatToolResult(
    {
      content: [
        {
          type: "text",
          text: '{"hello":"world"}',
        },
      ],
      isError: false,
    },
    true,
  );

  const parsed = JSON.parse(output) as { content: Array<{ text: string }> };
  expect(parsed.content[0]?.text).not.toBe('{"hello":"world"}');
  expect(parsed.content[0]?.text ?? "").toMatch(/hello/);
});

test("formatCliToolResult unwraps MCP text blocks for CLI output", () => {
  const output = formatCliToolResult(
    {
      content: [
        {
          type: "text",
          text: '{"hello":"world"}',
        },
      ],
      isError: false,
    },
    true,
  );

  expect(output).not.toMatch(/"content"/);
  expect(output).toMatch(/hello/);
});
