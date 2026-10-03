from __future__ import annotations

import argparse
import contextlib
import os
import subprocess
import sys
from pathlib import Path

from fastmcp import FastMCP

parser = argparse.ArgumentParser(add_help=False)
parser.add_argument("--pid-file", type=Path, required=True)
args, _ = parser.parse_known_args()
args.pid_file.write_text(str(os.getpid()), encoding="utf-8")

mcp = FastMCP("Rust Core Lifecycle Fixture")


@mcp.tool
def echo(message: str) -> str:
    return message


if __name__ == "__main__":
    # Trusted interpreter and fixed child code.
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(300)"])  # noqa: S603
    args.pid_file.with_suffix(".child.pid").write_text(str(child.pid), encoding="utf-8")
    try:
        with contextlib.suppress(KeyboardInterrupt):
            mcp.run(show_banner=False)
    finally:
        child.terminate()
        child.wait()
