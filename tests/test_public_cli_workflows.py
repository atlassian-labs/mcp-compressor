from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target" / "debug" / "mcp-compressor"
FIXTURE = ROOT / "crates" / "mcp-compressor-core" / "tests" / "fixtures" / "alpha_server.py"
PYTHON = os.environ.get("PYTHON", sys.executable)


def _ensure_binary() -> None:
    if not BINARY.exists():
        subprocess.run(["cargo", "build", "-p", "mcp-compressor-core"], cwd=ROOT, check=True)  # noqa: S607


def test_public_cli_mode_creates_executable_script(tmp_path: Path) -> None:
    _ensure_binary()
    output_dir = tmp_path / "bin"

    result = subprocess.run(  # noqa: S603
        [
            str(BINARY),
            "--cli-mode",
            "--server-name",
            "alpha",
            "--output-dir",
            str(output_dir),
            "--",
            PYTHON,
            str(FIXTURE),
        ],
        cwd=tmp_path,
        env={**os.environ, "MCP_COMPRESSOR_EXIT_AFTER_READY": "1"},
        text=True,
        capture_output=True,
        check=True,
        timeout=30,
    )

    assert "CLI ready" in result.stdout
    assert (output_dir / "alpha").exists()


def test_public_code_modes_default_to_dist(tmp_path: Path) -> None:
    _ensure_binary()

    for language, expected in [("python", "alpha.py"), ("typescript", "alpha.ts")]:
        result = subprocess.run(  # noqa: S603
            [
                str(BINARY),
                "--code-mode",
                language,
                "--server-name",
                "alpha",
                "--",
                PYTHON,
                str(FIXTURE),
            ],
            cwd=tmp_path,
            env={**os.environ, "MCP_COMPRESSOR_EXIT_AFTER_READY": "1"},
            text=True,
            capture_output=True,
            check=True,
            timeout=30,
        )
        assert "code client ready" in result.stdout
        assert (tmp_path / "dist" / expected).exists()


def test_public_backend_options_belong_after_separator(tmp_path: Path) -> None:
    _ensure_binary()

    before = subprocess.run(  # noqa: S603
        [str(BINARY), "--cwd", str(tmp_path), "--", PYTHON, str(FIXTURE)],
        text=True,
        capture_output=True,
        timeout=30,
    )
    assert before.returncode != 0
    assert "unexpected argument '--cwd'" in before.stderr

    after = subprocess.run(  # noqa: S603
        [
            str(BINARY),
            "--cli-mode",
            "--server-name",
            "alpha",
            "--output-dir",
            str(tmp_path / "bin"),
            "--",
            PYTHON,
            str(FIXTURE),
            "--cwd",
            str(tmp_path),
            "-e",
            "PUBLIC_WORKFLOW_SMOKE=1",
        ],
        env={**os.environ, "MCP_COMPRESSOR_EXIT_AFTER_READY": "1"},
        text=True,
        capture_output=True,
        check=True,
        timeout=30,
    )
    assert "CLI ready" in after.stdout
