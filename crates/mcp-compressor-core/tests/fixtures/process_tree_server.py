"""Long-lived process tree used to verify test child cleanup.

Started as ``parent`` it spawns one grandchild and both processes append a
heartbeat to their own file until they are killed. A caller can therefore tell
whether the whole tree stopped by checking that both heartbeat files stop
growing.
"""

from __future__ import annotations

import subprocess
import sys
import time
from pathlib import Path


def heartbeat(path: Path) -> None:
    while True:
        with path.open("a", encoding="utf-8") as handle:
            handle.write("tick\n")
        time.sleep(0.05)


if __name__ == "__main__":
    role = sys.argv[1]
    target = Path(sys.argv[2])
    if role == "parent":
        subprocess.Popen(  # noqa: S603
            [sys.executable, __file__, "child", str(target.with_suffix(".child"))]
        )
    heartbeat(target)
