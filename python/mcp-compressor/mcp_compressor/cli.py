"""Console entrypoint for the Python package."""

from __future__ import annotations

import json
import sys

from mcp_compressor import _native


def main(argv: list[str] | None = None) -> int:
    """Run the packaged mcp-compressor CLI.

    The Python package ships the native extension, so the console script should
    not require a separately installed or locally built Rust binary.
    """
    args = sys.argv[1:] if argv is None else argv
    return int(_native.run_cli_json(json.dumps(["mcp-compressor", *args])))


def entrypoint() -> None:
    raise SystemExit(main())


if __name__ == "__main__":
    entrypoint()
