"""
Test that mcp-compressor handles pre-initialize custom JSON-RPC requests correctly.

This test simulates clients like Antigravity CLI that send custom requests
like "server/discover" before sending the "initialize" request.
"""
from __future__ import annotations

import asyncio
import json
import subprocess
from pathlib import Path


def rust_core_command(*args: str) -> list[str]:
    return [
        "cargo",
        "run",
        "-q",
        "-p",
        "mcp-compressor-core",
        "--bin",
        "mcp-compressor",
        "--",
        *args,
    ]


async def test_pre_initialize_custom_request():
    """
    Test that mcp-compressor doesn't crash when receiving custom requests before initialize.
    
    This reproduces the issue where clients like Antigravity CLI send "server/discover"
    before "initialize", causing mcp-compressor to reject the request and terminate.
    """
    root = Path(__file__).parents[1]
    alpha = root / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "alpha_server.py"
    command = rust_core_command(
        "--compression",
        "max",
        "--server-name",
        "alpha",
        "--",
        "python3",
        str(alpha),
    )
    
    # Start the mcp-compressor process
    proc = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    
    try:
        # Send a custom "server/discover" request BEFORE initialize
        # This simulates what Antigravity CLI does
        discover_request = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "server/discover",
            "params": {}
        }
        
        proc.stdin.write(json.dumps(discover_request) + "\n")
        proc.stdin.flush()
        
        # Wait a bit for response
        await asyncio.sleep(0.5)
        
        # Now send the initialize request
        initialize_request = {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            }
        }
        
        proc.stdin.write(json.dumps(initialize_request) + "\n")
        proc.stdin.flush()
        
        # Wait a bit for response
        await asyncio.sleep(0.5)
        
        # Check if process is still alive
        # If the bug exists, the process will have terminated
        return_code = proc.poll()
        
        if return_code is not None:
            # Process terminated - this is the bug
            stderr = proc.stderr.read()
            raise AssertionError(
                f"mcp-compressor terminated unexpectedly after custom pre-initialize request. "
                f"Exit code: {return_code}, stderr: {stderr}"
            )
        
        # If we get here, the process is still alive - good!
        # Send initialized notification
        initialized_notification = {
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }
        proc.stdin.write(json.dumps(initialized_notification) + "\n")
        proc.stdin.flush()
        
        # Try to list tools to verify everything works
        list_tools_request = {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/list",
            "params": {}
        }
        
        proc.stdin.write(json.dumps(list_tools_request) + "\n")
        proc.stdin.flush()
        
        # Wait for response
        await asyncio.sleep(0.5)
        
        # Process should still be alive
        return_code = proc.poll()
        assert return_code is None, f"Process terminated unexpectedly with code {return_code}"
        
    finally:
        # Clean up
        proc.terminate()
        try:
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


if __name__ == "__main__":
    asyncio.run(test_pre_initialize_custom_request())
