"""CLI script generator for CLI mode.

Generates an executable script (Python on Unix, .cmd batch on Windows)
that forwards arguments to the local mcp-compressor bridge HTTP server.

Multi-instance support
----------------------
When multiple mcp-compressor processes run in CLI mode for the same backend
server, they share a single script file.  The script contains a ``BRIDGES``
mapping of ``{session_pid: bridge_url}`` where ``session_pid`` is the parent
PID of the mcp-compressor process (i.e. the orchestrating agent/shell PID).

At invocation time the script walks its own process ancestors to find the
entry in ``BRIDGES`` that belongs to the same session tree, ensuring each
agent routes to its own bridge transparently.

Lifecycle
---------
On shutdown, mcp-compressor removes its own entry from ``BRIDGES``.  If no
entries remain the script file is deleted entirely.
"""

from __future__ import annotations

import contextlib
import os
import platform
import stat
import sys
import urllib.request
from pathlib import Path

_IS_WINDOWS = platform.system() == "Windows"

# Ordered list of candidate directories for the generated CLI script.
_UNIX_CANDIDATE_SCRIPT_DIRS: list[str] = [
    "~/.local/bin",
    "~/bin",
    "/usr/local/bin",
    "/opt/homebrew/bin",
]

_WINDOWS_CANDIDATE_SCRIPT_DIRS: list[str] = [
    "~/AppData/Local/Microsoft/WindowsApps",
    "~/.local/bin",
]

# Marker written into every generated script so we can identify ours.
_SCRIPT_MARKER = "# mcp-compressor cli-mode script"

# Key used to locate the BRIDGES dict in the script source.
_BRIDGES_MARKER = "BRIDGES = "


def find_script_dir() -> tuple[Path, bool]:
    """Find the best directory for writing the generated CLI script.

    Tries each candidate in the platform-appropriate list in order, choosing the
    first that both exists and is present on the user's ``PATH``.  Falls back
    to the current working directory when none qualify.

    Returns:
        A 2-tuple of:
          - the chosen directory as a ``Path``
          - ``True`` if the directory is on PATH (script can be run without ``./``),
            ``False`` if the CWD fallback was used.
    """
    candidates = _WINDOWS_CANDIDATE_SCRIPT_DIRS if _IS_WINDOWS else _UNIX_CANDIDATE_SCRIPT_DIRS
    path_dirs = {Path(p).expanduser().resolve() for p in os.environ.get("PATH", "").split(os.pathsep) if p}

    for candidate in candidates:
        expanded = Path(candidate).expanduser().resolve()
        if expanded in path_dirs and expanded.exists():
            return expanded, True

    return Path.cwd(), False


def _script_path_for(cli_name: str, script_dir: Path) -> Path:
    """Return the script path for *cli_name* in *script_dir*."""
    if _IS_WINDOWS:
        return script_dir / f"{cli_name}.cmd"
    return script_dir / cli_name


def _read_bridges(script_path: Path) -> dict[int, str]:
    """Parse the BRIDGES dict from an existing mcp-compressor script.

    Returns an empty dict if the file doesn't exist, isn't an
    mcp-compressor script, or cannot be parsed.
    """
    if not script_path.exists():
        return {}
    try:
        content = script_path.read_text()
    except OSError:
        return {}
    if _SCRIPT_MARKER not in content:
        return {}
    for line in content.splitlines():
        if line.startswith(_BRIDGES_MARKER):
            try:
                return dict(eval(line[len(_BRIDGES_MARKER) :]))  # noqa: S307
            except Exception:
                return {}
    return {}


def _check_bridge_alive(url: str) -> bool:
    """Return True if the bridge at *url* responds to /health."""
    try:
        req = urllib.request.Request(url + "/health", method="GET")  # noqa: S310
        with urllib.request.urlopen(req, timeout=1) as resp:  # noqa: S310
            return resp.status == 200
    except Exception:
        return False


def _live_bridges(bridges: dict[int, str]) -> dict[int, str]:
    """Return only entries from *bridges* whose bridge server is alive."""
    return {pid: url for pid, url in bridges.items() if _check_bridge_alive(url)}


