/**
 * AST-level pipe/redirection detection for just-bash custom commands.
 *
 * Registers a ``TransformPlugin`` that walks the parsed AST and injects
 * ``MCP_TOONIFY=true|false`` env-var prefixes onto each invocation of one
 * of our wrapper commands, based on whether its stdout is piped
 * (``cmd | jq``) or redirected (``cmd > out.json``).  The wrapper command
 * in ``bash_commands.ts`` reads ``ctx.env.MCP_TOONIFY`` to choose between
 * TOON and raw JSON output.  User-supplied ``MCP_TOONIFY=...`` prefixes
 * are preserved.
 */

import type { Bash } from "just-bash";

/** Env var injected into wrapper invocations: "true" -> TOON, "false" -> JSON. */
export const MCP_TOONIFY_ENV_VAR = "MCP_TOONIFY";

/** Redirection operators that move stdout off the caller's pipe. */
const OUTPUT_REDIR_OPERATORS = new Set<string>([">", ">>", ">|", "&>", "&>>", "<>"]);

// --- minimal AST shape ------------------------------------------------------
// Type only the fields we touch so we don't depend on every AST detail.

interface LiteralPart {
  type: "Literal";
  value: string;
}

interface WordPart {
  type: string;
  value?: string;
}

interface WordNode {
  type: "Word";
  parts: WordPart[];
}

interface AssignmentNode {
  type: "Assignment";
  name: string;
  value: WordNode | null;
  append: boolean;
  array: WordNode[] | null;
}

interface RedirectionNode {
  type: "Redirection";
  fd: number | null;
  fdVariable?: string;
  operator: string;
  target: unknown;
}

interface SimpleCommandNode {
  type: "SimpleCommand";
  assignments: AssignmentNode[];
  name: WordNode | null;
  args: WordNode[];
  redirections: RedirectionNode[];
  line?: number;
}

interface PipelineNode {
  type: "Pipeline";
  commands: AnyCommandNode[];
  // ... other fields we don't touch
}

interface StatementNode {
  type: "Statement";
  pipelines: PipelineNode[];
  // ... other fields we don't touch
}

interface ScriptNode {
  type: "Script";
  statements: StatementNode[];
}

// All compound nodes have either ``body`` or ``clauses``/``items`` of
// ``StatementNode[]`` that we recurse into.  We keep this loosely typed.
type AnyCommandNode = SimpleCommandNode | Record<string, unknown>;

interface TransformInput {
  ast: ScriptNode;
  metadata: Record<string, unknown>;
}

interface TransformOutput {
  ast: ScriptNode;
  metadata?: Record<string, unknown>;
}

interface TransformPlugin {
  transform(input: TransformInput): TransformOutput;
}

// --- helpers ----------------------------------------------------------------

function isLiteralPart(part: WordPart): part is LiteralPart {
  return part.type === "Literal" && typeof part.value === "string";
}

/** Return *cmd*'s literal name, or ``null`` for dynamic invocations. */
function simpleCommandName(cmd: SimpleCommandNode): string | null {
  if (cmd.name === null) {
    return null;
  }
  const pieces: string[] = [];
  for (const part of cmd.name.parts) {
    if (!isLiteralPart(part)) {
      return null;
    }
    pieces.push(part.value);
  }
  const joined = pieces.join("");
  return joined.length > 0 ? joined : null;
}

/** Return ``true`` if *cmd* redirects fd 1 (stdout) somewhere else. */
function hasStdoutRedirection(cmd: SimpleCommandNode): boolean {
  for (const redir of cmd.redirections ?? []) {
    if (!OUTPUT_REDIR_OPERATORS.has(redir.operator)) {
      continue;
    }
    // fd defaults to 1 for >/>>/>|; &>/&>> always cover stdout.
    if (
      redir.fd === null ||
      redir.fd === 1 ||
      redir.operator === "&>" ||
      redir.operator === "&>>"
    ) {
      return true;
    }
  }
  return false;
}

