"""AST-level pipe/redirection detection for just-bash custom commands.

Subclasses :class:`just_bash.Bash` to walk the parsed AST and inject
``MCP_TOONIFY=true|false`` env-var prefixes onto each invocation of one of
our wrapper commands, based on whether its stdout is piped (``cmd | jq``)
or redirected (``cmd > out.json``).  The wrapper command in
:mod:`mcp_compressor.bash_commands` reads ``ctx.env.MCP_TOONIFY`` to choose
between TOON and raw JSON output.
"""

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import replace
from typing import Any

from just_bash import Bash, ExecResult
from just_bash.ast.types import (
    AssignmentNode,
    CaseItemNode,
    CaseNode,
    CStyleForNode,
    ForNode,
    FunctionDefNode,
    GroupNode,
    IfClause,
    IfNode,
    LiteralPart,
    PipelineNode,
    ScriptNode,
    SimpleCommandNode,
    StatementNode,
    SubshellNode,
    UntilNode,
    WhileNode,
    WordNode,
)

#: Env var injected into wrapper invocations: ``"true"`` -> emit TOON,
#: ``"false"`` -> emit raw JSON.
MCP_TOONIFY_ENV_VAR = "MCP_TOONIFY"

#: Redirection operators that send stdout somewhere other than the caller.
#: ``2>&1`` is intentionally excluded (it dup's stderr, not stdout).
_OUTPUT_REDIR_OPERATORS = frozenset({">", ">>", ">|", "&>", "&>>", "<>"})


def resolve_toonify_from_ctx(env: Any, default: bool = False) -> bool | None:
    """Read ``MCP_TOONIFY`` from *env*, returning *default* if absent."""
    if env is None:
        return default
    try:
        raw = env.get(MCP_TOONIFY_ENV_VAR)
    except AttributeError:
        return default
    if raw is None:
        return default
    return raw.lower() == "true"


def _simple_command_name(cmd: SimpleCommandNode) -> str | None:
    """Return *cmd*'s literal name, or ``None`` for dynamic invocations."""
    if cmd.name is None:
        return None
    pieces: list[str] = []
    for part in cmd.name.parts:
        if isinstance(part, LiteralPart):
            pieces.append(part.value)
        else:
            return None
    return "".join(pieces) or None


def _has_output_redirection(cmd: SimpleCommandNode) -> bool:
    """Return ``True`` if *cmd* redirects fd 1 (stdout) somewhere else."""
    for redir in cmd.redirections or ():
        # ``fd`` defaults to 1 for ``>``/``>>``/``>|``; ``&>``/``&>>`` always cover stdout.
        if redir.operator in _OUTPUT_REDIR_OPERATORS and (
            redir.fd is None or redir.fd == 1 or redir.operator in {"&>", "&>>"}
        ):
            return True
    return False


def _make_env_assignment(name: str, value: str) -> AssignmentNode:
    """Build an :class:`AssignmentNode` for ``NAME=value``."""
    word = WordNode(parts=(LiteralPart(value=value),))
    return AssignmentNode(name=name, value=word)


def _inject_toonify_env(cmd: SimpleCommandNode, *, toonify: bool) -> SimpleCommandNode:
    """Prepend ``MCP_TOONIFY=...`` to *cmd*'s assignments (preserving user-set values)."""
    if any(a.name == MCP_TOONIFY_ENV_VAR for a in cmd.assignments or ()):
        return cmd
    new_assign = _make_env_assignment(MCP_TOONIFY_ENV_VAR, "true" if toonify else "false")
    return replace(cmd, assignments=(new_assign, *(cmd.assignments or ())))


def _transform_pipeline(pipeline: PipelineNode, custom_command_names: frozenset[str]) -> PipelineNode:
    """Inject toonify hints into wrapper commands in *pipeline*.

    A command is "piped" if it is not the last in its pipeline or has an
    output redirection of its own.
    """
    n = len(pipeline.commands)
    new_commands: list[Any] = []
    for index, cmd in enumerate(pipeline.commands):
        if isinstance(cmd, SimpleCommandNode):
            name = _simple_command_name(cmd)
            if name in custom_command_names:
                is_last = index == n - 1
                is_piped = (not is_last) or _has_output_redirection(cmd)
                cmd = _inject_toonify_env(cmd, toonify=not is_piped)
        else:
            cmd = _transform_compound(cmd, custom_command_names)
        new_commands.append(cmd)
    return replace(pipeline, commands=tuple(new_commands))


