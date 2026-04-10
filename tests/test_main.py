"""Tests for mcp_compressor/main.py."""

import importlib.metadata
import logging
import re
from contextlib import asynccontextmanager
from typing import Any, cast

import pytest
from fastmcp.client.auth.bearer import BearerAuth
from fastmcp.client.auth.oauth import ClientNotFoundError, OAuth
from fastmcp.client.transports import SSETransport, StdioTransport, StreamableHttpTransport
from fastmcp.exceptions import McpError
from typer.testing import CliRunner

import mcp_compressor.logging as logging_module
import mcp_compressor.main as main_module
from mcp_compressor.logging import (
    _RecoverableOAuthTracebackFilter,
    suppress_recoverable_oauth_traceback_logging,
)
from mcp_compressor.main import (
    _get_single_server_transport_from_mcp_config,
    _get_sse_transport,
    _get_stdio_transport,
    _get_streamable_http_transport,
    _interpolate_string,
    _parse_mcp_config_json,
    _parse_single_server_mcp_config,
    _parse_tool_name_list,
    _proxy_client,
    _server,
    app,
)
from mcp_compressor.types import CompressionLevel


@pytest.fixture
def runner() -> CliRunner:
    return CliRunner()


@pytest.fixture(autouse=True)
def setup_env_vars(monkeypatch: pytest.MonkeyPatch) -> None:
    """Set up test environment variables."""
    monkeypatch.setenv("TEST_VAR", "test_value")
    monkeypatch.setenv("API_KEY", "secret123")


@pytest.mark.parametrize(
    "input_str,expected",
    [
        # Basic interpolation
        ("${TEST_VAR}", "test_value"),
        ("${API_KEY}", "secret123"),
        # With surrounding text
        ("prefix_${TEST_VAR}_suffix", "prefix_test_value_suffix"),
        ("Bearer ${API_KEY}", "Bearer secret123"),
        # Multiple variables
        ("${TEST_VAR}:${API_KEY}", "test_value:secret123"),
        # No variables - pass through unchanged
        ("plain_string", "plain_string"),
        ("", ""),
        # Dollar sign without braces - unchanged
        ("$TEST_VAR", "$TEST_VAR"),
    ],
)
def test_interpolate_string(input_str: str, expected: str) -> None:
    """Test that environment variables are correctly interpolated."""
    assert _interpolate_string(input_str) == expected


def test_interpolate_string_missing_var_returns_original() -> None:
    """Test that missing variables return the original string."""
    result = _interpolate_string("${NONEXISTENT_VAR}")
    assert result == "${NONEXISTENT_VAR}"


def test_interpolate_string_partial_missing_returns_original() -> None:
    """Test that partial interpolation failure returns original string."""
    result = _interpolate_string("${TEST_VAR}_${NONEXISTENT}")
    assert result == "${TEST_VAR}_${NONEXISTENT}"


@pytest.mark.parametrize(
    ("tool_name_group", "expected"),
    [
        (None, None),
        ("", None),
        ("add,do_nothing", ["add", "do_nothing"]),
        (" add , do_nothing , empty_tool ", ["add", "do_nothing", "empty_tool"]),
        ("add,,do_nothing", ["add", "do_nothing"]),
    ],
)
def test_parse_tool_name_list(tool_name_group: str | None, expected: list[str] | None) -> None:
    """Test parsing comma-separated tool lists from CLI options."""
    assert _parse_tool_name_list(tool_name_group) == expected


def test_parse_single_server_mcp_config() -> None:
    config_json = '{"mcpServers": {"weather": {"command": "uvx", "args": ["mcp-weather"]}}}'

    parsed = _parse_single_server_mcp_config([config_json])
    assert parsed is not None
    config, server_name = parsed

    assert server_name == "weather"
    assert list(config.mcpServers) == ["weather"]


def test_parse_single_server_mcp_config_rejects_multiple_servers() -> None:
    config_json = (
        '{"mcpServers": {'
        '"weather": {"command": "uvx", "args": ["mcp-weather"]}, '
        '"calendar": {"command": "uvx", "args": ["mcp-calendar"]}'
        "}}"
    )

    with pytest.raises(ValueError, match="exactly one server"):
        _parse_single_server_mcp_config([config_json])


