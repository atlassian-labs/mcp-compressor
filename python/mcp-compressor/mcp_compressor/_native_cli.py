"""External-process runner for long-lived packaged CLI modes."""

from __future__ import annotations

import json
import sys

from mcp_compressor import _native


def main() -> int:
    return int(_native.run_cli_json(json.dumps(["mcp-compressor", *sys.argv[1:]])))


if __name__ == "__main__":
    raise SystemExit(main())
