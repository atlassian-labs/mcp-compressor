from __future__ import annotations

import os
import signal
from pathlib import Path

from mcp_compressor.cli import main


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


def test_python_cli_skips_self_resolving_console_script(tmp_path: Path, monkeypatch) -> None:
    script = tmp_path / "mcp-compressor"
    script.write_text("#!/bin/sh\nexec mcp-compressor \"$@\"\n")
    script.chmod(0o755)
    commands: list[list[str]] = []

    def popen(command: list[str]) -> None:
        commands.append(command)
        raise FileNotFoundError(command[0])

    monkeypatch.delenv("MCP_COMPRESSOR_BINARY", raising=False)
    monkeypatch.delenv("MCP_COMPRESSOR_CORE_BINARY", raising=False)
    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.setattr("sys.argv", [str(script)])
    monkeypatch.setattr("subprocess.Popen", popen)

    assert main(["--help"]) == 127
    assert [str(script), "--help"] not in commands
    assert ["mcp-compressor", "--help"] not in commands


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
