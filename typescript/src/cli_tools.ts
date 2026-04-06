import type { Tool } from '@modelcontextprotocol/sdk/types.js';

function firstSentence(text: string | undefined): string {
  if (!text) {
    return '';
  }
  return text.trim().split(/\n+/u)[0]?.split('. ')[0] ?? '';
}

function getInputSchema(tool: Tool): Record<string, unknown> {
  return ((tool.inputSchema ?? {}) as Record<string, unknown>) || {};
}

function unwrapNullable(schema: Record<string, unknown>): Record<string, unknown> {
  const typeValue = schema.type;
  if (Array.isArray(typeValue)) {
    const nonNull = typeValue.filter((value) => value !== 'null');
    return nonNull.length === 1 ? { ...schema, type: nonNull[0] } : schema;
  }
  return schema;
}

function schemaTypeLabel(schema: Record<string, unknown>): string {
  const unwrapped = unwrapNullable(schema);
  const typeValue = unwrapped.type;
  if (typeof typeValue === 'string') {
    return `<${typeValue}>`;
  }
  if ('properties' in unwrapped) {
    return '<json>';
  }
  return '<value>';
}

export function toolNameToSubcommand(toolName: string): string {
  return toolName
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1-$2')
    .replace(/([a-z\d])([A-Z])/g, '$1-$2')
    .replace(/_/g, '-')
    .toLowerCase();
}

export function sanitizeCliName(name: string): string {
  let sanitized = name.toLowerCase().replace(/[^a-z0-9_-]/g, '-').replace(/[-_]{2,}/g, '-').replace(/^[-_]+|[-_]+$/g, '');
  if (!sanitized) {
    sanitized = 'mcp';
  }
  if (/^\d/u.test(sanitized)) {
    sanitized = `mcp-${sanitized}`;
  }
  return sanitized;
}

export function formatTopLevelHelp(cliName: string, tools: Tool[]): string {
  const lines = tools
    .slice()
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((tool) => `  ${toolNameToSubcommand(tool.name).padEnd(35)} ${firstSentence(tool.description)}`.trimEnd());

  return [
    `${cliName} - local CLI for backend MCP tools`,
    '',
    `Usage: ${cliName} <subcommand> [options]`,
    `       ${cliName} --help`,
    '',
    'Subcommands:',
    ...lines,
    '',
    `Run '${cliName} <subcommand> --help' for per-command help.`,
  ].join('\n');
}

export function formatToolHelp(cliName: string, tool: Tool): string {
  const schema = getInputSchema(tool);
  const properties = (schema.properties ?? {}) as Record<string, Record<string, unknown>>;
  const required = new Set(((schema.required ?? []) as string[]) || []);

  const optionLines = Object.entries(properties).map(([propName, propSchema]) => {
    const flag = `--${propName.replace(/_/g, '-')}`;
    const reqLabel = required.has(propName) ? '(required)' : '(optional)';
    const desc = typeof propSchema.description === 'string' ? propSchema.description : '';
    return `  ${flag} ${schemaTypeLabel(propSchema).padEnd(10)} ${reqLabel} ${desc}`.trimEnd();
  });
  optionLines.push(`  --json <json>               (optional) Raw JSON tool_input override`);

  return [
    `${cliName} ${toolNameToSubcommand(tool.name)}`,
    '',
    tool.description?.trim() || '(no description)',
    '',
    `Usage: ${cliName} ${toolNameToSubcommand(tool.name)} [options]`,
    '',
    'Options:',
    ...optionLines,
  ].join('\n');
}

function coerceValue(raw: string, schema: Record<string, unknown>): unknown {
  const unwrapped = unwrapNullable(schema);
  const typeValue = unwrapped.type;
  if (typeValue === 'integer') {
    return Number.parseInt(raw, 10);
  }
  if (typeValue === 'number') {
    return Number.parseFloat(raw);
  }
  if (typeValue === 'boolean') {
    return raw === 'true';
  }
  if (typeValue === 'object' || typeValue === 'array' || 'properties' in unwrapped) {
    return JSON.parse(raw);
  }
  return raw;
}

export function parseArgvToToolInput(argv: string[], tool: Tool): Record<string, unknown> {
  if (argv[0] === '--json') {
    if (!argv[1]) {
      throw new Error(`--json requires a value: --json '{"key":"value"}'`);
    }
    return JSON.parse(argv[1]) as Record<string, unknown>;
  }

  const schema = getInputSchema(tool);
  const properties = (schema.properties ?? {}) as Record<string, Record<string, unknown>>;
  const required = new Set(((schema.required ?? []) as string[]) || []);
  const flagToProp = new Map<string, string>();
  for (const propName of Object.keys(properties)) {
    flagToProp.set(`--${propName}`, propName);
    flagToProp.set(`--${propName.replace(/_/g, '-')}`, propName);
    flagToProp.set(`--no-${propName.replace(/_/g, '-')}`, propName);
  }

  const result: Record<string, unknown> = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]!;
    if (arg === '--quiet') {
      continue;
    }
    const propName = flagToProp.get(arg);
    if (!propName) {
      throw new Error(`Unknown option: ${arg}`);
    }

    if (arg.startsWith('--no-')) {
      result[propName] = false;
      continue;
    }

    const propSchema = properties[propName] ?? {};
    const unwrapped = unwrapNullable(propSchema);
    const typeValue = unwrapped.type;

    if (typeValue === 'boolean') {
      result[propName] = true;
      continue;
    }

    const value = argv[i + 1];
    if (value === undefined) {
      throw new Error(`Option ${arg} requires a value.`);
    }

    const coerced = coerceValue(value, propSchema);
    if (typeValue === 'array') {
      const current = (result[propName] as unknown[] | undefined) ?? [];
      current.push(coerced);
      result[propName] = current;
    } else {
      result[propName] = coerced;
    }
    i += 1;
  }

  const missing = [...required].filter((propName) => !(propName in result));
  if (missing.length > 0) {
    throw new Error(
      `Missing required option(s): ${missing.map((name) => `--${name.replace(/_/g, '-')}`).join(', ')}`,
    );
  }

  return result;
}
