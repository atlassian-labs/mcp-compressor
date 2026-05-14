#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

SEMVER_RE = re.compile(r"^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")
PEP440_PRERELEASE_RE = re.compile(
    r"^(?P<base>(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*))(?P<kind>a|b|rc)(?P<num>0|[1-9]\d*)$"
)
ROOT = Path(__file__).resolve().parents[1]
PACKAGE_JSON = ROOT / "typescript" / "package.json"


def version_from_tag(tag: str) -> str:
    version = tag.removeprefix("refs/tags/").removeprefix("v")
    if SEMVER_RE.match(version):
        return version
    if match := PEP440_PRERELEASE_RE.match(version):
        prerelease = {"a": "alpha", "b": "beta", "rc": "rc"}[match.group("kind")]
        return f"{match.group('base')}-{prerelease}.{match.group('num')}"
    raise SystemExit(
        f"Tag {tag!r} is not a supported SemVer tag such as v1.2.3 or Python prerelease tag such as v0.15.0a2"
    )


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
