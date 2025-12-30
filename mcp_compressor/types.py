"""Type definitions for MCP Compressor.

This module defines enumerations and type aliases used throughout the MCP Compressor package.
"""

from enum import Enum
from typing import TypeAlias

from fastmcp.client.transports import SSETransport, StdioTransport, StreamableHttpTransport
from fastmcp.utilities.types import Audio, File, Image


class LogLevel(str, Enum):
    """Logging levels for the MCP Compressor server and wrapped MCP servers."""

    DEBUG = "DEBUG"
    INFO = "INFO"
    WARNING = "WARNING"
    ERROR = "ERROR"
    CRITICAL = "CRITICAL"


class CompressionLevel(str, Enum):
    """Compression levels for tool descriptions in the wrapped MCP server.

    Higher compression levels provide less verbose tool descriptions, reducing token usage.
    Lower compression levels provide more detailed information upfront.

    Attributes:
        MAX: Maximum compression - exposes a list_tools function for viewing all tools.
        HIGH: High compression - only tool names and parameter names, no descriptions.
        MEDIUM: Medium compression - first sentence of tool descriptions only.
        LOW: Low compression - complete tool descriptions and schemas.
    """

    MAX = "max"
    HIGH = "high"
    MEDIUM = "medium"
    LOW = "low"


TransportType: TypeAlias = SSETransport | StdioTransport | StreamableHttpTransport
"""Type alias for supported MCP transport types (SSE, stdio, or streamable HTTP)."""

ToolResultBlock: TypeAlias = str | Audio | File | Image
"""Type alias for possible tool result content blocks (text, audio, file, or image)."""
