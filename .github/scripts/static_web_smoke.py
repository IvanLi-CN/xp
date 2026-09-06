#!/usr/bin/env python3
"""Public-only smoke gate for the static XP Web deployment."""

from __future__ import annotations

import argparse
import re
import sys
import time
from urllib.parse import quote, urljoin, urlparse
from urllib.request import Request, urlopen


def fetch(url: str) -> tuple[int, dict[str, str], bytes]:
    request = Request(url, headers={"Cache-Control": "no-cache", "Accept": "*/*"})
    with urlopen(request, timeout=10) as response:
        body = response.read()
        return response.status, {key.lower(): value for key, value in response.headers.items()}, body


def assert_static_contract(
    base_url: str, build_id: str, bootstrap_origin: str
) -> None:
    root_status, root_headers, root_body = fetch(base_url)
    if root_status != 200:
        raise RuntimeError(f"root returned HTTP {root_status}")
    root_text = root_body.decode("utf-8", "replace")
    encoded_build = quote(build_id, safe="")
    if f"xp-build={encoded_build}" not in root_text and build_id not in root_text:
        raise RuntimeError("root does not identify the expected Web build")
    csp = root_headers.get("content-security-policy", "")
    connect = re.search(r"(?:^|;)\s*connect-src\s+([^;]+)", csp, re.IGNORECASE)
    if not connect:
        raise RuntimeError("root CSP has no connect-src directive")
    sources = connect.group(1).split()
    if sources != ["'self'", bootstrap_origin]:
        raise RuntimeError(f"root CSP is not bootstrap-only: {sources!r}")
    if "nosniff" not in root_headers.get("x-content-type-options", "").lower():
        raise RuntimeError("root is missing X-Content-Type-Options: nosniff")
    if "no-store" not in root_headers.get("cache-control", "").lower():
        raise RuntimeError("root is cacheable")

    route_status, route_headers, route_body = fetch(urljoin(base_url, "/__xp-smoke-deep-route"))
    if route_status != 200 or route_body != root_body:
        raise RuntimeError("SPA deep route did not return the exact app shell")
    if "text/html" not in route_headers.get("content-type", "").lower():
        raise RuntimeError("SPA deep route is not HTML")

    asset_match = re.search(r"(?:src|href)=\"(/assets/[^\"]+)\?xp-build=", root_text)
    if not asset_match:
        raise RuntimeError("root did not expose a build-pinned asset")
    asset_status, asset_headers, _ = fetch(urljoin(base_url, asset_match.group(1)))
    if asset_status != 200 or "immutable" not in asset_headers.get("cache-control", "").lower():
        raise RuntimeError("hashed asset is not immutable")

    sw_status, sw_headers, _ = fetch(urljoin(base_url, "/sw.js"))
    if sw_status != 200 or "no-store" not in sw_headers.get("cache-control", "").lower():
        raise RuntimeError("sw.js is cacheable or unavailable")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", required=True)
    parser.add_argument("--build-id", required=True)
    parser.add_argument("--bootstrap-origin", required=True)
    parser.add_argument("--timeout-seconds", type=int, default=300)
    args = parser.parse_args()
    base_url = args.url.rstrip("/") + "/"
    parsed = urlparse(base_url)
    if parsed.scheme != "https" or not parsed.netloc:
        raise SystemExit("--url must be an HTTPS origin")
    deadline = time.monotonic() + args.timeout_seconds
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            assert_static_contract(base_url, args.build_id, args.bootstrap_origin)
            print("static web public smoke passed")
            return 0
        except Exception as error:  # noqa: BLE001 - retry transient propagation
            last_error = error
            time.sleep(5)
    print(f"static web public smoke failed: {last_error}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
