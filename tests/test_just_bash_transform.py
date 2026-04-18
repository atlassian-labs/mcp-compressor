"""Tests for the just-bash AST transformer that injects ``MCP_TOONIFY`` hints."""

from __future__ import annotations

import asyncio
from typing import Any

import pytest
from just_bash import CommandContext, ExecResult

from mcp_compressor.just_bash_transform import (
    MCP_TOONIFY_ENV_VAR,
    build_piping_aware_bash,
    resolve_toonify_from_ctx,
)


class _CapturingCommand:
    """A custom command that returns the value of ``MCP_TOONIFY`` it sees."""

    def __init__(self, name: str = "alpha") -> None:
        self.name = name

    async def execute(self, args: list[str], ctx: CommandContext) -> ExecResult:
        toon = ctx.env.get(MCP_TOONIFY_ENV_VAR, "<unset>")
        return ExecResult(stdout=f"toon={toon}", stderr="", exit_code=0)


@pytest.fixture
def bash() -> Any:
    return build_piping_aware_bash([_CapturingCommand("alpha")])


@pytest.fixture
def bash_two_commands() -> Any:
    return build_piping_aware_bash([_CapturingCommand("alpha"), _CapturingCommand("beta")])


def _exec(bash: Any, command: str) -> ExecResult:
    return asyncio.run(bash.exec(command))


def test_unpiped_command_gets_toonify_true(bash: Any) -> None:
    result = _exec(bash, "alpha foo")
    assert result.exit_code == 0
    assert result.stdout.strip() == "toon=true"


def test_piped_first_command_gets_toonify_false(bash: Any) -> None:
    result = _exec(bash, "alpha foo | wc -c")
    assert result.exit_code == 0
    assert result.stdout.strip() == str(len("toon=false"))


def test_command_at_pipeline_tail_is_not_piped(bash: Any) -> None:
    result = _exec(bash, "echo hi | alpha foo")
    assert result.exit_code == 0
    assert result.stdout.strip() == "toon=true"


def test_output_redirection_counts_as_piped(bash: Any) -> None:
    result = _exec(bash, "alpha foo > /tmp/out.json && cat /tmp/out.json")
    assert result.exit_code == 0
    assert "toon=false" in result.stdout


def test_append_redirection_also_counts_as_piped(bash: Any) -> None:
    result = _exec(bash, "alpha foo >> /tmp/out2.json && cat /tmp/out2.json")
    assert result.exit_code == 0
    assert "toon=false" in result.stdout


def test_stderr_redirection_does_not_count_as_piped(bash: Any) -> None:
    # 2>&1 dup's stderr; stdout still flows to the caller.
    result = _exec(bash, "alpha foo 2>&1")
    assert result.exit_code == 0
    assert result.stdout.strip() == "toon=true"


def test_logical_chain_does_not_count_as_piped(bash: Any) -> None:
    result = _exec(bash, "alpha foo && echo done")
    assert result.exit_code == 0
    assert "toon=true" in result.stdout
    assert "done" in result.stdout


def test_explicit_user_assignment_is_preserved(bash: Any) -> None:
    # User-supplied MCP_TOONIFY wins over the auto-injection.
    result = _exec(bash, "MCP_TOONIFY=true alpha foo | wc -c")
    assert result.exit_code == 0
    assert result.stdout.strip() == str(len("toon=true"))


def test_multiple_custom_commands_each_annotated(bash_two_commands: Any) -> None:
    # alpha is piped (toon=false), beta is last (toon=true); we only see beta.
    result = _exec(bash_two_commands, "alpha foo | beta bar")
    assert result.exit_code == 0
    assert result.stdout.strip() == "toon=true"


def test_unknown_command_is_left_alone(bash: Any) -> None:
    result = _exec(bash, "echo hello | wc -c")
    assert result.exit_code == 0
    assert result.stdout.strip() == "6"  # "hello\n"


def test_dynamic_command_name_is_left_alone(bash: Any) -> None:
    # Dynamic command names (param expansion) must be skipped without crashing.
    result = _exec(bash, "cmd=alpha; $cmd foo")
    assert result.exit_code == 0


# ---- resolve_toonify_from_ctx ------------------------------------------------


class _FakeEnv:
    def __init__(self, mapping: dict[str, str]) -> None:
        self._mapping = mapping

    def get(self, key: str, default: Any = None) -> Any:
        return self._mapping.get(key, default)


def test_resolve_toonify_returns_default_when_unset() -> None:
    assert resolve_toonify_from_ctx(_FakeEnv({}), default=True) is True
    assert resolve_toonify_from_ctx(_FakeEnv({}), default=False) is False


def test_resolve_toonify_reads_true() -> None:
    assert resolve_toonify_from_ctx(_FakeEnv({MCP_TOONIFY_ENV_VAR: "true"})) is True


def test_resolve_toonify_reads_false() -> None:
    assert resolve_toonify_from_ctx(_FakeEnv({MCP_TOONIFY_ENV_VAR: "false"})) is False


def test_resolve_toonify_handles_none_env() -> None:
    assert resolve_toonify_from_ctx(None, default=True) is True
