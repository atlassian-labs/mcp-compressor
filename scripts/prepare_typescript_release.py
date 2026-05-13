#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

SEMVER_RE = re.compile(r"^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")
ROOT = Path(__file__).resolve().parents[1]
PACKAGE_JSON = ROOT / "typescript" / "package.json"


def version_from_tag(tag: str) -> str:
    version = tag.removeprefix("refs/tags/").removeprefix("v")
    if not SEMVER_RE.match(version):
        raise SystemExit(f"Tag {tag!r} is not a supported semver tag such as v1.2.3")
    return version


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("tag")
    args = parser.parse_args()
    version = version_from_tag(args.tag)
    data = json.loads(PACKAGE_JSON.read_text())
    data["version"] = version
    PACKAGE_JSON.write_text(json.dumps(data, indent=2) + "\n")
    print(version)


if __name__ == "__main__":
    main()
