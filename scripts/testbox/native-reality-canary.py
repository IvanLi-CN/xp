#!/usr/bin/env python3
"""Compare ordinary HTTPS through XP-managed native ingress and Xray in an isolated guest.

Usage: python3 native-reality-canary.py /path/to/xp /path/to/xp-proxy /path/to/xray
Uses disposable identities and a local certificate. Requires curl with HTTP/2 and OpenSSL.
"""

import base64
import contextlib
import hashlib
import hmac
import json
import os
from pathlib import Path
import socket
import subprocess as sp
import sys
import tempfile
import time
import uuid


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


xp, proxy, xray = (str(Path(arg).resolve()) for arg in sys.argv[1:])
with tempfile.TemporaryDirectory(prefix="xp-reality-", dir="/tmp") as temp, contextlib.ExitStack() as resources:
    root = Path(temp)
    data = root / "data"
    config = root / "runtime.json"
    config.write_text('{"runtime":"xp-proxy","state":"active"}')
    listener = root / "proxy.sock"
    state = root / "state.json"
    ingress, canary, xp_port, reference = [free_port() for _ in range(4)]
    assert len({ingress, canary, xp_port, reference}) == 4
    host = "canary.test"
    target = f"127.0.0.1:{canary}"
    env = {
        "PATH": os.environ["PATH"], "RUST_LOG": "info",
        "XP_DATA_DIR": str(data), "XP_ACCESS_HOST": host,
        "XP_BIND": f"127.0.0.1:{xp_port}",
        "XP_API_BASE_URL": f"https://127.0.0.1:{xp_port}",
        "XP_DEFAULT_VLESS_PORT": str(ingress), "XP_VLESS_CANARY_BIND": target,
        "XP_NATIVE_PROXY_ENABLED": "true", "XP_NATIVE_PROXY_CONTROL_SOCKET": str(listener),
        "XP_RUNTIME_STATE_FILE": str(config),
    }
    sp.run([xp, "init"], cwd=root, env=env, check=True, timeout=15)
    certdir = data / "vless-https-canary"
    certdir.mkdir(exist_ok=True)
    cert = certdir / "cert.pem"
    sp.run([
        "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "60",
        "-subj", f"/CN={host}", "-addext", f"subjectAltName=DNS:{host}",
        "-keyout", str(certdir / "key.pem"), "-out", str(cert),
    ], check=True, stdout=sp.DEVNULL, stderr=sp.DEVNULL)

    def start(name, *args):
        log = resources.enter_context(open(root / (name + ".log"), "w+"))
        proc = sp.Popen(args, cwd=root, env=env, stdout=log, stderr=log)
        def finish():
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=4)
                except sp.TimeoutExpired:
                    proc.kill()
                    proc.wait()
        resources.callback(finish)
        return proc, log

    def wait(predicate, *processes):
        for _ in range(300):
            if all(proc.poll() is None for proc, _ in processes):
                result = predicate()
                if result:
                    return result
                time.sleep(.1)
            else:
                break
        for _, log in processes:
            log.seek(0)
            print(log.read()[-10000:], file=sys.stderr)
        raise AssertionError("fixture readiness failed")

    def probe(port, version, path="/generate_204", extra=(), hostname=host):
        return sp.run([
            "curl", "--noproxy", "*", "--max-time", "3", "--silent", "--show-error",
            "--cacert", str(cert), "--resolve", f"{hostname}:{port}:127.0.0.1", version,
            "--output", "/dev/null", "--write-out", "%{http_code} %{http_version}",
            *extra, f"https://{hostname}:{port}{path}",
        ], text=True, capture_output=True)

    native = start("proxy", proxy, "--control-socket", str(listener), "--state-file", str(state))
    wait(listener.exists, native)
    xp_proc = start("xp", xp, "run")

    def find_snapshot():
        path = state
        if not path.exists():
            return None
        snapshot = json.loads(path.read_text())
        return next((item for item in snapshot.get("listeners", [])
                     if item.get("reality_tls_target") == target), None)

    snap = wait(find_snapshot, native, xp_proc)
    assert snap["reality"]["dest"] == target and not snap["fallbacks"]
    reference_config = root / "xray.json"
    reference_config.write_text(json.dumps({
        "inbounds": [{"listen": "127.0.0.1", "port": reference, "protocol": "vless",
                      "settings": {"clients": [], "decryption": "none"},
                      "streamSettings": {"network": "xhttp", "security": "reality",
                                         "realitySettings": {
                                             "dest": target,
                                             "serverNames": snap["reality"]["server_names"],
                                             "privateKey": snap["reality"]["private_key"],
                                             "shortIds": snap["reality"]["short_ids"],
                                         },
                                         "xhttpSettings": {"path": snap["path"], "mode": "stream-one"}}}],
        "outbounds": [{"protocol": "freedom"}],
    }))
    print(sp.check_output([xray, "version"], text=True).splitlines()[0])
    ref = start("xray", xray, "run", "-c", str(reference_config))
    wait(lambda: probe(reference, "--http2").stdout == "204 2", ref)
    mesh_path = "/api/admin/_internal/mesh/health"
    cluster = data / "cluster"
    identity = json.loads((cluster / "metadata.json").read_text())
    key_der = sp.check_output(
        ["openssl", "ec", "-in", cluster / "cluster_ca_key.pem", "-outform", "DER"], stderr=sp.DEVNULL
    )
    cert_der = sp.check_output(["openssl", "x509", "-in", cluster / "cluster_ca.pem", "-outform", "DER"])
    prk = hmac.digest(hashlib.sha256(cert_der).digest(), key_der, "sha256")
    subkey = hmac.digest(prk, b"xp/internal-auth-v2/request\x01", "sha256")

    def mesh_headers():
        headers = {
            "x-xp-internal-route": "health-v2",
            "x-xp-cluster-id": identity["cluster_id"],
            "x-xp-sender-id": identity["node_id"],
            "x-xp-target-id": identity["node_id"],
            "x-xp-request-id": uuid.uuid4().hex,
            "x-xp-issued-at": str(int(time.time())),
        }
        fields = list(headers.values())
        message = "\n".join(["v2", fields[0], "GET", mesh_path, "", "0",
                             hashlib.sha256(b"").hexdigest(), *fields[1:]])
        signature = base64.urlsafe_b64encode(hmac.digest(subkey, message.encode(), "sha256")).decode().rstrip("=")
        headers["x-xp-internal-signature"] = "v2:" + signature
        return [item for key, value in headers.items() for item in ("-H", f"{key}: {value}")]

    for route, port in (("canary", canary), ("xp-proxy", ingress), ("xray", reference)):
        for flag, expected in (("--http1.1", "204 1.1"), ("--http2", "204 2")):
            result = probe(port, flag)
            assert (result.returncode, result.stdout) == (0, expected), (route, flag, result)
            print(route, flag, result.stdout, flush=True)
        result = probe(port, "--http2", mesh_path, [*mesh_headers(), "-D", "-"])
        assert result.returncode == 0 and result.stdout.endswith("200 2"), (
            route, result.returncode, result.stdout, result.stderr
        )
        assert "x-xp-internal-ack:" in result.stdout.lower(), (route, "no signed ACK")
        assert probe(port, "--http2", mesh_path).stdout == "404 2", route
        assert probe(port, "--http2", hostname="wrong.test").returncode == 60, route
        print(route, "signed Mesh 200, unsigned Mesh 404, wrong SNI rejected", flush=True)

    with socket.create_connection(("127.0.0.1", ingress), timeout=3) as stream:
        stream.sendall(bytes.fromhex("160303000401000005"))
        stream.shutdown(socket.SHUT_WR)
        assert stream.recv(1) == b"", "malformed TLS did not close"
    xp_proc[0].terminate()
    xp_proc[0].wait(timeout=5)
    assert probe(ingress, "--http2").returncode in (35, 56), "unavailable canary did not fail closed"
    xp_proc = start("xp-restart", xp, "run")
    wait(lambda: probe(ingress, "--http2").stdout == "204 2", native, xp_proc)
    print("xp-proxy malformed TLS and unavailable canary fail closed; XP restart recovers", flush=True)
