import contextlib

from fastmcp import FastMCP

mcp = FastMCP("Beta E2E Server")


@mcp.tool
def beta_echo(message: str) -> str:
    """Echo a message from beta."""
    return f"beta:{message}"


@mcp.tool
def beta_multiply(a: int, b: int) -> int:
    """Multiply two integers on beta."""
    return a * b


@mcp.tool
def beta_object() -> dict[str, object]:
    """Return structured beta data."""
    return {"server": "beta", "values": [3, 4]}


@mcp.resource("e2e://beta-resource")
def beta_resource() -> str:
    return "beta resource"


@mcp.prompt
def beta_prompt() -> str:
    return "beta prompt"


if __name__ == "__main__":
    with contextlib.suppress(KeyboardInterrupt):
        mcp.run(show_banner=False)
