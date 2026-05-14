from __future__ import annotations

import importlib.util
from pathlib import Path
from types import ModuleType

ROOT = Path(__file__).resolve().parents[1]


def _load_script(name: str) -> ModuleType:
    path = ROOT / "scripts" / name
    spec = importlib.util.spec_from_file_location(name.removesuffix(".py"), path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_python_release_script_accepts_pep440_prerelease_tags() -> None:
    script = _load_script("prepare_python_release.py")

    assert script.version_from_tag("v0.15.0a1") == "0.15.0a1"
    assert script.version_from_tag("refs/tags/v1.2.3") == "1.2.3"
    assert script.version_from_tag("v1.2.3rc4") == "1.2.3rc4"


def test_typescript_release_script_accepts_semver_tags() -> None:
    script = _load_script("prepare_typescript_release.py")

    assert script.version_from_tag("v1.2.3") == "1.2.3"
    assert script.version_from_tag("refs/tags/v1.2.3-alpha.1") == "1.2.3-alpha.1"
    assert script.version_from_tag("v0.15.0a2") == "0.15.0-alpha.2"
    assert script.version_from_tag("v0.15.0b3") == "0.15.0-beta.3"
    assert script.version_from_tag("v0.15.0rc4") == "0.15.0-rc.4"
    assert script.npm_dist_tag_for_version("1.2.3") == "latest"
    assert script.npm_dist_tag_for_version("1.2.3-alpha.1") == "next"


def test_rust_release_script_accepts_semver_tags() -> None:
    script = _load_script("prepare_rust_crate_release.py")

    assert script.version_from_tag("v1.2.3") == "1.2.3"
    assert script.version_from_tag("refs/tags/v1.2.3-alpha.1") == "1.2.3-alpha.1"
    assert script.version_from_tag("v0.15.0a2") == "0.15.0-alpha.2"
    assert script.version_from_tag("v0.15.0b3") == "0.15.0-beta.3"
    assert script.version_from_tag("v0.15.0rc4") == "0.15.0-rc.4"
