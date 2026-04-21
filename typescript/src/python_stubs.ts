/**
 * Python stub generator for the `python` transform mode.
 *
 * Given a list of MCP tools (from one server), generate a tree of `.py` files that an agent can
 * import to call those tools natively from Python — e.g. `await tools.jira.search_issues(jql=...)`.
 *
 * The generated functions delegate to the `_call` helper exported from the package's
 * `__init__.py` (see `python_runtime_assets.ts`), which POSTs the invocation to a loopback HTTP
 * bridge configured at runtime via an environment variable.
 *
 * This file is a pure transform: `Tool[] → Map<filename, contents>`. It performs no I/O and has no
 * runtime dependencies beyond the MCP SDK type definitions, which makes it directly testable and
 * usable in any consumer that needs to materialise the file tree into an execution environment.
 *
 * The generated content is deliberately neutral about its origins — agent-visible files contain
 * no library or host-system identifiers.
 */

import type { Tool } from "@modelcontextprotocol/sdk/types.js";

/** Default top-level package name for generated stubs. */
export const DEFAULT_PACKAGE_NAME = "tools";

/** Options controlling stub generation. */
export interface GeneratePythonStubsOptions {
  /** Name of the MCP server (used as the bundle directory name and as the `service` field in calls). */
  serverName: string;
  /** Top-level package name. Defaults to {@link DEFAULT_PACKAGE_NAME}. */
  packageName?: string;
}

/** Result of {@link generatePythonStubs}. */
export interface GeneratedPythonStubs {
  /** Filename (relative, forward-slash separated) → file contents. */
  files: ReadonlyMap<string, string>;
  /** The Python module path the LLM should import to access this server's tools. */
  entryModule: string;
}

interface ParsedParam {
  /** Original JSON Schema property name. */
  jsonName: string;
  /** Identifier-safe Python name. */
  pyName: string;
  /** Python type expression (no leading `Optional[…]`). */
  pyType: string;
  required: boolean;
  description: string | undefined;
}

/**
 * Generate Python stub files for one MCP server's tools.
 *
 * Returns a flat map of relative paths → file contents. Paths use forward slashes and are
 * relative to the consumer's chosen mount root (the consumer is responsible for creating
 * directories and writing files).
 */
export function generatePythonStubs(
  tools: ReadonlyArray<Tool>,
  opts: GeneratePythonStubsOptions,
): GeneratedPythonStubs {
  const packageName = opts.packageName ?? DEFAULT_PACKAGE_NAME;
  const serverName = sanitizePythonModuleName(opts.serverName);
  const files = new Map<string, string>();

  // Per-tool function modules.
  const exportedFunctionNames: string[] = [];
  for (const tool of tools) {
    const pyName = sanitizePythonIdentifier(tool.name);
    exportedFunctionNames.push(pyName);
    files.set(
      `${packageName}/${serverName}/${pyName}.py`,
      renderToolModule(tool, pyName, serverName),
    );
  }

  // Bundle __init__.py — re-exports each generated function.
  files.set(
    `${packageName}/${serverName}/__init__.py`,
    renderBundleInit(serverName, exportedFunctionNames),
  );

  return {
    files,
    entryModule: `${packageName}.${serverName}`,
  };
}

// ============================================================================
// Per-tool module
// ============================================================================

function renderToolModule(tool: Tool, pyName: string, serverName: string): string {
  const params = parseToolParameters(tool);
  const signature = renderSignature(params);
  const docstring = renderDocstring(tool, params);
  const body = renderBody(tool.name, serverName, params);

  const moduleHeader = (tool.description ?? "").trim();
  return [
    '"""',
    moduleHeader.length > 0 ? moduleHeader : `${tool.name} tool.`,
    '"""',
    "",
    "from __future__ import annotations",
    "",
    "from typing import Any, Literal",
    "",
    "",
    `async def ${pyName}(${signature}) -> Any:`,
    docstring,
    body,
    "",
  ].join("\n");
}

function renderSignature(params: ReadonlyArray<ParsedParam>): string {
  if (params.length === 0) {
    return "";
  }
  // Required params first, then optional. Optional ones default to `None`.
  const required = params.filter((p) => p.required);
  const optional = params.filter((p) => !p.required);
  const all = [...required, ...optional];
  return all
    .map((p) => {
      if (p.required) {
        return `${p.pyName}: ${p.pyType}`;
      }
      return `${p.pyName}: ${p.pyType} | None = None`;
    })
    .join(", ");
}

function renderDocstring(tool: Tool, params: ReadonlyArray<ParsedParam>): string {
  const lines: string[] = ['    """'];
  const desc = (tool.description ?? "").trim();
  if (desc.length > 0) {
    for (const line of desc.split("\n")) {
      lines.push(`    ${line}`);
    }
  } else {
    lines.push(`    Invoke the ${tool.name} tool.`);
  }

  if (params.length > 0) {
    lines.push("");
    lines.push("    Args:");
    for (const p of params) {
      const tail =
        p.description !== undefined && p.description.trim().length > 0
          ? p.description.trim()
          : `${p.pyType}${p.required ? "" : " (optional)"}`;
      lines.push(`        ${p.pyName}: ${escapeDocstring(tail)}`);
    }
  }

  lines.push('    """');
  return lines.join("\n");
}

function renderBody(
  toolName: string,
  serverName: string,
  params: ReadonlyArray<ParsedParam>,
): string {
  const lines: string[] = [];
  lines.push("    from .. import _call");
  lines.push("");
  lines.push("    payload: dict[str, Any] = {}");
  for (const p of params) {
    if (p.required) {
      lines.push(`    payload[${pyStr(p.jsonName)}] = ${p.pyName}`);
    } else {
      lines.push(`    if ${p.pyName} is not None:`);
      lines.push(`        payload[${pyStr(p.jsonName)}] = ${p.pyName}`);
    }
  }
  lines.push("");
  lines.push(`    return await _call(${pyStr(serverName)}, ${pyStr(toolName)}, payload)`);
  return lines.join("\n");
}