def _transform_statements(
    statements: Iterable[StatementNode], custom_command_names: frozenset[str]
) -> tuple[StatementNode, ...]:
    out: list[StatementNode] = []
    for stmt in statements:
        new_pipelines = tuple(_transform_pipeline(p, custom_command_names) for p in stmt.pipelines)
        out.append(replace(stmt, pipelines=new_pipelines))
    return tuple(out)


def _transform_compound(node: Any, custom_command_names: frozenset[str]) -> Any:
    """Recurse into compound nodes (``if``/``for``/``while``/subshell/etc.)."""
    if isinstance(node, IfNode):
        new_clauses = tuple(
            IfClause(
                condition=_transform_statements(c.condition, custom_command_names),
                body=_transform_statements(c.body, custom_command_names),
            )
            for c in node.clauses
        )
        new_else = _transform_statements(node.else_body, custom_command_names) if node.else_body is not None else None
        return replace(node, clauses=new_clauses, else_body=new_else)
    if isinstance(node, ForNode):
        return replace(node, body=_transform_statements(node.body, custom_command_names))
    if isinstance(node, CStyleForNode):
        return replace(node, body=_transform_statements(node.body, custom_command_names))
    if isinstance(node, (WhileNode, UntilNode)):
        return replace(
            node,
            condition=_transform_statements(node.condition, custom_command_names),
            body=_transform_statements(node.body, custom_command_names),
        )
    if isinstance(node, CaseNode):
        new_items = tuple(
            CaseItemNode(
                patterns=item.patterns,
                body=_transform_statements(item.body, custom_command_names),
            )
            for item in node.items
        )
        return replace(node, items=new_items)
    if isinstance(node, (SubshellNode, GroupNode)):
        return replace(node, body=_transform_statements(node.body, custom_command_names))
    if isinstance(node, FunctionDefNode) and node.body is not None:
        return replace(node, body=_transform_compound(node.body, custom_command_names))
    return node


def transform_script(ast: ScriptNode, custom_command_names: Iterable[str]) -> ScriptNode:
    """Rewrite *ast* to inject ``MCP_TOONIFY`` prefixes into wrapper commands."""
    names = frozenset(custom_command_names)
    if not names:
        return ast
    return replace(ast, statements=_transform_statements(ast.statements, names))


class _PipingAwareBash(Bash):
    """:class:`Bash` subclass that transforms each script's AST before execution.

    Wraps the underlying :class:`Interpreter`'s ``execute_script`` so we get a
    hook between parse and execute *without* duplicating ``Bash.exec``'s
    parse / env / cwd / error-handling logic.
    """

    def __init__(self, *args: Any, custom_command_names: Iterable[str], **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self._mcp_custom_command_names: frozenset[str] = frozenset(custom_command_names)

        names = self._mcp_custom_command_names
        original_execute_script = self._interpreter.execute_script

        async def execute_script(node: Any) -> ExecResult:
            return await original_execute_script(transform_script(node, names))

        self._interpreter.execute_script = execute_script  # type: ignore[method-assign]


def build_piping_aware_bash(
    custom_commands: list[Any],
    *,
    fs: Any | None = None,
    cwd: str = "/",
    **kwargs: Any,
) -> Bash:
    """Construct a :class:`Bash` with built-ins + *custom_commands* + piping auto-detect."""
    from just_bash.commands import create_command_registry

    registry = create_command_registry()
    names: list[str] = []
    for command in custom_commands:
        registry[command.name] = command
        names.append(command.name)

    return _PipingAwareBash(
        custom_command_names=names,
        commands=registry,
        fs=fs,
        cwd=cwd,
        **kwargs,
    )
