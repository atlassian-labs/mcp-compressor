from collections.abc import AsyncGenerator
from pathlib import Path

import pytest
from fastmcp.client import Client

from mcp_compressor.main import _server
from mcp_compressor.types import CompressionLevel


@pytest.fixture
async def proxy_mcp_client(request) -> AsyncGenerator[Client, None]:
    """Fixture that provides a FastMCP client connected to the MCP compressor server."""
    if hasattr(request, "param"):
        compression_level: CompressionLevel = request.param or CompressionLevel.LOW
    else:
        compression_level = CompressionLevel.LOW
    server_path = Path(__file__).parent / "mcp_server.py"
    async with (
        _server(
            command_or_url_list=["python", str(server_path)],
            cwd=None,
            env_list=None,
            header_list=None,
            timeout=10.0,
            compression_level=compression_level,
            server_name="test_server",
        ) as mcp,
        Client(mcp) as client,
    ):
        yield client


@pytest.fixture
async def proxy_mcp_client_toonify() -> AsyncGenerator[Client, None]:
    """Fixture that provides a FastMCP client connected to a toonified MCP compressor server."""
    server_path = Path(__file__).parent / "mcp_server.py"
    async with (
        _server(
            command_or_url_list=["python", str(server_path)],
            cwd=None,
            env_list=None,
            header_list=None,
            timeout=10.0,
            compression_level=CompressionLevel.LOW,
            server_name="test_server",
            toonify=True,
        ) as mcp,
        Client(mcp) as client,
    ):
        yield client


@pytest.fixture
async def backend_mcp_client() -> AsyncGenerator[Client, None]:
    """Fixture that provides a FastMCP client connected directly to the backend MCP server."""
    server_path = Path(__file__).parent / "mcp_server.py"
    async with Client(server_path) as client:
        yield client
