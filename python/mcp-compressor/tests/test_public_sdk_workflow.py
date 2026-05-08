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

        response = proxy.invoke("echo", {"message": "public-python"})
        assert response == "alpha:public-python"
