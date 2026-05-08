from __future__ import annotations

from pathlib import Path

FORBIDDEN = {
    "mcp_compressor_rust": "Use public Python import package `mcp_compressor` instead.",
    "mcp_compressor_core::": "Use public Rust crate path `mcp_compressor::` in user-facing examples.",
    "from mcp_compressor_core": "Use public Rust crate path `mcp_compressor` in user-facing examples.",
}

SEARCH_ROOTS = [
    Path("docs"),
    Path("README.md"),
    Path("python/mcp-compressor-rust/README.md"),
    Path("typescript/README.md"),
    Path("tests"),
]

IGNORED_PARTS = {
    "__pycache__",
    ".ruff_cache",
    ".pytest_cache",
    ".venv",
    "dist",
    "site",
}


def candidate_files() -> list[Path]:
    files: list[Path] = []
    for root in SEARCH_ROOTS:
        if not root.exists():
            continue
        if root.is_file():
            files.append(root)
            continue
        for path in root.rglob("*"):
            if any(part in IGNORED_PARTS for part in path.parts):
                continue
            if path.suffix in {".md", ".py", ".ts", ".tsx", ".yml", ".yaml"}:
                files.append(path)
    return files


def main() -> int:
    failures: list[str] = []
    for path in candidate_files():
        text = path.read_text(errors="ignore")
        for forbidden, message in FORBIDDEN.items():
            if forbidden in text:
                failures.append(f"{path}: found `{forbidden}`. {message}")
    if failures:
        print("Public API name check failed:")
        for failure in failures:
            print(f"- {failure}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