def test_parse_mcp_config_json_single_server() -> None:
    config_json = '{"mcpServers": {"weather": {"command": "uvx", "args": ["mcp-weather"]}}}'

    config = _parse_mcp_config_json([config_json])
    assert config is not None
    assert list(config.mcpServers) == ["weather"]


def test_parse_mcp_config_json_multiple_servers() -> None:
    config_json = (
        '{"mcpServers": {'
        '"weather": {"command": "uvx", "args": ["mcp-weather"]}, '
        '"calendar": {"command": "uvx", "args": ["mcp-calendar"]}'
        "}}"
    )

    config = _parse_mcp_config_json([config_json])
    assert config is not None
    assert list(config.mcpServers) == ["weather", "calendar"]


def test_parse_mcp_config_json_returns_none_for_non_json() -> None:
    assert _parse_mcp_config_json(["uvx", "mcp-server-fetch"]) is None
    assert _parse_mcp_config_json(["https://example.com/mcp"]) is None


def test_parse_mcp_config_json_rejects_empty_servers() -> None:
    with pytest.raises(ValueError, match="at least one server"):
        _parse_mcp_config_json(['{"mcpServers": {}}'])


# Tests for transport creation functions


def test_get_single_server_transport_from_mcp_config_remote_defaults_to_oauth(monkeypatch: pytest.MonkeyPatch) -> None:
    config_json = '{"mcpServers": {"weather": {"url": "https://example.com/mcp"}}}'
    parsed = _parse_single_server_mcp_config([config_json])
    assert parsed is not None
    config, _ = parsed

    token_storage = object()
    monkeypatch.setattr(main_module, "_build_token_storage", lambda: token_storage)

    transport, transport_type = _get_single_server_transport_from_mcp_config(config=config)

    assert transport_type == "http"
    assert isinstance(transport, StreamableHttpTransport)
    assert isinstance(transport.auth, OAuth)
    assert transport.auth.mcp_url == "https://example.com/mcp"
    assert transport.auth._token_storage is token_storage


def test_get_single_server_transport_from_mcp_config_remote_preserves_explicit_auth() -> None:
    config_json = '{"mcpServers": {"weather": {"url": "https://example.com/mcp", "auth": "abc"}}}'
    parsed = _parse_single_server_mcp_config([config_json])
    assert parsed is not None
    config, _ = parsed

    transport, transport_type = _get_single_server_transport_from_mcp_config(config=config)

    assert transport_type == "http"
    assert isinstance(transport, StreamableHttpTransport)
    assert isinstance(transport.auth, BearerAuth)
    assert transport.auth.token.get_secret_value() == "abc"


def test_get_single_server_transport_from_mcp_config_sse_uses_only_config_timeout() -> None:
    config_json = '{"mcpServers": {"weather": {"url": "https://example.com/sse", "transport": "sse"}}}'
    parsed = _parse_single_server_mcp_config([config_json])
    assert parsed is not None
    config, _ = parsed

    transport, transport_type = _get_single_server_transport_from_mcp_config(config=config)

    assert transport_type == "sse"
    assert isinstance(transport, SSETransport)
    assert transport.sse_read_timeout is None


def test_get_stdio_transport(tmp_path) -> None:
    """Test that stdio transport is created with correct parameters."""
    transport = _get_stdio_transport(
        command="python",
        args=["-m", "my_server"],
        cwd=str(tmp_path),
        env_list=["FOO=bar", "BAZ=qux"],
    )
    assert isinstance(transport, StdioTransport)


def test_get_stdio_transport_no_env() -> None:
    """Test stdio transport with no environment variables."""
    transport = _get_stdio_transport(command="python", args=[], cwd=None, env_list=None)
    assert isinstance(transport, StdioTransport)


