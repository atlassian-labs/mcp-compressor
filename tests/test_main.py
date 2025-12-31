"""Tests for mcp_compressor/main.py."""

import pytest
from fastmcp.client.transports import SSETransport, StdioTransport, StreamableHttpTransport

from mcp_compressor.main import (
    _get_sse_transport,
    _get_stdio_transport,
    _get_streamable_http_transport,
    _interpolate_string,
)


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


def test_get_streamable_http_transport() -> None:
    """Test that HTTP transport is created with correct parameters."""
    transport = _get_streamable_http_transport(
        url="https://example.com/mcp",
        header_list=["Authorization=Bearer token"],
        timeout=30.0,
    )
    assert isinstance(transport, StreamableHttpTransport)


def test_get_sse_transport() -> None:
    """Test that SSE transport is created with correct parameters."""
    transport = _get_sse_transport(
        url="https://example.com/sse",
        header_list=["X-Custom=value"],
        timeout=15.0,
    )
    assert isinstance(transport, SSETransport)
