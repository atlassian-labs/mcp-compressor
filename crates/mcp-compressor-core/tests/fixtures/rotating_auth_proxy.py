from __future__ import annotations

import http.server
import os
import sys
import threading
import urllib.error
import urllib.request

TARGET_URL = os.environ["MCP_COMPRESSOR_AUTH_PROXY_TARGET"].rstrip("/")
EXPECTED_START = int(os.environ.get("MCP_COMPRESSOR_AUTH_PROXY_EXPECTED_START", "1"))
ALLOW_INITIAL_REPEATS = int(os.environ.get("MCP_COMPRESSOR_AUTH_PROXY_ALLOW_INITIAL_REPEATS", "0"))
ALLOW_ANY_REPEATS = int(os.environ.get("MCP_COMPRESSOR_AUTH_PROXY_ALLOW_ANY_REPEATS", "0"))
DEBUG = os.environ.get("MCP_COMPRESSOR_AUTH_PROXY_DEBUG") == "1"


class Handler(http.server.BaseHTTPRequestHandler):
    counter = 0
    last_token = EXPECTED_START - 1
    initial_repeats = 0
    any_repeats = 0
    lock = threading.Lock()

    def do_POST(self) -> None:
        self._proxy()

    def do_GET(self) -> None:
        self._proxy()

    def do_DELETE(self) -> None:
        self._proxy()

    def log_message(self, format: str, *args: object) -> None:  # noqa: A002
        return

    def _proxy(self) -> None:  # noqa: C901
        actual = self.headers.get("Authorization")
        if DEBUG:
            print(
                f"AUTH_PROXY_REQUEST method={self.command} path={self.path} auth={actual}", file=sys.stderr, flush=True
            )
        if actual is None or not actual.startswith("Bearer token-"):
            self.send_response(401)
            self.end_headers()
            self.wfile.write(f"expected rotating bearer token, got {actual}".encode())
            return
        try:
            token_number = int(actual.rsplit("-", 1)[1])
        except ValueError:
            self.send_response(401)
            self.end_headers()
            self.wfile.write(f"invalid rotating bearer token: {actual}".encode())
            return
        with Handler.lock:
            Handler.counter += 1
            if token_number == Handler.last_token and Handler.counter <= ALLOW_ANY_REPEATS + 1:
                Handler.any_repeats += 1
            elif (
                token_number == EXPECTED_START
                and Handler.last_token == EXPECTED_START
                and Handler.initial_repeats < ALLOW_INITIAL_REPEATS
            ):
                Handler.initial_repeats += 1
            elif token_number < EXPECTED_START or token_number <= Handler.last_token:
                self.send_response(401)
                self.end_headers()
                self.wfile.write(
                    f"expected token number > {Handler.last_token} and >= {EXPECTED_START}, got {token_number}".encode()
                )
                return
            else:
                Handler.last_token = token_number
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length else None
        target = f"{TARGET_URL}{self.path}"
        headers = {
            key: value
            for key, value in self.headers.items()
            if key.lower() not in {"host", "content-length", "authorization"}
        }
        if DEBUG:
            print(f"AUTH_PROXY_FORWARD target={target}", file=sys.stderr, flush=True)
        request = urllib.request.Request(target, data=body, headers=headers, method=self.command)  # noqa: S310
        try:
            with urllib.request.urlopen(request, timeout=30) as response:  # noqa: S310 - local test proxy
                payload = response.read()
                if DEBUG:
                    print(
                        f"AUTH_PROXY_RESPONSE status={response.status} bytes={len(payload)}",
                        file=sys.stderr,
                        flush=True,
                    )
                self.send_response(response.status)
                for key, value in response.headers.items():
                    if key.lower() not in {"transfer-encoding", "connection", "content-length"}:
                        self.send_header(key, value)
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)
        except urllib.error.HTTPError as error:
            payload = error.read()
            if DEBUG:
                print(
                    f"AUTH_PROXY_HTTP_ERROR status={error.code} bytes={len(payload)} body={payload.decode(errors='replace')}",
                    file=sys.stderr,
                    flush=True,
                )
            self.send_response(error.code)
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        except Exception as error:
            if DEBUG:
                print(f"AUTH_PROXY_FORWARD_ERROR {type(error).__name__}: {error}", file=sys.stderr, flush=True)
            self.send_response(502)
            self.end_headers()
            self.wfile.write(str(error).encode())


def main() -> None:
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    print(f"AUTH_PROXY_URL=http://127.0.0.1:{server.server_port}", file=sys.stderr, flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