def generate_cli_script(
    cli_name: str,
    bridge_port: int,
    session_pid: int,
    script_dir: Path | None = None,
) -> tuple[Path, bool]:
    """Write (or update) the CLI script for *cli_name*.

    Reads any existing BRIDGES map from the script, prunes dead entries,
    adds the new ``session_pid → bridge_url`` entry, and rewrites the script.

    Args:
        cli_name: The CLI command name (e.g. ``"atlassian"``).
        bridge_port: The local port the bridge HTTP server is listening on.
        session_pid: The parent PID of the mcp-compressor process (the
            orchestrating agent/shell PID used as the session key).
        script_dir: Directory in which to write the script. If ``None``,
            ``find_script_dir`` is used to pick the best available location.

    Returns:
        A 2-tuple of:
          - the ``Path`` to the generated script
          - ``True`` if the script dir is on PATH (no ``./`` needed)
    """
    if script_dir is not None:
        script_dir_path = script_dir
        on_path = True
    else:
        script_dir_path, on_path = find_script_dir()

    script_dir_path.mkdir(parents=True, exist_ok=True)
    script_path = _script_path_for(cli_name, script_dir_path)

    # Merge with any existing live bridges
    existing = _read_bridges(script_path)
    bridges = _live_bridges(existing)
    bridges[session_pid] = f"http://127.0.0.1:{bridge_port}"

    if _IS_WINDOWS:
        _write_windows_script(script_path, cli_name, bridges)
    else:
        _write_unix_script(script_path, cli_name, bridges)

    return script_path, on_path


def remove_cli_script_entry(
    cli_name: str,
    session_pid: int,
    script_dir: Path | None = None,
) -> None:
    """Remove *session_pid* from the script's BRIDGES map on shutdown.

    If no live entries remain after removal, the script file is deleted.

    Args:
        cli_name: The CLI command name.
        session_pid: The session PID key to remove.
        script_dir: Directory containing the script. If ``None``,
            ``find_script_dir`` is used.
    """
    if script_dir is not None:
        script_dir_path = script_dir
    else:
        script_dir_path, _ = find_script_dir()

    script_path = _script_path_for(cli_name, script_dir_path)
    if not script_path.exists():
        return

    bridges = _read_bridges(script_path)
    bridges.pop(session_pid, None)

    # Remove dead entries too while we're here
    bridges = _live_bridges(bridges)

    if not bridges:
        with contextlib.suppress(OSError):
            script_path.unlink()
        return

    if _IS_WINDOWS:
        _write_windows_script(script_path, cli_name, bridges)
    else:
        _write_unix_script(script_path, cli_name, bridges)


# ---------------------------------------------------------------------------
# Platform-specific script writers
# ---------------------------------------------------------------------------


def _bridges_repr(bridges: dict[int, str]) -> str:
    """Render *bridges* as a compact Python literal."""
    inner = ", ".join(f"{pid}: {url!r}" for pid, url in sorted(bridges.items()))
    return "{" + inner + "}"


