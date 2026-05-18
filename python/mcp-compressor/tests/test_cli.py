from __future__ import annotations

import subprocess
import sys


def test_python_cli_runs_packaged_native_help() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "mcp_compressor.cli", "--help"],
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0
    assert "Usage:" in result.stdout
    assert "mcp-compressor" in result.stdout


def test_python_cli_reports_usage_errors() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "mcp_compressor.cli", "--definitely-not-a-real-option"],
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 2
    assert "error:" in result.stderr
