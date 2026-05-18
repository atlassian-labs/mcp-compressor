"""Rust-backed mcp-compressor CLI entrypoint for the Python package."""

from __future__ import annotations

import os
import signal
import shutil
import subprocess
import sys
from pathlib import Path


def _resolved_executable(command: str) -> Path | None:
    resolved = shutil.which(command)
    if resolved is None:
        return None
    return Path(resolved).resolve()


def _is_current_entrypoint(command: str) -> bool:
    resolved_command = _resolved_executable(command)
    resolved_entrypoint = _resolved_executable(sys.argv[0])
    return resolved_command is not None and resolved_command == resolved_entrypoint


def _candidate_binaries() -> list[str]:
    env_binary = os.environ.get("MCP_COMPRESSOR_BINARY") or os.environ.get("MCP_COMPRESSOR_CORE_BINARY")
    candidates: list[str] = []
    if env_binary:
        return [] if _is_current_entrypoint(env_binary) else [env_binary]
    if not _is_current_entrypoint("mcp-compressor"):
        candidates.append("mcp-compressor")
    repo_binary = Path(__file__).resolve().parents[3] / "target" / "debug" / "mcp-compressor"
    candidates.append(str(repo_binary))
    return candidates


def _run_child(command: list[str]) -> int:
    child = subprocess.Popen(command)  # noqa: S603 - controlled CLI delegation
    try:
        return int(child.wait())
    except KeyboardInterrupt:
        child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
        return 128 + signal.SIGINT


def main(argv: list[str] | None = None) -> int:
    """Run the Rust core CLI, preserving stdio and process semantics."""
    args = sys.argv[1:] if argv is None else argv
    last_error: OSError | None = None
    for binary in _candidate_binaries():
        try:
            return _run_child([binary, *args])
        except FileNotFoundError as exc:
            last_error = exc
            continue
    print(
        "mcp-compressor binary was not found. Build it with `cargo build -p mcp-compressor-core` "
        "or set MCP_COMPRESSOR_BINARY.",
        file=sys.stderr,
    )
    if last_error is not None:
        print(str(last_error), file=sys.stderr)
    return 127


def entrypoint() -> None:
    raise SystemExit(main())


if __name__ == "__main__":
    entrypoint()
