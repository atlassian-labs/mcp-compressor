"""Console entrypoint for the Python package."""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import threading
from importlib.metadata import PackageNotFoundError, version

from mcp_compressor import _native


def main(argv: list[str] | None = None) -> int:
    """Run the packaged mcp-compressor CLI.

    The Python package ships the native extension, so the console script should
    not require a separately installed or locally built Rust binary.
    """
    args = sys.argv[1:] if argv is None else argv
    if args in (["--version"], ["-V"]):
        print(f"mcp-compressor {_package_version()}")
        return 0
    if _needs_long_lived_process(args):
        return _run_external_process(args)
    return int(_native.run_cli_json(json.dumps(["mcp-compressor", *args])))


def _needs_long_lived_process(args: list[str]) -> bool:
    if os.environ.get("MCP_COMPRESSOR_FORCE_NATIVE_CLI") == "1":
        return False
    return bool(_native.cli_needs_external_process_json(json.dumps(["mcp-compressor", *args])))


def _run_external_process(args: list[str]) -> int:
    child = subprocess.Popen(  # noqa: S603
        [sys.executable, "-m", "mcp_compressor._native_cli", *args]
    )
    previous_sigint = signal.getsignal(signal.SIGINT)

    def forward_sigint(_signum: int, _frame: object) -> None:
        child.send_signal(signal.SIGINT)

    if threading.current_thread() is threading.main_thread():
        signal.signal(signal.SIGINT, forward_sigint)
    try:
        return child.wait()
    except KeyboardInterrupt:
        child.send_signal(signal.SIGINT)
        try:
            return child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
            return 130
    finally:
        if threading.current_thread() is threading.main_thread():
            signal.signal(signal.SIGINT, previous_sigint)


def _package_version() -> str:
    try:
        return version("mcp-compressor")
    except PackageNotFoundError:
        return "0.0.0+editable"


def entrypoint() -> None:
    raise SystemExit(main())


if __name__ == "__main__":
    entrypoint()
