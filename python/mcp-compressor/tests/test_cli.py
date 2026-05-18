from __future__ import annotations

import os
import signal
from pathlib import Path

from mcp_compressor.cli import _candidate_binaries, _is_native_executable, main


class InterruptingProcess:
    def __init__(self, command: list[str]) -> None:
        self.command = command
        self.terminated = False
        self.killed = False
        self.wait_calls = 0

    def wait(self, timeout: float | None = None) -> int:
        self.wait_calls += 1
        if timeout is None and self.wait_calls == 1:
            raise KeyboardInterrupt
        return -signal.SIGTERM

    def terminate(self) -> None:
        self.terminated = True

    def kill(self) -> None:
        self.killed = True


class StubbornInterruptingProcess(InterruptingProcess):
    def wait(self, timeout: float | None = None) -> int:
        self.wait_calls += 1
        if timeout is None and self.wait_calls == 1:
            raise KeyboardInterrupt
        if timeout is not None:
            import subprocess

            raise subprocess.TimeoutExpired(self.command, timeout)
        return -signal.SIGKILL


def test_python_cli_delegates_to_rust_core_binary(tmp_path: Path, monkeypatch) -> None:
    log = tmp_path / "argv.txt"
    binary = tmp_path / "mcp-compressor"
    binary.write_text('#!/bin/sh\nprintf \'%s\\n\' "$@" > "$MCP_COMPRESSOR_TEST_LOG"\nexit 7\n')
    binary.chmod(0o755)
    monkeypatch.setenv("MCP_COMPRESSOR_BINARY", str(binary))
    monkeypatch.setenv("MCP_COMPRESSOR_TEST_LOG", str(log))

    assert main(["--version"]) == 7
    assert log.read_text().strip() == "--version"


def test_python_cli_reports_missing_rust_core_binary(monkeypatch) -> None:
    monkeypatch.setenv("MCP_COMPRESSOR_BINARY", os.devnull + "-missing")
    monkeypatch.setenv("PATH", "")
    assert main(["--help"]) == 127


def test_python_cli_ctrl_c_terminates_child_without_traceback(monkeypatch) -> None:
    process = InterruptingProcess(["mcp-compressor"])

    def popen(command: list[str]) -> InterruptingProcess:
        process.command = command
        return process

    monkeypatch.setenv("MCP_COMPRESSOR_BINARY", "mcp-compressor")
    monkeypatch.setattr("subprocess.Popen", popen)

    assert main(["--cli-mode"]) == 128 + signal.SIGINT
    assert process.terminated
    assert not process.killed


def test_python_cli_ctrl_c_kills_stubborn_child(monkeypatch) -> None:
    process = StubbornInterruptingProcess(["mcp-compressor"])

    def popen(command: list[str]) -> StubbornInterruptingProcess:
        process.command = command
        return process

    monkeypatch.setenv("MCP_COMPRESSOR_BINARY", "mcp-compressor")
    monkeypatch.setattr("subprocess.Popen", popen)

    assert main(["--cli-mode"]) == 128 + signal.SIGINT
    assert process.terminated
    assert process.killed


# ---------------------------------------------------------------------------
# Fork-bomb prevention tests
# ---------------------------------------------------------------------------


def test_is_native_executable_detects_elf(tmp_path: Path) -> None:
    """A file starting with the ELF magic bytes is recognised as native."""
    binary = tmp_path / "mcp-compressor"
    binary.write_bytes(b"\x7fELF" + b"\x00" * 60)
    binary.chmod(0o755)
    assert _is_native_executable(str(binary))


def test_is_native_executable_detects_macho(tmp_path: Path) -> None:
    """A file starting with a Mach-O magic is recognised as native."""
    binary = tmp_path / "mcp-compressor"
    binary.write_bytes(b"\xcf\xfa\xed\xfe" + b"\x00" * 60)  # Mach-O 64-bit LE
    binary.chmod(0o755)
    assert _is_native_executable(str(binary))


def test_is_native_executable_detects_pe(tmp_path: Path) -> None:
    """A file starting with MZ (Windows PE stub) is recognised as native."""
    binary = tmp_path / "mcp-compressor.exe"
    binary.write_bytes(b"MZ\x90\x00" + b"\x00" * 60)
    binary.chmod(0o755)
    assert _is_native_executable(str(binary))


def test_is_native_executable_rejects_python_shim(tmp_path: Path) -> None:
    """A pip/uvx Python wrapper script is NOT a native executable."""
    shim = tmp_path / "mcp-compressor"
    shim.write_text("#!/usr/bin/env python\nimport sys; from mcp_compressor.cli import entrypoint; entrypoint()\n")
    shim.chmod(0o755)
    assert not _is_native_executable(str(shim))


def test_is_native_executable_rejects_shell_script(tmp_path: Path) -> None:
    """A plain shell script is NOT a native executable."""
    shim = tmp_path / "mcp-compressor"
    shim.write_text("#!/bin/sh\nexec python -m mcp_compressor \"$@\"\n")
    shim.chmod(0o755)
    assert not _is_native_executable(str(shim))


def test_candidate_binaries_excludes_python_shim_on_path(tmp_path: Path, monkeypatch) -> None:
    """When the only 'mcp-compressor' on PATH is a Python shim, it is skipped.

    This is the core fork-bomb prevention check: the pip/uvx installed shim
    must never appear in _candidate_binaries() or the shim would spawn itself
    recursively, consuming all available memory.
    """
    shim = tmp_path / "mcp-compressor"
    shim.write_text("#!/usr/bin/env python\nfrom mcp_compressor.cli import entrypoint; entrypoint()\n")
    shim.chmod(0o755)

    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.delenv("MCP_COMPRESSOR_BINARY", raising=False)
    monkeypatch.delenv("MCP_COMPRESSOR_CORE_BINARY", raising=False)

    candidates = _candidate_binaries()
    assert str(shim) not in candidates


def test_candidate_binaries_includes_native_binary_on_path(tmp_path: Path, monkeypatch) -> None:
    """When 'mcp-compressor' on PATH is a native ELF binary, it IS included."""
    binary = tmp_path / "mcp-compressor"
    binary.write_bytes(b"\x7fELF" + b"\x00" * 60)
    binary.chmod(0o755)

    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.delenv("MCP_COMPRESSOR_BINARY", raising=False)
    monkeypatch.delenv("MCP_COMPRESSOR_CORE_BINARY", raising=False)

    candidates = _candidate_binaries()
    assert str(binary) in candidates
