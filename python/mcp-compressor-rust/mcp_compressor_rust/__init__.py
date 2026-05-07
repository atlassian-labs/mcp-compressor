"""Rust-backed Python API for mcp-compressor."""

from mcp_compressor_rust.client import (
    CompressorClient,
    CompressorProxy,
    JustBashCommand,
    JustBashProvider,
    ProxyResponse,
    ProxyTool,
    normalize_servers,
)
from mcp_compressor_rust.core import (
    BackendConfig,
    CompressedSession,
    CompressedSessionConfig,
    ToolSpec,
    clear_oauth_credentials,
    compress_tool_listing,
    format_tool_schema_response,
    list_oauth_credentials,
    parse_mcp_config,
    parse_tool_argv,
    start_compressed_session,
    start_compressed_session_from_mcp_config,
)

__all__ = [
    "BackendConfig",
    "CompressedSession",
    "CompressedSessionConfig",
    "CompressorClient",
    "CompressorProxy",
    "JustBashCommand",
    "JustBashProvider",
    "ProxyResponse",
    "ProxyTool",
    "ToolSpec",
    "clear_oauth_credentials",
    "compress_tool_listing",
    "format_tool_schema_response",
    "list_oauth_credentials",
    "normalize_servers",
    "parse_mcp_config",
    "parse_tool_argv",
    "start_compressed_session",
    "start_compressed_session_from_mcp_config",
]
