"""Rust-backed mcp-compressor CLI entrypoint for the Python package."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path


def _candidate_binaries() -> list[str]:
    env_binary = os.environ.get("MCP_COMPRESSOR_CORE_BINARY")
    candidates: list[str] = []
    if env_binary:
        return [env_binary]
    candidates.append("mcp-compressor-core")
    repo_binary = Path(__file__).resolve().parents[3] / "target" / "debug" / "mcp-compressor-core"
    candidates.append(str(repo_binary))
    return candidates


def main(argv: list[str] | None = None) -> int:
    """Run the Rust core CLI, preserving stdio and process semantics."""
    args = sys.argv[1:] if argv is None else argv
    last_error: OSError | None = None
    for binary in _candidate_binaries():
        try:
            completed = subprocess.run([binary, *args], check=False)  # noqa: S603 - controlled CLI delegation
            return int(completed.returncode)
        except FileNotFoundError as exc:
            last_error = exc
            continue
    print(
        "mcp-compressor-core binary was not found. Build it with `cargo build -p mcp-compressor-core` "
        "or set MCP_COMPRESSOR_CORE_BINARY.",
        file=sys.stderr,
    )
    if last_error is not None:
        print(str(last_error), file=sys.stderr)
    return 127


def entrypoint() -> None:
    raise SystemExit(main())


if __name__ == "__main__":
    entrypoint()
