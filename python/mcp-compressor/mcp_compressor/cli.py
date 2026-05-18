"""Rust-backed mcp-compressor CLI entrypoint for the Python package."""

from __future__ import annotations

import os
import shutil
import signal
import subprocess
import sys
from pathlib import Path

# Leading byte sequences that identify native compiled binaries.
# ELF and Mach-O entries are 4 bytes; the Windows PE stub is only 2 bytes
# ("MZ") because the remaining two bytes vary by linker.  All entries are
# matched with ``bytes.startswith`` so shorter prefixes work correctly.
# Used to distinguish native binaries from Python/shell wrapper scripts
# (pip/uvx shims) and thereby prevent an infinite fork-bomb.
_NATIVE_MAGIC: tuple[bytes, ...] = (
    b"\x7fELF",          # ELF (Linux, FreeBSD, …)
    b"\xfe\xed\xfa\xce", # Mach-O 32-bit big-endian
    b"\xfe\xed\xfa\xcf", # Mach-O 64-bit big-endian
    b"\xce\xfa\xed\xfe", # Mach-O 32-bit little-endian
    b"\xcf\xfa\xed\xfe", # Mach-O 64-bit little-endian
    b"\xca\xfe\xba\xbe", # Mach-O fat binary big-endian
    b"\xbe\xba\xfe\xca", # Mach-O fat binary little-endian
    b"MZ",               # PE (Windows) — 2-byte stub; trailing bytes vary
)


def _is_native_executable(path: str) -> bool:
    """Return True if *path* is a native compiled binary, not a Python/shell script.

    This prevents the fork-bomb that occurs when the Python pip/uvx shim is the
    only ``mcp-compressor`` on PATH and would otherwise call itself recursively.
    """
    try:
        with open(path, "rb") as fh:
            magic = fh.read(4)
    except OSError:
        return False
    return any(magic.startswith(m) for m in _NATIVE_MAGIC)


def _candidate_binaries() -> list[str]:
    env_binary = os.environ.get("MCP_COMPRESSOR_BINARY") or os.environ.get("MCP_COMPRESSOR_CORE_BINARY")
    candidates: list[str] = []
    if env_binary:
        return [env_binary]
    # Resolve 'mcp-compressor' on PATH, but only add it when it is a native
    # compiled binary.  If the resolved file is a Python/shell wrapper (the
    # pip/uvx console-script shim), including it would cause an infinite fork
    # bomb: the shim calls _candidate_binaries() → finds itself → spawns
    # another copy → ad infinitum.
    path_binary = shutil.which("mcp-compressor")
    if path_binary is not None and _is_native_executable(path_binary):
        candidates.append(path_binary)
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
