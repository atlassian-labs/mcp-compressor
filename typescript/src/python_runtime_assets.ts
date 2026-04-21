/**
 * Static Python runtime assets shipped alongside the generated tool stubs. These are embedded as
 * TypeScript string constants so consumers can mount them into any execution environment without
 * wiring up file-asset loading. The agent-visible content is deliberately neutral: it does not
 * mention the host system, the library that produced it, or any internal concerns — from the
 * agent's perspective the package is just "the tool client".
 */

/** Default name of the env var the Python runtime reads to discover the bridge URL. */
export const DEFAULT_BRIDGE_ENV_VAR = "MCP_TOOL_BRIDGE_URL";

export interface RuntimeAssetOptions {
  /** Top-level Python package name. */
  packageName: string;
  /** Env var name the runtime reads. Defaults to {@link DEFAULT_BRIDGE_ENV_VAR}. */
  bridgeEnvVar?: string;
}

function renderPackageInit(bridgeEnvVar: string): string {
  return `"""Tool client package.

Importable async functions in this package's sub-modules invoke tools running on the host. Each
sub-module corresponds to one tool service (e.g. \`tools.jira\`, \`tools.github\`).
"""

from __future__ import annotations

import asyncio
import json
import os
import urllib.error
import urllib.request
from typing import Any

DEFAULT_TIMEOUT_SECONDS = 60.0
_BRIDGE_ENV_VAR = ${JSON.stringify(bridgeEnvVar)}


class ToolCallError(RuntimeError):
    """Raised when a tool call fails. Wraps both transport errors and tool-side errors."""

    def __init__(self, message: str, error_type: str | None = None, status_code: int = 500) -> None:
        super().__init__(message)
        self.error_type = error_type
        self.status_code = status_code


def _resolve_base_url() -> str:
    url = os.environ.get(_BRIDGE_ENV_VAR)
    if not url:
        raise ToolCallError(
            f"{_BRIDGE_ENV_VAR} is not set; cannot reach the tool bridge.",
            error_type="ConfigurationError",
            status_code=500,
        )
    return url.rstrip("/")


def _post_function_sync(service: str, function: str, params: dict[str, Any]) -> Any:
    base_url = _resolve_base_url()
    body = json.dumps({"service": service, "function": function, "params": params}).encode("utf-8")
    request = urllib.request.Request(
        url=f"{base_url}/function",
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

/** Get the runtime assets keyed by their relative file paths under \`packageName/\`. */
export function getPythonRuntimeAssets(opts: RuntimeAssetOptions): ReadonlyMap<string, string> {
  const bridgeEnvVar = opts.bridgeEnvVar ?? DEFAULT_BRIDGE_ENV_VAR;
  return new Map([[`${opts.packageName}/__init__.py`, renderPackageInit(bridgeEnvVar)]]);
}

/** Backwards-compat re-export of the default rendered runtime, used in tests/snapshots. */
export const PACKAGE_INIT_PY = renderPackageInit(DEFAULT_BRIDGE_ENV_VAR);