function makeAssignment(name: string, value: string): AssignmentNode {
  return {
    type: "Assignment",
    name,
    value: { type: "Word", parts: [{ type: "Literal", value }] },
    append: false,
    array: null,
  };
}

function isSimpleCommand(node: unknown): node is SimpleCommandNode {
  return (
    typeof node === "object" &&
    node !== null &&
    (node as { type?: string }).type === "SimpleCommand"
  );
}

function injectToonify(cmd: SimpleCommandNode, toonify: boolean): void {
  // Preserve user-set MCP_TOONIFY assignments.
  for (const assignment of cmd.assignments ?? []) {
    if (assignment.name === MCP_TOONIFY_ENV_VAR) {
      return;
    }
  }
  cmd.assignments = [
    makeAssignment(MCP_TOONIFY_ENV_VAR, toonify ? "true" : "false"),
    ...(cmd.assignments ?? []),
  ];
}

function transformPipeline(pipeline: PipelineNode, names: Set<string>): void {
  const n = pipeline.commands.length;
  pipeline.commands.forEach((cmd, index) => {
    if (isSimpleCommand(cmd)) {
      const name = simpleCommandName(cmd);
      if (name !== null && names.has(name)) {
        const isLast = index === n - 1;
        const isPiped = !isLast || hasStdoutRedirection(cmd);
        injectToonify(cmd, !isPiped);
      }
    } else {
      transformCompound(cmd, names);
    }
  });
}

function transformStatements(statements: StatementNode[], names: Set<string>): void {
  for (const stmt of statements) {
    for (const pipeline of stmt.pipelines) {
      transformPipeline(pipeline, names);
    }
  }
}

/** Recurse into compound nodes (``if``/``for``/``while``/subshell/etc.) by shape. */
function transformCompound(node: Record<string, unknown>, names: Set<string>): void {
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) {
      for (const item of value) {
        if (item && typeof item === "object") {
          const t = (item as { type?: string }).type;
          if (t === "Statement") {
            transformStatements(value as StatementNode[], names);
            break;
          }
          if (
            "body" in item ||
            "condition" in item ||
            "patterns" in item ||
            "clauses" in item ||
            "elseBody" in item
          ) {
            transformCompound(item as Record<string, unknown>, names);
          }
        }
      }
    } else if (value && typeof value === "object") {
      transformCompound(value as Record<string, unknown>, names);
    }
  }
}

// --- public plugin factory --------------------------------------------------

/** Build a ``TransformPlugin`` that injects ``MCP_TOONIFY`` prefixes (in-place AST mutation). */
export function createPipingHintPlugin(customCommandNames: readonly string[]): TransformPlugin {
  const names = new Set(customCommandNames);
  return {
    transform({ ast, metadata }) {
      if (names.size === 0) {
        return { ast, metadata };
      }
      transformStatements(ast.statements, names);
      return { ast, metadata };
    },
  };
}

/** Read ``MCP_TOONIFY`` from *env*, returning *defaultValue* if absent/invalid. */
export function resolveToonifyFromEnv(
  env: Map<string, string> | Record<string, string> | undefined,
  defaultValue: boolean,
): boolean {
  if (!env) {
    return defaultValue;
  }
  let raw: string | undefined;
  if (env instanceof Map) {
    raw = env.get(MCP_TOONIFY_ENV_VAR);
  } else {
    raw = env[MCP_TOONIFY_ENV_VAR];
  }
  if (raw === undefined) {
    return defaultValue;
  }
  const normalized = raw.trim().toLowerCase();
  if (normalized === "true") {
    return true;
  }
  if (normalized === "false") {
    return false;
  }
  return defaultValue;
}

/** Register :func:`createPipingHintPlugin` on *bash*. */
export function installPipingHintPlugin(bash: Bash, customCommandNames: readonly string[]): void {
  bash.registerTransformPlugin(createPipingHintPlugin(customCommandNames));
}
