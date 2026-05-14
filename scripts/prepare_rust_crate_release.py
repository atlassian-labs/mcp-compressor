#!/usr/bin/env python3
"""Patch Rust crate versions from a release tag in the CI workspace.

The repository keeps Rust crate versions at 0.0.0 so source does not need a
version bump commit for every release. Release workflows call this script with a
Git tag such as `v1.2.3`; it rewrites only the checked-out workspace before
`cargo package` / `cargo publish`.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

SEMVER_RE = re.compile(r"^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")
PEP440_PRERELEASE_RE = re.compile(
    r"^(?P<base>(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*))(?P<kind>a|b|rc)(?P<num>0|[1-9]\d*)$"
)

ROOT = Path(__file__).resolve().parents[1]
PUBLIC_MANIFEST = ROOT / "crates" / "mcp-compressor" / "Cargo.toml"
CORE_MANIFEST = ROOT / "crates" / "mcp-compressor-core" / "Cargo.toml"
LOCKFILE = ROOT / "Cargo.lock"


def version_from_tag(tag: str) -> str:
    version = tag.removeprefix("refs/tags/")
    version = version.removeprefix("v")
    if SEMVER_RE.match(version):
        return version
    if match := PEP440_PRERELEASE_RE.match(version):
        prerelease = {"a": "alpha", "b": "beta", "rc": "rc"}[match.group("kind")]
        return f"{match.group('base')}-{prerelease}.{match.group('num')}"
    raise SystemExit(
        f"Tag {tag!r} is not a supported SemVer tag such as v1.2.3 or Python prerelease tag such as v0.15.0a2"
    )


def replace_regex_once(path: Path, pattern: str, replacement: str) -> None:
    text = path.read_text()
    updated, replacements = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if replacements != 1:
        raise SystemExit(f"Did not find expected pattern in {path}: {pattern!r}")
    path.write_text(updated)


def patch_manifest_versions(version: str) -> None:
    replace_regex_once(CORE_MANIFEST, r'^(version\s*=\s*)"[^"]+"', rf'\g<1>"{version}"')
    replace_regex_once(PUBLIC_MANIFEST, r'^(version\s*=\s*)"[^"]+"', rf'\g<1>"{version}"')
    replace_regex_once(
        PUBLIC_MANIFEST,
        r'(mcp-compressor-core\s*=\s*\{\s*path\s*=\s*"\.\./mcp-compressor-core",\s*version\s*=\s*)"[^"]+"(\s*\})',
        rf'\g<1>"{version}"\2',
    )


def patch_lock_versions(version: str) -> None:
    text = LOCKFILE.read_text()
    for package_name in ("mcp-compressor", "mcp-compressor-core"):
        text, replacements = re.subn(
            rf'(name = "{re.escape(package_name)}"\nversion = ")[^"]+(")',
            rf"\g<1>{version}\2",
            text,
            count=1,
        )
        if replacements != 1:
            raise SystemExit(f"Did not find {package_name} package entry in {LOCKFILE}")
    LOCKFILE.write_text(text)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("tag", help="Git release tag, for example v1.2.3")
    args = parser.parse_args()

    version = version_from_tag(args.tag)
    patch_manifest_versions(version)
    patch_lock_versions(version)
    print(version)


if __name__ == "__main__":
    main()
