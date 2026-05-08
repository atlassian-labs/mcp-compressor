"""Rust-backed Python API for mcp-compressor."""

from mcp_compressor.client import (
    CompressorClient,
    CompressorProxy,
    JustBashCommand,
    JustBashProvider,
    ProxyResponse,
    ProxyTool,
    normalize_servers,
)
from mcp_compressor.core import (
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
from mcp_compressor.just_bash_host import (
    JustBashCallableCommand,
    create_just_bash_commands,
    install_just_bash_commands,
)

__all__ = [
    "BackendConfig",
    "CompressedSession",
    "CompressedSessionConfig",
    "CompressorClient",
    "CompressorProxy",
    "JustBashCallableCommand",
    "JustBashCommand",
    "JustBashProvider",
    "ProxyResponse",
    "ProxyTool",
    "ToolSpec",
    "clear_oauth_credentials",
    "compress_tool_listing",
    "create_just_bash_commands",
    "format_tool_schema_response",
    "install_just_bash_commands",
    "list_oauth_credentials",
    "normalize_servers",
    "parse_mcp_config",
    "parse_tool_argv",
    "start_compressed_session",
    "start_compressed_session_from_mcp_config",
]
