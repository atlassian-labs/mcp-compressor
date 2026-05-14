#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
from pathlib import Path

# Python package versions follow PEP 440. This intentionally accepts prerelease
# tags such as `v0.15.0a1` in addition to ordinary release tags such as `v1.2.3`.
PEP440_RELEASE_TAG_RE = re.compile(
    r"^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:(a|b|rc)(0|[1-9]\d*))?"
    r"(?:\.post(0|[1-9]\d*))?"
    r"(?:\.dev(0|[1-9]\d*))?$"
)
ROOT = Path(__file__).resolve().parents[1]
PYPROJECT = ROOT / "python" / "mcp-compressor" / "pyproject.toml"


def version_from_tag(tag: str) -> str:
    version = tag.removeprefix("refs/tags/").removeprefix("v")
    if not PEP440_RELEASE_TAG_RE.match(version):
        raise SystemExit(f"Tag {tag!r} is not a supported Python release tag such as v1.2.3 or v0.15.0a1")
    return version


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("tag")
    args = parser.parse_args()
    version = version_from_tag(args.tag)
    text = PYPROJECT.read_text()
    updated, replacements = re.subn(
        r'^(version\s*=\s*)"[^"]+"',
        rf'\g<1>"{version}"',
        text,
        count=1,
        flags=re.MULTILINE,
    )
    if replacements != 1:
        raise SystemExit(f"Did not find project version in {PYPROJECT}")
    PYPROJECT.write_text(updated)
    print(version)


if __name__ == "__main__":
    main()
