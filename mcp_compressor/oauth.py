"""OAuth utilities for MCP transports.

Provides helpers for building OAuth auth objects, detecting stale OAuth errors,
and clearing cached OAuth state on transports.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import httpx
import keyring
import keyring.errors
from cryptography.fernet import Fernet
from fastmcp.client.auth import OAuth
from fastmcp.client.transports import SSETransport, StreamableHttpTransport
from key_value.aio.protocols import AsyncKeyValue
from key_value.aio.stores.filetree import (
    FileTreeStore,
    FileTreeV1CollectionSanitizationStrategy,
    FileTreeV1KeySanitizationStrategy,
)
from key_value.aio.wrappers.encryption import FernetEncryptionWrapper
from loguru import logger


def build_auth(headers: dict[str, str], url: str) -> httpx.Auth | None:
    """Return an ``OAuth`` auth provider, or ``None`` if an Authorization header is already present.

    When users explicitly provide an Authorization header, we leave it in the headers
    dict and skip OAuth entirely.  This prevents OAuth from silently overriding the
    user's token.  The header is passed through to the httpx client as-is, which means
    any scheme (Bearer, Basic, etc.) is supported.
    """
    if any(key.lower() == "authorization" for key in headers):
        return None
    return OAuth(mcp_url=url, token_storage=_build_token_storage())


async def handle_stale_oauth_error(exc: Exception, transport: Any) -> bool:
    """If *exc* looks like a stale cached OAuth error on *transport*, clear the cache and return ``True``.

    Callers should retry the connection once when this returns ``True``.
    Returns ``False`` (and does nothing) when the error is unrelated to OAuth.
    """
    if not isinstance(transport, StreamableHttpTransport | SSETransport):
        return False

    auth = getattr(transport, "auth", None)
    if not isinstance(auth, OAuth):
        return False

    # Any HTTP 4xx/5xx auth failure from an OAuth-enabled transport indicates a stale token.
    # Common cases:
    # - 401 Unauthorized: token expired or revoked (any OAuth server)
    # - 500 with "OAuth client not found" or "Unexpected authorization response": Atlassian-specific stale state
    if isinstance(exc, httpx.HTTPStatusError) and exc.response.status_code in (401, 403):
        is_stale = True
    else:
        exc_str = str(exc)
        is_stale = "Unexpected authorization response: 500" in exc_str or "OAuth client not found" in exc_str

    if not is_stale:
        return False

    logger.warning("OAuth connection failed due to stale cached credentials; clearing cached state for retry")
    if hasattr(auth, "token_storage_adapter"):
        auth._initialized = False
        await auth.token_storage_adapter.clear()

    return True


_OAUTH_CONFIG_DIR = Path.home() / ".config" / "mcp-compressor"
_OAUTH_TOKEN_DIR = _OAUTH_CONFIG_DIR / "oauth-tokens"
_OAUTH_KEY_FILE = _OAUTH_CONFIG_DIR / ".key"
_KEYRING_SERVICE = "mcp-compressor"
_KEYRING_USERNAME = "oauth-encryption-key"


def _get_or_create_encryption_key() -> bytes:
    """Return a persistent Fernet encryption key for OAuth token storage.

    Tries the OS keychain first (macOS Keychain, Windows Credential Manager,
    GNOME Keyring).  Falls back to a file at
    ``~/.config/mcp-compressor/.key`` with 0o600 permissions if the keychain
    is unavailable (e.g. headless/server environments).
    """
    try:
        stored = keyring.get_password(_KEYRING_SERVICE, _KEYRING_USERNAME)
        if stored:
            logger.debug("OAuth encryption key loaded from OS keychain")
            return stored.encode()
        new_key = Fernet.generate_key()
        keyring.set_password(_KEYRING_SERVICE, _KEYRING_USERNAME, new_key.decode())
        logger.debug("OAuth encryption key generated and stored in OS keychain")
    except (keyring.errors.NoKeyringError, keyring.errors.PasswordSetError, keyring.errors.KeyringError):
        logger.debug("OS keychain unavailable or access denied; falling back to file-based encryption key")
    else:
        return new_key

    _OAUTH_CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    if _OAUTH_KEY_FILE.exists():
        key = _OAUTH_KEY_FILE.read_bytes().strip()
        logger.debug("OAuth encryption key loaded from {}", _OAUTH_KEY_FILE)
        return key
    new_key = Fernet.generate_key()
    _OAUTH_KEY_FILE.write_bytes(new_key)
    _OAUTH_KEY_FILE.chmod(0o600)
    logger.debug("OAuth encryption key generated and stored at {}", _OAUTH_KEY_FILE)
    return new_key


def _build_token_storage() -> AsyncKeyValue:
    """Build an encrypted persistent OAuth token storage backend."""
    _OAUTH_TOKEN_DIR.mkdir(parents=True, exist_ok=True)
    store: AsyncKeyValue = FileTreeStore(
        data_directory=_OAUTH_TOKEN_DIR,
        key_sanitization_strategy=FileTreeV1KeySanitizationStrategy(_OAUTH_TOKEN_DIR),
        collection_sanitization_strategy=FileTreeV1CollectionSanitizationStrategy(_OAUTH_TOKEN_DIR),
    )
    fernet_key = _get_or_create_encryption_key()
    encrypted_store = FernetEncryptionWrapper(key_value=store, fernet=Fernet(fernet_key))
    logger.debug("OAuth token storage: encrypted file-tree store at {}", _OAUTH_TOKEN_DIR)
    return encrypted_store
