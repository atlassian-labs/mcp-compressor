import test from 'node:test';
import assert from 'node:assert/strict';

import { formatCliToolResult, formatToolResult, maybeToonifyText } from '../src/formatting.js';

test('maybeToonifyText returns non-json text unchanged', () => {
  assert.equal(maybeToonifyText('hello', true), 'hello');
});

test('maybeToonifyText toonifies json text when enabled', () => {
  assert.match(maybeToonifyText('{"hello":"world"}', true), /hello/);
});

test('formatToolResult toonifies JSON text blocks inside MCP tool results', () => {
  const output = formatToolResult(
    {
      content: [
        {
          type: 'text',
          text: '{"hello":"world"}',
        },
      ],
      isError: false,
    },
    true,
  );

  const parsed = JSON.parse(output) as { content: Array<{ text: string }> };
  assert.notEqual(parsed.content[0]?.text, '{"hello":"world"}');
  assert.match(parsed.content[0]?.text ?? '', /hello/);
});

test('formatCliToolResult unwraps MCP text blocks for CLI output', () => {
  const output = formatCliToolResult(
    {
      content: [
        {
          type: 'text',
          text: '{"hello":"world"}',
        },
      ],
      isError: false,
    },
    true,
  );

  assert.doesNotMatch(output, /"content"/);
  assert.match(output, /hello/);
});