// ============================================================================
// Bundle __init__.py
// ============================================================================

function renderBundleInit(serverName: string, functionNames: ReadonlyArray<string>): string {
  const lines: string[] = [];
  lines.push(`"""${serverName} tools."""`);
  lines.push("");
  for (const name of functionNames) {
    lines.push(`from .${name} import ${name}`);
  }
  lines.push("");
  lines.push("__all__ = [");
  for (const name of functionNames) {
    lines.push(`    ${pyStr(name)},`);
  }
  lines.push("]");
  lines.push("");
  return lines.join("\n");
}

// ============================================================================
// Schema → Python type mapping
// ============================================================================

/**
 * Translate a JSON Schema fragment to a Python type expression. Handles primitives, arrays, enums,
 * and falls back to `Any` for anything we don't recognise. The result is intentionally permissive
 * — the goal is to give the LLM useful hints, not to enforce strict typing.
 */
export function jsonSchemaToPythonType(schema: unknown): string {
  if (schema === null || schema === undefined || typeof schema !== "object") {
    return "Any";
  }
  const s = schema as Record<string, unknown>;

  // Enums become Literal["a", "b", ...].
  if (Array.isArray(s["enum"]) && s["enum"].length > 0) {
    const values = s["enum"].filter((v): v is string => typeof v === "string").map((v) => pyStr(v));
    if (values.length > 0) {
      return `Literal[${values.join(", ")}]`;
    }
  }

  const t = s["type"];

  if (Array.isArray(t)) {
    // e.g. ["string", "null"] — drop "null" and recurse on the rest.
    const nonNull = t.filter((v) => v !== "null");
    if (nonNull.length === 1) {
      return jsonSchemaToPythonType({ ...s, type: nonNull[0] });
    }
    return "Any";
  }

  switch (t) {
    case "string":
      return "str";
    case "integer":
      return "int";
    case "number":
      return "float";
    case "boolean":
      return "bool";
    case "array": {
      const items = s["items"];
      return `list[${jsonSchemaToPythonType(items)}]`;
    }
    case "object":
      return "dict[str, Any]";
    default:
      return "Any";
  }
}

// ============================================================================
// Parameter parsing
// ============================================================================

function parseToolParameters(tool: Tool): ReadonlyArray<ParsedParam> {
  const schema = tool.inputSchema;
  if (schema === undefined || schema === null || typeof schema !== "object") {
    return [];
  }
  const props = (schema as { properties?: unknown }).properties;
  if (props === undefined || props === null || typeof props !== "object") {
    return [];
  }
  const requiredRaw = (schema as { required?: unknown }).required;
  const required = new Set(
    Array.isArray(requiredRaw) ? requiredRaw.filter((v): v is string => typeof v === "string") : [],
  );
  const result: ParsedParam[] = [];
  for (const [jsonName, rawSchema] of Object.entries(props as Record<string, unknown>)) {
    const description = extractDescription(rawSchema);
    result.push({
      jsonName,
      pyName: sanitizePythonIdentifier(jsonName),
      pyType: jsonSchemaToPythonType(rawSchema),
      required: required.has(jsonName),
      description,
    });
  }
  return result;
}

function extractDescription(schema: unknown): string | undefined {
  if (schema === null || typeof schema !== "object") {
    return undefined;
  }
  const desc = (schema as { description?: unknown }).description;
  return typeof desc === "string" && desc.length > 0 ? desc : undefined;
}

// ============================================================================
// Identifier sanitisation
// ============================================================================

const PY_KEYWORDS = new Set([
  "False",
  "None",
  "True",
  "and",
  "as",
  "assert",
  "async",
  "await",
  "break",
  "class",
  "continue",
  "def",
  "del",
  "elif",
  "else",
  "except",
  "finally",
  "for",
  "from",
  "global",
  "if",
  "import",
  "in",
  "is",
  "lambda",
  "nonlocal",
  "not",
  "or",
  "pass",
  "raise",
  "return",
  "try",
  "while",
  "with",
  "yield",
  "match",
  "case",
]);

/**
 * Coerce an arbitrary string into a valid Python identifier. Replaces anything that isn't an
 * ASCII letter / digit / underscore with `_`, prefixes a leading underscore if the result starts
 * with a digit, and suffixes an underscore if the result collides with a Python keyword.
 *
 * Exported so the loader integration can mirror the same naming rules when emitting prompts that
 * reference the generated symbols.
 */
export function sanitizePythonIdentifier(name: string): string {
  let sanitized = name.replaceAll(/[^A-Za-z0-9_]/g, "_");
  if (sanitized.length === 0) {
    sanitized = "_unnamed";
  }
  if (/^[0-9]/.test(sanitized)) {
    sanitized = `_${sanitized}`;
  }
  if (PY_KEYWORDS.has(sanitized)) {
    sanitized = `${sanitized}_`;
  }
  return sanitized;
}

/** Same as {@link sanitizePythonIdentifier} but stripped to lowercase for use as a module name. */
export function sanitizePythonModuleName(name: string): string {
  return sanitizePythonIdentifier(name.toLowerCase());
}

// ============================================================================
// Misc helpers
// ============================================================================

function pyStr(value: string): string {
  // JSON.stringify gives us valid double-quoted Python string semantics for ASCII content.
  return JSON.stringify(value);
}

function escapeDocstring(text: string): string {
  // Avoid prematurely closing the surrounding triple-quoted docstring.
  return text.replaceAll('"""', '\\"\\"\\"');
}
