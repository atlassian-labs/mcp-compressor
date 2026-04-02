"""Logging setup and utilities for MCP Compressor.

Configures loguru as the logging backend and intercepts stdlib logging from
upstream libraries. Also provides narrow log filtering for recoverable OAuth
errors that are handled locally with a retry.
"""

from __future__ import annotations

import contextlib
import logging
import sys
from collections.abc import Iterator
from typing import TYPE_CHECKING

from loguru import logger
from loguru_logging_intercept import setup_loguru_logging_intercept

if TYPE_CHECKING:
    from mcp_compressor.types import LogLevel, TransportType


__all__ = [
    "configure_logging",
    "suppress_recoverable_oauth_traceback_logging",
]


def configure_logging(log_level: LogLevel) -> None:
    """Configure loguru and intercept upstream stdlib loggers.

    Should be called once at startup before any I/O begins.
    """
    logger.remove()
    logger.add(sys.stderr, level=log_level.value.upper())
    setup_loguru_logging_intercept(modules=("fastmcp",))


class _RecoverableOAuthTracebackFilter(logging.Filter):
    """Suppress upstream OAuth traceback logs for the narrow recoverable stale-cache case.

    FastMCP / python-sdk call logger.exception("OAuth flow error") before the
    exception propagates to our retry handler. When mcp-compressor detects a
    recoverable stale-credential error and retries, this filter prevents the
    upstream traceback from being printed even though the operation succeeds.
    """

    def filter(self, record: logging.LogRecord) -> bool:
        if record.getMessage() != "OAuth flow error" or record.exc_info is None:
            return True

        exc_str = str(record.exc_info[1])
        if "Unexpected authorization response: 500" in exc_str:
            return False
        return "OAuth client not found" not in exc_str


@contextlib.contextmanager
def suppress_recoverable_oauth_traceback_logging(transport: TransportType) -> Iterator[None]:
    """Temporarily suppress noisy upstream OAuth traceback logs for recoverable retries.

    Attaches a narrow log filter to the python-sdk and FastMCP OAuth loggers
    for the duration of the first ProxyClient connection attempt. The filter is
    always removed on exit, even if an exception is raised.

    Only active for HTTP/SSE transports using a FastMCP OAuth instance.
    """
    from fastmcp.client.auth import OAuth
    from fastmcp.client.transports import SSETransport, StreamableHttpTransport

    if not isinstance(transport, StreamableHttpTransport | SSETransport):
        yield
        return

    auth = getattr(transport, "auth", None)
    if not isinstance(auth, OAuth):
        yield
        return

    log_filter = _RecoverableOAuthTracebackFilter()
    auth_loggers = (
        logging.getLogger("mcp.client.auth.oauth2"),
        logging.getLogger("fastmcp.client.auth.oauth"),
    )
    for auth_logger in auth_loggers:
        auth_logger.addFilter(log_filter)

    try:
        yield
    finally:
        for auth_logger in auth_loggers:
            auth_logger.removeFilter(log_filter)
