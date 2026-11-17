#!/usr/bin/env python3
"""Concurrent OpenSSL TLS flight fixture for the pinned REALITY implementation.

REALITY mirrors individual handshake record lengths. rustls coalesces the TLS
flight into one record, which that pinned implementation cannot mirror. asyncio
uses OpenSSL's separate handshake records without s_server's serial accept loop.
This disposable target supplies TLS/ALPN only, never the measured payload path.
"""

import argparse
import asyncio
from pathlib import Path
import ssl
import subprocess


async def serve(port, directory):
    key, cert = directory / "fixture.key", directory / "fixture.pem"
    subprocess.run(
        [
            "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
            "-keyout", str(key), "-out", str(cert), "-days", "1",
            "-subj", "/CN=test.local",
        ],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_3
    context.load_cert_chain(cert, key)
    context.set_alpn_protocols(["h2"])

    async def accept(reader, writer):
        try:
            while await reader.read(16384):
                pass
        finally:
            writer.close()
            try:
                await writer.wait_closed()
            except (OSError, ssl.SSLError):
                pass

    server = await asyncio.start_server(
        accept, "127.0.0.1", port, ssl=context, ssl_handshake_timeout=3,
    )
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    asyncio.run(serve(args.port, args.directory))
