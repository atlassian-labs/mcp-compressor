from fastmcp import FastMCP

mcp = FastMCP("Test MCP Server")


@mcp.tool
def do_nothing(arg: str) -> str:
    """A test tool that does nothing.

    Second description line.

    Args:
        arg: A string argument.

    Returns:
        The same string argument.
    """
    return arg


@mcp.tool
def add(a: int, b: int) -> int:
    """A test tool that adds two numbers together.

    Args:
        a: An integer number.
        b: Another integer number.

    Returns:
        The sum of the two integer numbers.
    """
    return a + b


@mcp.tool
def throw_error(message: str) -> None:
    """A test tool that throws an error.

    Args:
        message: The error message to throw.

    Raises:
        ValueError: Always raised with the provided message.
    """
    raise ValueError(message)


@mcp.tool
def empty_tool() -> None:
    """A test tool that does nothing and has no arguments or return value."""
    pass


@mcp.resource("test://test-resource")
def test_resource() -> str:
    """A test resource that returns a static string.

    Returns:
        A static string indicating the resource was accessed.
    """
    return "Test resource accessed."


@mcp.prompt
def test_prompt() -> str:
    """A test prompt that returns a static string.

    Returns:
        A static string indicating the prompt was accessed.
    """
    return "Test prompt accessed."


if __name__ == "__main__":
    mcp.run()
