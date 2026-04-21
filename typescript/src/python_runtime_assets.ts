/**
 * Static Python runtime assets shipped alongside the generated tool stubs. These are embedded as
 * TypeScript string constants so consumers can mount them into any execution environment without
 * wiring up file-asset loading. The agent-visible content is deliberately neutral: it does not
 * mention the host system, the library that produced it, or any internal concerns — from the
 * agent's perspective the package is just "the tool client".
 *
 * The runtime asset is emitted **per-server** at `<packageName>/<serverName>/_call.py`. This makes
 * the top-level `<packageName>/` directory a PEP 420 namespace package (no `__init__.py`), so when
 * multiple servers are mounted into separate directories on `PYTHONPATH`, Python merges their
 * `<packageName>/<serverName>/` subtrees automatically — `from <packageName> import <svc>` works
 * for every mounted service without any cross-server coordination.
 */

export interface RuntimeAssetOptions {
  /** Top-level Python package name (kept for symmetry with stub generation; not used in path layout). */
  packageName: string;
  /** Server name — used as the sub-package directory and the `service` field in bridge calls. */
  serverName: string;
  /**
   * The loopback bridge URL to bake into the generated client. The Python code will POST tool
   * calls directly to this URL — no env var indirection is needed since the URL is known at
   * generation time and the bridge lifetime matches the session lifetime.
   */
  bridgeUrl: string;
}

function renderCallModule(bridgeUrl: string): string {
  return `"""Internal transport for this tool service. Not intended for direct use by callers."""

from __future__ import annotations

import asyncio
import json
import urllib.error
import urllib.request
from typing import Any

DEFAULT_TIMEOUT_SECONDS = 60.0
_BRIDGE_URL = ${JSON.stringify(bridgeUrl)}


class ToolCallError(RuntimeError):
    """Raised when a tool call fails. Wraps both transport errors and tool-side errors."""

    def __init__(self, message: str, error_type: str | None = None, status_code: int = 500) -> None:
        super().__init__(message)
        self.error_type = error_type
        self.status_code = status_code


def _post_function_sync(service: str, function: str, params: dict[str, Any]) -> Any:
    body = json.dumps({"service": service, "function": function, "params": params}).encode("utf-8")
    request = urllib.request.Request(
        url=f"{_BRIDGE_URL}/function",
        data=body,
        method="POST",
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=DEFAULT_TIMEOUT_SECONDS) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except urllib.error.URLError as exc:
        raise ToolCallError(
            f"Failed to reach tool bridge: {exc}",
            error_type="TransportError",
            status_code=502,
        ) from exc
    except json.JSONDecodeError as exc:
        raise ToolCallError(
            f"Tool bridge returned non-JSON response: {exc}",
            error_type="ProtocolError",
            status_code=502,
        ) from exc

    if not isinstance(payload, dict):
        raise ToolCallError(
            "Tool bridge returned a non-object response.",
            error_type="ProtocolError",
            status_code=502,
        )

    if payload.get("success") is True:
        return payload.get("data")

    raise ToolCallError(
        str(payload.get("error", "Unknown tool error.")),
        error_type=payload.get("errorType"),
        status_code=int(payload.get("statusCode", 500)),
    )


async def _call(service: str, function: str, params: dict[str, Any]) -> Any:
    """Invoke a tool by service name and function name. Returns whatever the tool produced.

    Transport is a synchronous \`urllib\` POST wrapped in \`asyncio.to_thread\` so this runtime
    has no third-party dependencies — only the Python standard library.
    """
    return await asyncio.to_thread(_post_function_sync, service, function, params)


__all__ = ["ToolCallError", "_call"]
`;
}

/**
 * Get the per-server runtime assets keyed by their relative file paths under the package root.
 *
 * Returns a single file: `<packageName>/<serverName>/_call.py`. The top-level package directory
 * itself has no `__init__.py` — see the module-level docstring for the rationale (namespace
 * package semantics let multiple servers coexist on PYTHONPATH).
 */
export function getPythonRuntimeAssets(opts: RuntimeAssetOptions): ReadonlyMap<string, string> {
  return new Map([
    [`${opts.packageName}/${opts.serverName}/_call.py`, renderCallModule(opts.bridgeUrl)],
  ]);
}
