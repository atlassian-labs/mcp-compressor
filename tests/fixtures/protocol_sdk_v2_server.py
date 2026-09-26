"""An actual SDK 2.2.0 backend, independently negotiated by the compressor."""

from mcp.server import MCPServer  # ty: ignore[unresolved-import] -- isolated SDK 2.2.0 fixture

server = MCPServer("SDK 2.2.0 fixture")


@server.tool()
def echo(message: str) -> str:
    return f"sdk2:{message}"


if __name__ == "__main__":
    server.run()
