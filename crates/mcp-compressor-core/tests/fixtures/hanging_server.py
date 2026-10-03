from __future__ import annotations

import asyncio
import contextlib
import os
import time
from typing import Any

mode = os.environ.get("HANG_OPERATION")
time.sleep(float(os.environ.get("STARTUP_DELAY", "0")))


def ready(operation: str) -> None:
    if ready_file := os.environ.get("READY_FILE"):
        with open(ready_file, "w", encoding="utf-8") as handle:
            handle.write(operation)


if pid_file := os.environ.get("PID_FILE"):
    with open(pid_file, "w", encoding="utf-8") as handle:
        handle.write(str(os.getpid()))

if mode == "connection":
    ready("connection")
    while True:
        time.sleep(60)

from fastmcp import FastMCP  # noqa: E402
from fastmcp.server.middleware import Middleware, MiddlewareContext  # noqa: E402

mcp = FastMCP("Rust Core Hanging Fixture")


class HangingDiscoveryMiddleware(Middleware):
    async def on_list_tools(
        self,
        context: MiddlewareContext[Any],
        call_next: Any,
    ) -> Any:
        if mode == "discovery":
            ready("discovery")
            await asyncio.Event().wait()
        return await call_next(context)

    async def on_list_resources(
        self,
        context: MiddlewareContext[Any],
        call_next: Any,
    ) -> Any:
        if mode == "resource-discovery":
            ready("resource-discovery")
            await asyncio.Event().wait()
        return await call_next(context)

    async def on_list_prompts(
        self,
        context: MiddlewareContext[Any],
        call_next: Any,
    ) -> Any:
        if mode == "prompt-discovery":
            ready("prompt-discovery")
            await asyncio.Event().wait()
        return await call_next(context)


mcp.add_middleware(HangingDiscoveryMiddleware())


@mcp.tool
def fast() -> str:
    """Return without blocking."""
    return "fast"


@mcp.tool
async def hang() -> str:
    """Wait forever so clients can enforce their request deadline."""
    ready("tools/call")
    await asyncio.Event().wait()
    raise AssertionError("unreachable")


@mcp.resource("fixture://hanging-resource")
async def hanging_resource() -> str:
    """Wait forever so clients can enforce their resource deadline."""
    ready("resources/read")
    await asyncio.Event().wait()
    raise AssertionError("unreachable")


@mcp.prompt
async def hanging_prompt() -> str:
    """Wait forever so clients can enforce their prompt deadline."""
    ready("prompts/get")
    await asyncio.Event().wait()
    raise AssertionError("unreachable")


if __name__ == "__main__":
    with contextlib.suppress(KeyboardInterrupt):
        mcp.run(show_banner=False)
