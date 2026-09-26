"""Strict initialize-only backend with no modern wire fields."""

from __future__ import annotations

import json
import sys


def send(message: dict) -> None:
    print(json.dumps({"jsonrpc": "2.0", **message}), flush=True)


def result(request_id: object, value: dict) -> None:
    send({"id": request_id, "result": value})


def text(request_id: object, value: str, *, error: bool = False) -> None:
    result(request_id, {"content": [{"type": "text", "text": value}], "isError": error})


def main() -> None:  # noqa: C901 - explicit JSON-RPC fixture dispatch
    initialized = False
    for line in sys.stdin:
        request = json.loads(line)
        method = request.get("method")
        request_id = request.get("id")
        params = request.get("params", {})
        if not initialized:
            if method != "initialize":
                raise RuntimeError("legacy backend requires initialize as its first request")
            initialized = True
            result(
                request_id,
                {
                    "protocolVersion": "2025-11-25",
                    "serverInfo": {"name": "strict-legacy", "version": "1"},
                    "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                },
            )
            continue
        if "id" not in request:
            continue
        if method == "tools/list":
            names = ["echo", "records", "fail", "rpc_fail", "metadata", "envelope"]
            tools = [{"name": name, "description": name, "inputSchema": {"type": "object"}} for name in names]
            tools[0]["inputSchema"] = {
                "type": "object",
                "properties": {"message": {"type": "string"}},
                "required": ["message"],
            }
            result(request_id, {"tools": tools})
        elif method == "tools/call":
            meta = params.get("_meta", {})
            if any(
                key in meta
                for key in (
                    "io.modelcontextprotocol/protocolVersion",
                    "io.modelcontextprotocol/clientInfo",
                    "io.modelcontextprotocol/clientCapabilities",
                )
            ):
                raise RuntimeError("frontend negotiation metadata leaked to legacy backend")
            name = params["name"]
            if name == "echo":
                text(request_id, f"echo:{params['arguments']['message']}")
            elif name == "records":
                text(request_id, json.dumps({"items": [{"id": 1, "name": "one"}, {"id": 2, "name": "two"}]}))
            elif name == "fail":
                text(request_id, "fixture tool failure", error=True)
            elif name == "rpc_fail":
                send({"id": request_id, "error": {"code": -32602, "message": "fixture protocol failure"}})
            elif name == "metadata":
                text(request_id, json.dumps(meta))
            elif name == "envelope":
                result(
                    request_id,
                    {
                        "content": [
                            {
                                "type": "text",
                                "text": '{"value":42}',
                                "annotations": {"audience": ["user"]},
                                "_meta": {"fixture/block": True},
                            }
                        ],
                        "structuredContent": {"value": 42},
                        "isError": False,
                        "_meta": {"fixture/result": True},
                    },
                )
            else:
                send({"id": request_id, "error": {"code": -32602, "message": "unknown tool"}})
        elif method == "resources/list":
            result(request_id, {"resources": [{"uri": "fixture://resource", "name": "fixture"}]})
        elif method == "resources/read":
            result(request_id, {"contents": [{"uri": params["uri"], "text": "fixture resource"}]})
        elif method == "prompts/list":
            result(request_id, {"prompts": [{"name": "fixture_prompt"}]})
        elif method == "prompts/get":
            result(request_id, {"messages": [{"role": "user", "content": {"type": "text", "text": "fixture prompt"}}]})
        else:
            send({"id": request_id, "error": {"code": -32601, "message": "Method not found"}})


if __name__ == "__main__":
    main()
