from __future__ import annotations

import signal
import subprocess
import sys
from importlib.metadata import version


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


def test_python_cli_reports_package_version() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "mcp_compressor.cli", "--version"],
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == f"mcp-compressor {version('mcp-compressor')}"


def test_python_cli_reports_usage_errors() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "mcp_compressor.cli", "--definitely-not-a-real-option"],
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 2
    assert "error:" in result.stderr


def test_python_cli_uses_external_process_for_cli_mode(monkeypatch) -> None:
    calls: list[list[str]] = []

    class FakeProcess:
        def __init__(self, command: list[str]) -> None:
            calls.append(command)

        def wait(self, timeout: float | None = None) -> int:
            return 17

    monkeypatch.setattr("mcp_compressor.cli.subprocess.Popen", FakeProcess)
    monkeypatch.delenv("MCP_COMPRESSOR_EXIT_AFTER_READY", raising=False)

    from mcp_compressor.cli import main

    result = main(["--cli-mode", "--server-name", "alpha", "--", "python", "server.py"])

    assert result == 17
    assert calls == [
        [
            sys.executable,
            "-m",
            "mcp_compressor._native_cli",
            "--cli-mode",
            "--server-name",
            "alpha",
            "--",
            "python",
            "server.py",
        ]
    ]


def test_python_cli_keeps_exit_after_ready_modes_in_process(monkeypatch) -> None:
    def fail_popen(*_args: object, **_kwargs: object) -> None:
        raise AssertionError("exit-after-ready commands should not spawn an external process")

    monkeypatch.setattr("mcp_compressor.cli.subprocess.Popen", fail_popen)
    monkeypatch.setenv("MCP_COMPRESSOR_EXIT_AFTER_READY", "1")

    from mcp_compressor.cli import _needs_long_lived_process

    assert not _needs_long_lived_process(["--cli-mode", "--server-name", "alpha", "--", "python", "server.py"])


def test_python_cli_forwards_keyboard_interrupt_to_external_process(monkeypatch) -> None:
    signals: list[int] = []
    killed = False

    class FakeProcess:
        def wait(self, timeout: float | None = None) -> int:
            if timeout is None:
                raise KeyboardInterrupt
            return 0

        def send_signal(self, sig: int) -> None:
            signals.append(sig)

        def poll(self) -> int | None:
            return None

        def kill(self) -> None:
            nonlocal killed
            killed = True

    monkeypatch.setattr("mcp_compressor.cli.subprocess.Popen", lambda *_args, **_kwargs: FakeProcess())
    monkeypatch.delenv("MCP_COMPRESSOR_EXIT_AFTER_READY", raising=False)

    from mcp_compressor.cli import main

    assert main(["--cli-mode", "--server-name", "alpha", "--", "python", "server.py"]) == 0
    assert signals == [signal.SIGINT]
    assert not killed