def test_get_stdio_transport_inherits_parent_env(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test that stdio transport inherits environment variables from parent process."""
    # Set an environment variable in the parent process
    monkeypatch.setenv("PARENT_VAR", "parent_value")

    # Create transport without explicit env_list
    transport = _get_stdio_transport(command="python", args=[], cwd=None, env_list=None)

    # Verify the transport has env configured and includes parent environment
    assert isinstance(transport, StdioTransport)
    assert transport.env is not None
    assert "PARENT_VAR" in transport.env
    assert transport.env["PARENT_VAR"] == "parent_value"


def test_get_stdio_transport_explicit_env_overrides_parent(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test that explicit -e args override parent environment variables."""
    # Set an environment variable in the parent process
    monkeypatch.setenv("MY_VAR", "parent_value")
    monkeypatch.setenv("KEEP_VAR", "keep_this")

    # Create transport with explicit env that overrides MY_VAR
    transport = _get_stdio_transport(
        command="python", args=[], cwd=None, env_list=["MY_VAR=overridden_value", "NEW_VAR=new_value"]
    )

    # Verify the transport has the overridden value
    assert isinstance(transport, StdioTransport)
    assert transport.env is not None
    assert transport.env["MY_VAR"] == "overridden_value"  # Overridden
    assert transport.env["NEW_VAR"] == "new_value"  # New variable
    assert transport.env["KEEP_VAR"] == "keep_this"  # Inherited from parent


def test_get_streamable_http_transport() -> None:
    """Test that HTTP transport is created with correct parameters."""
    transport = _get_streamable_http_transport(
        url="https://example.com/mcp",
        header_list=["Authorization=Bearer token"],
        timeout=30.0,
    )
    assert isinstance(transport, StreamableHttpTransport)
    assert isinstance(transport.auth, OAuth)


def test_get_sse_transport() -> None:
    """Test that SSE transport is created with correct parameters."""
    transport = _get_sse_transport(
        url="https://example.com/sse",
        header_list=["X-Custom=value"],
        timeout=15.0,
    )
    assert isinstance(transport, SSETransport)
    assert isinstance(transport.auth, OAuth)


async def test_remote_server_connects_eagerly() -> None:
    """Test that remote proxy startup eagerly connects to the upstream backend."""
    # The server should attempt to connect to the upstream backend eagerly,
    # which means it will raise a connection error for an unreachable URL.
    with pytest.raises((McpError, Exception)):
        async with _server(
            command_or_url_list=["https://example.com/mcp"],
            cwd=None,
            env_list=None,
            header_list=None,
            timeout=10.0,
            compression_level=CompressionLevel.MEDIUM,
            server_name=None,
        ) as _:
            pass


def _strip_ansi(text: str) -> str:
    return re.sub(r"\x1b\[[0-9;]*m", "", text)


def test_cli_mode_without_server_name_raises(runner: CliRunner) -> None:
    """Test that --cli-mode without --server-name exits with a bad parameter error."""
    result = runner.invoke(app, ["--cli-mode", "uvx", "some-mcp-server"])
    assert result.exit_code != 0
    assert "--server-name" in _strip_ansi(result.output)


def test_max_compression_without_server_name_raises(runner: CliRunner) -> None:
    """Test that --compression-level=max without --server-name exits with a bad parameter error."""
    result = runner.invoke(app, ["--compression-level", "max", "uvx", "some-mcp-server"])
    assert result.exit_code != 0
    assert "--server-name" in _strip_ansi(result.output)


def test_default_log_level_is_error(runner: CliRunner, monkeypatch: pytest.MonkeyPatch) -> None:
    captured: dict[str, Any] = {}

    async def fake_async_main(**kwargs: Any) -> None:
        captured.update(kwargs)

    original_asyncio_run = main_module.asyncio.run

    def fake_run(coro):
        original_asyncio_run(coro)

    monkeypatch.setattr(main_module, "_async_main", fake_async_main)
    monkeypatch.setattr(main_module.asyncio, "run", fake_run)

    result = runner.invoke(app, ["uvx", "some-mcp-server"])

    assert result.exit_code == 0
    assert captured["log_level"] == main_module.LogLevel.ERROR


def test_cli_mode_uses_server_name_from_single_server_mcp_config(
    runner: CliRunner, monkeypatch: pytest.MonkeyPatch
) -> None:
    captured: dict[str, Any] = {}

    async def fake_async_main(**kwargs: Any) -> None:
        captured.update(kwargs)

    original_asyncio_run = main_module.asyncio.run

    def fake_run(coro):
        original_asyncio_run(coro)

    monkeypatch.setattr(main_module, "_async_main", fake_async_main)
    monkeypatch.setattr(main_module.asyncio, "run", fake_run)

    config_json = '{"mcpServers": {"weather": {"command": "uvx", "args": ["mcp-weather"]}}}'
    result = runner.invoke(app, ["--cli-mode", config_json])

    assert result.exit_code == 0
    assert captured["server_name"] == "weather"


def test_server_name_option_overrides_single_server_mcp_config_name(
    runner: CliRunner, monkeypatch: pytest.MonkeyPatch
) -> None:
    captured: dict[str, Any] = {}

    async def fake_async_main(**kwargs: Any) -> None:
        captured.update(kwargs)

    original_asyncio_run = main_module.asyncio.run

    def fake_run(coro):
        original_asyncio_run(coro)

    monkeypatch.setattr(main_module, "_async_main", fake_async_main)
    monkeypatch.setattr(main_module.asyncio, "run", fake_run)

    config_json = '{"mcpServers": {"weather": {"command": "uvx", "args": ["mcp-weather"]}}}'
    result = runner.invoke(app, ["--cli-mode", "--server-name", "custom", config_json])

    assert result.exit_code == 0
    assert captured["server_name"] == "custom"


def test_multi_server_mcp_config_accepted_via_cli(
    runner: CliRunner, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Multi-server MCP JSON configs are now accepted and passed to _async_main."""
    config_json = (
        '{"mcpServers": {'
        '"weather": {"command": "uvx", "args": ["mcp-weather"]}, '
        '"calendar": {"command": "uvx", "args": ["mcp-calendar"]}'
        "}}"
    )
    async_main_called = False

    async def fake_async_main(**kwargs: Any) -> None:
        nonlocal async_main_called
        async_main_called = True

    monkeypatch.setattr(main_module, "_async_main", fake_async_main)
    result = runner.invoke(app, [config_json])

    assert result.exit_code == 0
    assert async_main_called is True


@pytest.mark.parametrize(
    ("extra_args", "expected_option"),
    [
        (["--cwd", "."], "--cwd"),
        (["--env", "FOO=bar"], "--env"),
        (["--header", "Authorization=Bearer abc"], "--header"),
        (["--timeout", "30"], "--timeout"),
    ],
)
def test_single_server_mcp_config_rejects_conflicting_transport_options(
    runner: CliRunner, monkeypatch: pytest.MonkeyPatch, extra_args: list[str], expected_option: str
) -> None:
    config_json = '{"mcpServers": {"weather": {"url": "https://example.com/mcp"}}}'
    async_main_called = False

    async def fake_async_main(**kwargs: Any) -> None:
        nonlocal async_main_called
        async_main_called = True

    monkeypatch.setattr(main_module, "_async_main", fake_async_main)
    result = runner.invoke(app, [*extra_args, config_json])

    assert result.exit_code != 0
    assert expected_option in _strip_ansi(result.output)
    assert async_main_called is False


async def test_server_uses_single_server_config_transport_directly(monkeypatch: pytest.MonkeyPatch) -> None:
    config_json = '{"mcpServers": {"weather": {"command": "uvx", "args": ["mcp-weather"]}}}'
    captured: dict[str, Any] = {}

    @asynccontextmanager
    async def fake_proxy_client(transport):
        captured["transport"] = transport
        yield object()

    class FakeCompressedTools:
        def __init__(self, mcp, **kwargs) -> None:
            self.mcp = mcp
            captured["compressed_tools_kwargs"] = kwargs

        async def configure_server(self) -> None:
            return None

        async def get_compression_stats(self) -> dict[str, int]:
            return {"compressed": 1, "original": 1}

    fake_mcp = object()

    monkeypatch.setattr(main_module, "_proxy_client", fake_proxy_client)
    monkeypatch.setattr(main_module, "create_proxy", lambda client, name: fake_mcp)
    monkeypatch.setattr(main_module, "CompressedTools", FakeCompressedTools)
    monkeypatch.setattr(main_module, "print_banner", lambda *args, **kwargs: None)

    async with _server(
        command_or_url_list=[config_json],
        cwd=None,
        env_list=None,
        header_list=None,
        timeout=10.0,
        compression_level=CompressionLevel.MEDIUM,
        server_name="weather",
    ) as mcp:
        assert mcp is fake_mcp

    assert isinstance(captured["transport"], StdioTransport)
    assert captured["transport"].command == "uvx"
    assert captured["transport"].args == ["mcp-weather"]
    assert captured["compressed_tools_kwargs"]["server_name"] == "weather"


async def test_server_uses_multi_server_config(monkeypatch: pytest.MonkeyPatch) -> None:
    """Multi-server JSON config creates separate proxy clients for each backend."""
    config_json = (
        '{"mcpServers": {'
        '"weather": {"command": "uvx", "args": ["mcp-weather"]}, '
        '"calendar": {"command": "uvx", "args": ["mcp-calendar"]}'
        "}}"
    )
    captured: dict[str, Any] = {"transports": [], "server_names": []}

    @asynccontextmanager
    async def fake_proxy_client(transport):
        captured["transports"].append(transport)
        yield object()

    class FakeCompressedTools:
        def __init__(self, mcp, **kwargs) -> None:
            self.mcp = mcp
            captured["server_names"].append(kwargs.get("server_name"))

        async def configure_server(self) -> None:
            return None

        async def get_compression_stats(self) -> dict[str, int]:
            return {"compressed": 1, "original": 1}

    monkeypatch.setattr(main_module, "_proxy_client", fake_proxy_client)
    monkeypatch.setattr(main_module, "create_proxy", lambda client, name: object())
    monkeypatch.setattr(main_module, "CompressedTools", FakeCompressedTools)
    monkeypatch.setattr(main_module, "print_banner", lambda *args, **kwargs: None)

    async with _server(
        command_or_url_list=[config_json],
        cwd=None,
        env_list=None,
        header_list=None,
        timeout=10.0,
        compression_level=CompressionLevel.MEDIUM,
        server_name=None,
    ) as mcp:
        from fastmcp import FastMCP

        assert isinstance(mcp, FastMCP)

    assert len(captured["transports"]) == 2
    assert isinstance(captured["transports"][0], StdioTransport)
    assert captured["transports"][0].command == "uvx"
    assert captured["transports"][0].args == ["mcp-weather"]
    assert isinstance(captured["transports"][1], StdioTransport)
    assert captured["transports"][1].command == "uvx"
    assert captured["transports"][1].args == ["mcp-calendar"]
    assert captured["server_names"] == ["weather", "calendar"]


def test_multi_server_mcp_config_rejects_cli_mode(
    runner: CliRunner, monkeypatch: pytest.MonkeyPatch
) -> None:
    """--cli-mode cannot be combined with a multi-server MCP JSON config."""
    config_json = (
        '{"mcpServers": {'
        '"weather": {"command": "uvx", "args": ["mcp-weather"]}, '
        '"calendar": {"command": "uvx", "args": ["mcp-calendar"]}'
        "}}"
    )
    async_main_called = False

    async def fake_async_main(**kwargs: Any) -> None:
        nonlocal async_main_called
        async_main_called = True

    monkeypatch.setattr(main_module, "_async_main", fake_async_main)
    result = runner.invoke(app, ["--cli-mode", config_json])

    assert result.exit_code != 0
    assert async_main_called is False


def test_multi_server_mcp_config_with_server_name_prefix(
    runner: CliRunner, monkeypatch: pytest.MonkeyPatch
) -> None:
    """--server-name acts as a common prefix for all backend names in multi-server mode."""
    config_json = (
        '{"mcpServers": {'
        '"weather": {"command": "uvx", "args": ["mcp-weather"]}, '
        '"calendar": {"command": "uvx", "args": ["mcp-calendar"]}'
        "}}"
    )
    captured: dict[str, Any] = {}

    async def fake_async_main(**kwargs: Any) -> None:
        captured.update(kwargs)

    original_asyncio_run = main_module.asyncio.run

    def fake_run(coro):
        original_asyncio_run(coro)

    monkeypatch.setattr(main_module, "_async_main", fake_async_main)
    monkeypatch.setattr(main_module.asyncio, "run", fake_run)

    result = runner.invoke(app, ["--server-name", "myapp", config_json])

    assert result.exit_code == 0
    # server_name is forwarded as-is; _multi_server uses it as a prefix internally
    assert captured.get("server_name") == "myapp"


def test_recoverable_oauth_traceback_filter_suppresses_known_stale_oauth_signatures() -> None:
    """Test that only the recoverable stale OAuth traceback logs are suppressed."""
    log_filter = _RecoverableOAuthTracebackFilter()

    # Suppressed: upstream 500 error
    recoverable_500_record = logging.makeLogRecord({
        "msg": "OAuth flow error",
        "exc_info": (RuntimeError, RuntimeError("Unexpected authorization response: 500"), None),
    })
    assert log_filter.filter(recoverable_500_record) is False

    # Suppressed: ClientNotFoundError (stale cached credentials)
    recoverable_client_not_found_record = logging.makeLogRecord({
        "msg": "OAuth flow error",
        "exc_info": (
            ClientNotFoundError,
            ClientNotFoundError("OAuth client not found - cached credentials may be stale"),
            None,
        ),
    })
    assert log_filter.filter(recoverable_client_not_found_record) is False

    # Not suppressed: different log message
    different_message_record = logging.makeLogRecord({
        "msg": "Different error",
        "exc_info": (RuntimeError, RuntimeError("Unexpected authorization response: 500"), None),
    })
    assert log_filter.filter(different_message_record) is True

    # Not suppressed: unrelated exception
    different_exception_record = logging.makeLogRecord({
        "msg": "OAuth flow error",
        "exc_info": (RuntimeError, RuntimeError("Some other failure"), None),
    })
    assert log_filter.filter(different_exception_record) is True


def test_suppress_recoverable_oauth_traceback_logging_restores_logger_filters(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Test that temporary OAuth traceback suppression is scoped and restored."""
    transport = _get_streamable_http_transport(url="https://example.com/mcp", header_list=None, timeout=30.0)

    class FakeLogger:
        def __init__(self) -> None:
            self.filters: list[logging.Filter] = []

        def addFilter(self, log_filter: logging.Filter) -> None:
            self.filters.append(log_filter)

        def removeFilter(self, log_filter: logging.Filter) -> None:
            self.filters.remove(log_filter)

    loggers = {
        "mcp.client.auth.oauth2": FakeLogger(),
        "fastmcp.client.auth.oauth": FakeLogger(),
    }
    original_get_logger = logging_module.logging.getLogger

    def fake_get_logger(name: str | None = None):
        if name in loggers:
            return loggers[name]
        return original_get_logger(name)

    monkeypatch.setattr(logging_module.logging, "getLogger", fake_get_logger)

    with suppress_recoverable_oauth_traceback_logging(transport):
        assert len(loggers["mcp.client.auth.oauth2"].filters) == 1
        assert len(loggers["fastmcp.client.auth.oauth"].filters) == 1

    assert loggers["mcp.client.auth.oauth2"].filters == []
    assert loggers["fastmcp.client.auth.oauth"].filters == []


async def test_proxy_client_retries_once_after_stale_oauth_error(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test that a narrow OAuth 500 signature clears cached OAuth state and retries once."""
    transport = _get_streamable_http_transport(url="https://example.com/mcp", header_list=None, timeout=30.0)
    assert isinstance(transport.auth, OAuth)

    class FakeAdapter:
        def __init__(self) -> None:
            self.cleared = False

        async def clear(self) -> None:
            self.cleared = True

    adapter = FakeAdapter()
    cast(Any, transport.auth).token_storage_adapter = adapter
    transport.auth._initialized = True

    attempts = 0

    class FakeProxyClient:
        def __init__(self, transport, init_timeout=None) -> None:
            self.transport = transport
            self.init_timeout = init_timeout

        async def __aenter__(self):
            nonlocal attempts
            attempts += 1
            if attempts == 1:
                raise RuntimeError("Client failed to connect: Unexpected authorization response: 500")
            return self

        async def __aexit__(self, exc_type, exc, tb) -> None:
            return None

    monkeypatch.setattr(main_module, "ProxyClient", FakeProxyClient)

    async with _proxy_client(transport) as client:
        assert isinstance(client, FakeProxyClient)

    assert attempts == 2
    assert adapter.cleared is True
    assert transport.auth._initialized is False


async def test_proxy_client_surfaces_helpful_hint_after_retry_failure(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test that a repeated stale OAuth failure suggests clearing cached OAuth state."""
    transport = _get_streamable_http_transport(url="https://example.com/mcp", header_list=None, timeout=30.0)
    assert isinstance(transport.auth, OAuth)

    class FakeAdapter:
        def __init__(self) -> None:
            self.clear_calls = 0

        async def clear(self) -> None:
            self.clear_calls += 1

    adapter = FakeAdapter()
    cast(Any, transport.auth).token_storage_adapter = adapter

    attempts = 0

    class FakeProxyClient:
        def __init__(self, transport, init_timeout=None) -> None:
            self.transport = transport
            self.init_timeout = init_timeout

        async def __aenter__(self):
            nonlocal attempts
            attempts += 1
            raise RuntimeError("Client failed to connect: Unexpected authorization response: 500")

        async def __aexit__(self, exc_type, exc, tb) -> None:
            return None

    monkeypatch.setattr(main_module, "ProxyClient", FakeProxyClient)

    with pytest.raises(RuntimeError, match="mcp-compressor clear-oauth"):
        async with _proxy_client(transport):
            pass

    assert attempts == 2
    assert adapter.clear_calls == 1


async def test_proxy_client_retries_once_after_client_not_found_error(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test that a ClientNotFoundError also clears cached OAuth state and retries once."""
    transport = _get_streamable_http_transport(url="https://example.com/mcp", header_list=None, timeout=30.0)
    assert isinstance(transport.auth, OAuth)

    class FakeAdapter:
        def __init__(self) -> None:
            self.cleared = False

        async def clear(self) -> None:
            self.cleared = True

    adapter = FakeAdapter()
    cast(Any, transport.auth).token_storage_adapter = adapter
    transport.auth._initialized = True

    attempts = 0

    class FakeProxyClient:
        def __init__(self, transport, init_timeout=None) -> None:
            self.transport = transport
            self.init_timeout = init_timeout

        async def __aenter__(self):
            nonlocal attempts
            attempts += 1
            if attempts == 1:
                raise ClientNotFoundError("OAuth client not found - cached credentials may be stale")
            return self

        async def __aexit__(self, exc_type, exc, tb) -> None:
            return None

    monkeypatch.setattr(main_module, "ProxyClient", FakeProxyClient)

    async with _proxy_client(transport) as client:
        assert isinstance(client, FakeProxyClient)

    assert attempts == 2
    assert adapter.cleared is True
    assert transport.auth._initialized is False


async def test_proxy_client_does_not_retry_non_oauth_transports(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test that non-OAuth transports are not retried on the same error signature."""
    transport = _get_stdio_transport(command="python", args=[], cwd=None, env_list=None)

    attempts = 0

    class FakeProxyClient:
        def __init__(self, transport, init_timeout=None) -> None:
            self.transport = transport
            self.init_timeout = init_timeout

        async def __aenter__(self):
            nonlocal attempts
            attempts += 1
            raise RuntimeError("Client failed to connect: Unexpected authorization response: 500")

        async def __aexit__(self, exc_type, exc, tb) -> None:
            return None

    monkeypatch.setattr(main_module, "ProxyClient", FakeProxyClient)

    with pytest.raises(RuntimeError, match="Unexpected authorization response: 500"):
        async with _proxy_client(transport):
            pass

    assert attempts == 1


def test_version_flag(runner: CliRunner) -> None:
    """--version should print the package version and exit with code 0."""
    result = runner.invoke(app, ["--version"])
    assert result.exit_code == 0
    expected_version = importlib.metadata.version("mcp-compressor")
    assert f"mcp-compressor {expected_version}" in result.output


def test_version_short_flag(runner: CliRunner) -> None:
    """-V should be an alias for --version."""
    result = runner.invoke(app, ["-V"])
    assert result.exit_code == 0
    expected_version = importlib.metadata.version("mcp-compressor")
    assert f"mcp-compressor {expected_version}" in result.output
