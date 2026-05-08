from __future__ import annotations

import os
import sys
from pathlib import Path

from mcp_compressor import CompressorClient

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "alpha_server.py"
PYTHON = os.environ.get("PYTHON", sys.executable)


def test_public_python_sdk_quickstart_flow() -> None:
    with CompressorClient(
        servers={"alpha": {"command": PYTHON, "args": [str(FIXTURE)]}},
        compression_level="max",
    ) as proxy:
        tool_names = {tool.name for tool in proxy.tools}
        assert "alpha_get_tool_schema" in tool_names
        assert "alpha_invoke_tool" in tool_names

        schema = proxy.schema("echo")
        assert "message" in schema["properties"]

        response = proxy.invoke("echo", {"message": "public-python"})
        assert response == "alpha:public-python"


def test_public_python_sdk_schema_lookup_supports_multi_server() -> None:
    with CompressorClient(
        servers={
            "alpha": {"command": PYTHON, "args": [str(FIXTURE)]},
            "beta": {
                "command": PYTHON,
                "args": [str(ROOT / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "beta_server.py")],
            },
        },
        compression_level="max",
    ) as proxy:
        schema = proxy.schema("echo", server="alpha")
        assert "message" in schema["properties"]

        try:
            proxy.schema("echo")
        except RuntimeError as error:
            assert "Multiple backend tools" in str(error)
        else:  # pragma: no cover - defensive assertion
            raise AssertionError("expected ambiguous schema lookup to fail")
