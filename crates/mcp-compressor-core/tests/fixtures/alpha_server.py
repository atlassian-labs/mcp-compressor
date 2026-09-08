from __future__ import annotations

import contextlib
from typing import Any

from fastmcp import Context, FastMCP
from fastmcp.exceptions import ToolError
from fastmcp.tools.tool import ToolResult
from mcp.types import Annotations, AudioContent, ImageContent, TextContent

mcp = FastMCP("Rust Core Alpha Fixture")


@mcp.tool
def echo(message: str) -> str:
    """Echo a message from alpha."""
    return f"alpha:{message}"


@mcp.tool
def add(a: int, b: int) -> int:
    """Add two integers on alpha."""
    return a + b


@mcp.tool
def structured_data() -> dict[str, Any]:
    """Return structured alpha data."""
    return {"server": "alpha", "values": [1, 2], "nested": {"ok": True}}


@mcp.tool
def summarize_payload(items: list[str], metadata: dict[str, Any]) -> dict[str, Any]:
    """Summarize structured list and object arguments."""
    return {
        "item_count": len(items),
        "items": items,
        "metadata": metadata,
        "metadata_keys": sorted(metadata.keys()),
    }


@mcp.tool
def json_rows() -> ToolResult:
    """Return a JSON table in an annotated text block for toonify contract tests."""
    return ToolResult(
        content=[
            TextContent(
                type="text",
                text='[{"id": 1, "name": "alpha"}, {"id": 2, "name": "beta"}]',
                annotations=Annotations(audience=["assistant"], priority=0.75),
            )
        ]
    )


@mcp.tool
def rich_result(ctx: Context) -> ToolResult:
    """Return every supported tool-result field for contract tests."""
    request_meta: dict[str, Any] = {}
    if ctx.request_context is not None and ctx.request_context.meta is not None:
        request_meta = ctx.request_context.meta.model_dump(by_alias=True, exclude_none=True)

    return ToolResult(
        content=[
            TextContent(
                type="text",
                text="rich text",
                annotations=Annotations(audience=["assistant"], priority=0.75),
                _meta={"content": "text"},
            ),
            ImageContent(
                type="image",
                data="aW1hZ2U=",
                mimeType="image/png",
                annotations=Annotations(audience=["user"], priority=0.5),
                _meta={"content": "image"},
            ),
            AudioContent(
                type="audio",
                data="YXVkaW8=",
                mimeType="audio/wav",
                annotations=Annotations(priority=0.25),
            ),
        ],
        structured_content={"ok": True, "values": [1, 2]},
        meta={"result": "rich", "request": request_meta},
    )


@mcp.tool
def tool_error() -> None:
    """Return an MCP tool error result."""
    raise ToolError("fixture tool error")


@mcp.resource("fixture://alpha-resource")
def alpha_resource() -> str:
    """Return a static alpha resource."""
    return "alpha resource"


@mcp.prompt
def alpha_prompt() -> str:
    """Return a static alpha prompt."""
    return "alpha prompt"


if __name__ == "__main__":
    with contextlib.suppress(KeyboardInterrupt):
        mcp.run(show_banner=False)
