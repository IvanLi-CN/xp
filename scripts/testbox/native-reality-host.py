#!/usr/bin/env python3
"""Run managed ingress restart and upgrade checks on a disposable CI Linux host."""

import argparse
import functools
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

BIN = Path("/usr/local/bin")
DATA = Path("/var/lib/xp/data")
ENV_FILE = Path("/etc/xp/xp.env")
CERTS = DATA / "vless-https-canary"
PORT = 26443
SERVICES = ("xp", "xp-proxy", "xray")
ASSETS = (*SERVICES, "xp-ops", "cloudflared")


def run(*args, **kwargs):
    return subprocess.run(args, timeout=180, check=True, **kwargs)


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def assert_ingress(owner):
    for _ in range(80):
        listeners = run("ss", "-H", "-lntp", f"sport = :{PORT}", capture_output=True, text=True)
        lines = listeners.stdout.splitlines()
        if lines:
            assert len(lines) == 1, lines
            # ss scans process FDs before querying sockets; a new listener may
            # appear between those observations. Require ownership on a later
            # sample within the existing bound, never accept an unknown owner.
            if "users:" not in lines[0]:
                time.sleep(0.25)
                continue
            assert f'("{owner}",' in lines[0], lines
        response = subprocess.run(
            ["curl", "--http2", "--noproxy", "*", "--max-time", "2",
             "--silent", "--cacert", CERTS / "cert.pem", "--resolve",
             f"canary.test:{PORT}:127.0.0.1", "--output", "/dev/null",
             "--write-out", "%{http_code} %{http_version}",
             f"https://canary.test:{PORT}/generate_204"],
            capture_output=True, text=True, timeout=3,
        )
        if lines and response.returncode == 0 and response.stdout == "204 2":
            print(f"{owner}: sole listener, HTTPS 204 over HTTP/2", flush=True)
            return
        time.sleep(0.25)
    raise AssertionError(f"{owner}: ingress did not become ready")


def upgrade_probe(fixture, owner):
    with tempfile.TemporaryDirectory(prefix="xp-managed-release-") as tmp:
        root = Path(tmp)
        downloads = root / "download"
        downloads.mkdir()
        tags = root / "repos/IvanLi-CN/xp/releases/tags"
        tags.mkdir(parents=True)
        for name in ASSETS:
            filename = f"{name}-linux-x86_64"
            source = fixture / (name if name in ("xp", "xp-proxy", "xp-ops") else filename)
            shutil.copyfile(source, downloads / filename)

        handler = functools.partial(SimpleHTTPRequestHandler, directory=root)
        with ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            base = f"http://127.0.0.1:{server.server_port}"
            env = {**os.environ, "XP_OPS_GITHUB_API_BASE_URL": base}

            def publish(tag):
                assets = sorted(downloads.glob("*-linux-x86_64"))
                (downloads / "checksums.txt").write_text(
                    "".join(f"{sha256(path)}  {path.name}\n" for path in assets)
                )
                (tags / f"v{tag}").write_text(json.dumps({
                    "tag_name": f"v{tag}", "prerelease": False,
                    "published_at": "2026-11-18T00:00:00Z",
                    "assets": [
                        {"name": name, "browser_download_url": f"{base}/download/{name}"}
                        for name in ("checksums.txt", *(path.name for path in assets))
                    ],
                }))

            try:
                publish("3.48.99")
                run(BIN / "xp-ops", "upgrade", "--version", "3.48.99", env=env)
                assert_ingress(owner)
                expected = sha256(BIN / owner)
                (downloads / f"{owner}-linux-x86_64").write_bytes(b"#!/bin/sh\nexit 46\n")
                publish("3.48.100")
                failed = subprocess.run(
                    [BIN / "xp-ops", "upgrade", "--version", "3.48.100"],
                    env=env, timeout=180, capture_output=True, text=True,
                )
                print(failed.stdout, failed.stderr, flush=True)
                assert failed.returncode != 0 and "service_error" in failed.stderr, failed
                assert sha256(BIN / owner) == expected, "rollback did not restore the binary"
                assert_ingress(owner)
                print(f"{owner}: upgrade and failed-upgrade rollback passed", flush=True)
            finally:
                server.shutdown()
                thread.join()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manager", choices=("systemd", "openrc"))
    parser.add_argument("fixture", type=Path)
    args = parser.parse_args()
    assert os.environ.get("XP_DISPOSABLE_CI_HOST") == "1" and os.geteuid() == 0
    assert not ENV_FILE.exists() and not DATA.exists(), "refusing to overwrite an XP installation"
    for name in ASSETS:
        assert not (BIN / name).exists(), f"{name} is already installed"

    def service(action, name, **kwargs):
        argv = ("systemctl", action, name) if args.manager == "systemd" else (
            "rc-service", name, action,
        )
        return subprocess.run(argv, timeout=60, **kwargs)

    for name in (*SERVICES, "xp-ops"):
        source = args.fixture / ("xray-linux-x86_64" if name == "xray" else name)
        run("install", "-m", "0755", source, BIN / name)
    run(BIN / "xp-ops", "init", "--init-system", args.manager)
    if args.manager == "systemd":
        run("systemctl", "daemon-reload")
    CERTS.mkdir(parents=True, exist_ok=True)
    access = {
        "XP_ACCESS_HOST": "canary.test",
        "XP_API_BASE_URL": "https://127.0.0.1:26146",
    }
    config = {
        **access, "XP_BIND": "127.0.0.1:26146",
        "XP_DEFAULT_VLESS_PORT": str(PORT), "XP_VLESS_CANARY_BIND": "127.0.0.1:26444",
    }
    ENV_FILE.write_text("".join(f"{key}={value}\n" for key, value in config.items()))
    run("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "60",
        "-subj", "/CN=canary.test", "-addext", "subjectAltName=DNS:canary.test",
        "-keyout", CERTS / "key.pem", "-out", CERTS / "cert.pem", capture_output=True)
    run(BIN / "xp", "init", "--data-dir", DATA, env={**os.environ, **access},
        stdout=subprocess.DEVNULL)
    run("chown", "-R", "xp:xp", DATA)
    try:
        service("start", "xray", check=True)
        service("start", "xp", check=True)
        assert_ingress("xray")
        for owner in ("xray", "xp-proxy"):
            if owner == "xp-proxy":
                run(BIN / "xp-ops", "runtime", "switch", "--to", owner, "--yes")
            assert_ingress(owner)
            service("stop", "xp", check=True)
            service("restart", owner, check=True)
            service("start", "xp", check=True)
            assert_ingress(owner)
            upgrade_probe(args.fixture, owner)
    finally:
        for name in SERVICES:
            service("stop", name)


if __name__ == "__main__":
    main()
