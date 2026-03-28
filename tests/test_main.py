"""Tests for mcp_compressor/main.py."""

from typing import Any, cast

import pytest
from fastmcp.client.auth.oauth import OAuth
from fastmcp.client.transports import SSETransport, StdioTransport, StreamableHttpTransport

import mcp_compressor.main as main_module
from mcp_compressor.main import (
    _get_sse_transport,
    _get_stdio_transport,
    _get_streamable_http_transport,
    _interpolate_string,
    _parse_tool_name_list,
    _proxy_client,
    _server,
)
from mcp_compressor.types import CompressionLevel


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


# Tests for transport creation functions


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
    import pytest
    from fastmcp.exceptions import McpError

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
                raise RuntimeError("Client failed to connect: Unexpected authorization response: 500")  # noqa: TRY003
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
            raise RuntimeError("Client failed to connect: Unexpected authorization response: 500")  # noqa: TRY003

        async def __aexit__(self, exc_type, exc, tb) -> None:
            return None

    monkeypatch.setattr(main_module, "ProxyClient", FakeProxyClient)

    with pytest.raises(RuntimeError, match="mcp-compressor clear-oauth"):
        async with _proxy_client(transport):
            pass

    assert attempts == 2
    assert adapter.clear_calls == 1


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
            raise RuntimeError("Client failed to connect: Unexpected authorization response: 500")  # noqa: TRY003

        async def __aexit__(self, exc_type, exc, tb) -> None:
            return None

    monkeypatch.setattr(main_module, "ProxyClient", FakeProxyClient)

    with pytest.raises(RuntimeError, match="Unexpected authorization response: 500"):
        async with _proxy_client(transport):
            pass

    assert attempts == 1
