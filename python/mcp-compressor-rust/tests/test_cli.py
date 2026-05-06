from __future__ import annotations

import os
from pathlib import Path

from mcp_compressor_rust.cli import main


def test_python_cli_delegates_to_rust_core_binary(tmp_path: Path, monkeypatch) -> None:
    log = tmp_path / "argv.txt"
    binary = tmp_path / "mcp-compressor-core"
    binary.write_text('#!/bin/sh\nprintf \'%s\\n\' "$@" > "$MCP_COMPRESSOR_TEST_LOG"\nexit 7\n')
    binary.chmod(0o755)
    monkeypatch.setenv("MCP_COMPRESSOR_CORE_BINARY", str(binary))
    monkeypatch.setenv("MCP_COMPRESSOR_TEST_LOG", str(log))

    assert main(["--version"]) == 7
    assert log.read_text().strip() == "--version"


def test_python_cli_reports_missing_rust_core_binary(monkeypatch) -> None:
    monkeypatch.setenv("MCP_COMPRESSOR_CORE_BINARY", os.devnull + "-missing")
    monkeypatch.setenv("PATH", "")
    assert main(["--help"]) == 127