def _write_unix_script(script_path: Path, cli_name: str, bridges: dict[int, str]) -> None:
    """Write a Unix Python3 shebang script with the given BRIDGES map."""
    content = f"""\
#!{sys.executable}
{_SCRIPT_MARKER}
# CLI name: {cli_name}
# Do not edit manually — managed by mcp-compressor

import json
import os
import sys
import urllib.error
import urllib.request

BRIDGES = {_bridges_repr(bridges)}


def _find_bridge() -> str | None:
    \"\"\"Walk process ancestors to find the bridge for the current session.\"\"\"
    try:
        import psutil  # noqa: PLC0415

        for ancestor in psutil.Process().parents():
            if ancestor.pid in BRIDGES:
                return BRIDGES[ancestor.pid]
    except Exception:
        pass
    return None


def _bridge_alive(url: str) -> bool:
    try:
        req = urllib.request.Request(url + "/health", method="GET")
        with urllib.request.urlopen(req, timeout=1) as resp:
            return resp.status == 200
    except Exception:
        return False


def _pick_bridge() -> str | None:
    url = _find_bridge()
    if url and _bridge_alive(url):
        return url
    # Fallback: try all live bridges
    for url in BRIDGES.values():
        if _bridge_alive(url):
            return url
    return None


def main() -> None:
    bridge = _pick_bridge()
    if not bridge:
        sys.stderr.write(
            "error: could not connect to mcp-compressor bridge\\n"
            "Is mcp-compressor running in --cli-mode?\\n"
        )
        sys.exit(1)

    argv = sys.argv[1:]
    # Skip TOON when stdout is piped/redirected so downstream tools get raw JSON.
    try:
        toonify_hint = sys.stdout.isatty()
    except Exception:
        toonify_hint = True
    payload = json.dumps({{"argv": argv, "toonify": toonify_hint}}).encode()
    req = urllib.request.Request(
        bridge + "/exec",
        data=payload,
        headers={{"Content-Type": "application/json"}},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req) as resp:
            sys.stdout.write(resp.read().decode())
            sys.exit(0)
    except urllib.error.HTTPError as exc:
        sys.stderr.write(exc.read().decode())
        sys.exit(1)
    except urllib.error.URLError:
        sys.stderr.write(
            f"error: could not connect to mcp-compressor bridge at {{bridge}}\\n"
            "Is mcp-compressor running in --cli-mode?\\n"
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
"""
    script_path.write_text(content)
    current_mode = script_path.stat().st_mode
    script_path.chmod(current_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def _write_windows_script(script_path: Path, cli_name: str, bridges: dict[int, str]) -> None:
    """Write a Windows .cmd script that calls PowerShell to POST to the bridge."""
    # For Windows, embed the bridges as a PowerShell hashtable and do the
    # ancestor walk in PowerShell using Get-Process / .Parent.
    bridges_ps = "; ".join(f"{pid} = '{url}'" for pid, url in sorted(bridges.items()))
    content = f"""\
@echo off
REM {_SCRIPT_MARKER}
REM CLI name: {cli_name}
REM Do not edit manually - managed by mcp-compressor

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$bridges = @{{ {bridges_ps} }}; " ^
  "$bridge = $null; " ^
  "try {{ " ^
  "  $proc = Get-Process -Id $PID; " ^
  "  while ($proc.Parent -ne $null) {{ " ^
  "    $proc = $proc.Parent; " ^
  "    if ($bridges.ContainsKey($proc.Id)) {{ $bridge = $bridges[$proc.Id]; break }} " ^
  "  }} " ^
  "}} catch {{}}; " ^
  "if (-not $bridge) {{ foreach ($u in $bridges.Values) {{ " ^
  "  try {{ if ((Invoke-WebRequest -Uri ($u+'/health') -UseBasicParsing -TimeoutSec 1 -ErrorAction Stop).StatusCode -eq 200) {{ $bridge = $u; break }} }} catch {{}} " ^
  "}}}}; " ^
  "if (-not $bridge) {{ Write-Error 'error: no live mcp-compressor bridge found'; exit 1 }}; " ^
  "try {{ $toonify = -not [Console]::IsOutputRedirected }} catch {{ $toonify = $true }}; " ^
  "$payload = @{{ argv = @(%*); toonify = $toonify }} | ConvertTo-Json -Compress; " ^
  "try {{ " ^
  "  $r = Invoke-WebRequest -Uri ($bridge+'/exec') -Method POST -ContentType 'application/json' -Body $payload -UseBasicParsing -ErrorAction Stop; " ^
  "  Write-Host $r.Content; exit 0 " ^
  "}} catch {{ " ^
  "  $e = $_.Exception; " ^
  "  if ($e -is [System.Net.WebException] -and $e.Response) {{ " ^
  "    $sr = New-Object System.IO.StreamReader($e.Response.GetResponseStream()); " ^
  "    Write-Error $sr.ReadToEnd(); exit 1 " ^
  "  }}; " ^
  "  Write-Error ('error: ' + $e.Message); exit 1 " ^
  "}}"
"""
    script_path.write_text(content)
