"""Rust-backed Python API for mcp-compressor."""

from mcp_compressor.adapters import to_ai_sdk_tools, to_mastra_tools
from mcp_compressor.client import (
    CompressorClient,
    CompressorProxy,
    ExecutableTool,
    GeneratedCodeClient,
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
    "ExecutableTool",
    "GeneratedCodeClient",
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
    "to_ai_sdk_tools",
    "to_mastra_tools",
]
