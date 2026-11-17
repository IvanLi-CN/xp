#!/usr/bin/env python3
"""Address-family correctness fixture, not a performance measurement.

Run --peer xray first, then --peer mihomo with --xray-evidence pointing to
the successful same-binary Xray receipt. Never skips unavailable IPv6.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import ipaddress
import json
import socket
import socketserver
import subprocess
import threading
import time
import urllib.request
from pathlib import Path

from benchmark import Processes, configs, port, digest, PUBLIC, SHORT, UUID


def authority(host, number):
    return f"[{host}]:{number}" if ":" in host else f"{host}:{number}"


def receive(stream, size):
    data = bytearray()
    while len(data) < size:
        chunk = stream.recv(size - len(data))
        if not chunk:
            raise RuntimeError(f"truncated payload: {len(data)}/{size}")
        data.extend(chunk)
    return bytes(data)


def ready(host, number, process):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"process exited {process.returncode}")
        try:
            with socket.create_connection((host, number), timeout=.2):
                return
        except OSError:
            time.sleep(.02)
    raise RuntimeError(f"listener unavailable: {authority(host, number)}")


def origin(host):
    records = []
    pause, observed = threading.Event(), threading.Event()
    pause.set()

    class Handler(socketserver.BaseRequestHandler):
        def handle(self):
            self.request.settimeout(10)
            header = self.request.recv(4)
            if not header:  # Readiness probes carry no application payload.
                return
            header += receive(self.request, 4 - len(header))
            size = int.from_bytes(header, "big")
            if not 0 < size <= 1048576:
                raise RuntimeError("invalid fixture payload size")
            upload = receive(self.request, size)
            download = bytes(byte ^ 0xa5 for byte in upload)
            records.append(dict(local=self.request.getsockname(), peer=self.request.getpeername(),
                                upload_sha256=hashlib.sha256(upload).hexdigest(),
                                download_sha256=hashlib.sha256(download).hexdigest()))
            observed.set()
            if not pause.wait(15):
                raise RuntimeError("socket observation barrier timed out")
            self.request.sendall(hashlib.sha256(upload).digest() + download)

    class Server(socketserver.ThreadingTCPServer):
        address_family = socket.AF_INET6 if ":" in host else socket.AF_INET
        daemon_threads = True
        allow_reuse_address = True

        def server_bind(self):
            if host == "::":
                self.socket.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 0)
            super().server_bind()

    server = Server((host, 0), Handler)
    server.pause, server.observed = pause, observed
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, records


def sample(host, number, index):
    upload = bytes((i * 31 + index) % 256 for i in range(128 * 1024))
    download = bytes(byte ^ 0xa5 for byte in upload)
    with socket.create_connection((host, number), timeout=15) as stream:
        stream.sendall(len(upload).to_bytes(4, "big") + upload)
        response = receive(stream, 32 + len(download))
    assert response[:32] == hashlib.sha256(upload).digest(), "upload corruption"
    assert hashlib.sha256(response[32:]).digest() == hashlib.sha256(download).digest(), "download corruption"
    return dict(upload_sha256=hashlib.sha256(upload).hexdigest(),
                download_sha256=hashlib.sha256(download).hexdigest())


def socket_evidence(number, directory, name):
    # Linux ss is authoritative server-side evidence, including IPv4-mapped
    # addresses. Keep raw output rather than guessing the family from config.
    output = subprocess.check_output(["ss", "-lntp", f"sport = :{number}"], text=True)
    (directory / f"{name}-listener.txt").write_text(output)
    return output


def run_case(args, root, transport, destination, direction):
    directory = root / f"{direction}-{transport.replace(':', '_')}-{destination.replace(':', '_')}"
    directory.mkdir()
    group = Processes(directory)
    server, records = origin(destination)
    remote, local, fallback = (port() for _ in range(3))
    try:
        camouflage = group.start("fallback", ["python3", Path(__file__).with_name("tls_origin.py"),
                                             "--port", fallback, "--directory", directory])
        ready("127.0.0.1", fallback, camouflage)
        xserver, xclient, snapshot, outbound = configs(directory, server.server_address[1], remote, local, fallback)
        def edit(path, callback):
            value = json.loads(path.read_text())
            callback(value)
            path.write_text(json.dumps(value))
        edit(snapshot, lambda c: c["listeners"][0].update(bind=f"0.0.0.0:{remote}"))
        edit(xserver, lambda c: c["inbounds"][0].update(listen=transport))
        edit(xclient, lambda c: c["inbounds"][0]["settings"].update(address=destination))
        edit(xclient, lambda c: c["outbounds"][0]["settings"]["vnext"][0].update(address=transport))
        edit(outbound, lambda c: c.update(listen=f"0.0.0.0:{local}", remote=authority(transport, remote), destination_host=destination))
        if direction == "outbound":
            peer = group.start("xray-server", [args.xray, "run", "-config", xserver])
            ready(transport, remote, peer)
            subject = group.start("xp-proxy", [args.xp, "--outbound-config", outbound])
            ready(transport, local, subject)
        else:
            subject = group.start("xp-proxy", [args.xp, "--control-socket", directory / "control.sock",
                                              "--state-file", directory / "state.json", "--snapshot", snapshot])
            ready(transport, remote, subject)
            if args.peer == "xray":
                peer = group.start("xray-client", [args.xray, "run", "-config", xclient])
            else:
                controller = port()
                config = directory / "mihomo.json"
                config.write_text(json.dumps({
                    "mode": "rule", "log-level": "debug", "ipv6": True, "dns": {"enable": False},
                    "external-controller": f"127.0.0.1:{controller}",
                    "tunnels": [{"network": ["tcp"], "address": f"127.0.0.1:{local}",
                                 "target": authority(destination, server.server_address[1]), "proxy": "xp-proxy"}],
                    "proxies": [{"name": "xp-proxy", "type": "vless", "server": transport, "port": remote,
                                 "uuid": UUID, "network": "xhttp", "tls": True, "udp": False, "alpn": ["h2"],
                                 "servername": "test.local", "client-fingerprint": "chrome",
                                 "reality-opts": {"public-key": PUBLIC, "short-id": SHORT},
                                 "xhttp-opts": {"path": "/xp-xhttp", "mode": "stream-one"}}],
                    "rules": ["MATCH,xp-proxy"],
                }))
                peer = group.start("mihomo", [args.mihomo, "-d", directory, "-f", config])
                ready("127.0.0.1", controller, peer)
                request = urllib.request.Request(f"http://127.0.0.1:{controller}/configs", method="PUT",
                                                 data=json.dumps({"path": str(config)}).encode(),
                                                 headers={"Content-Type": "application/json"})
                with urllib.request.urlopen(request, timeout=10) as response:
                    assert response.status == 204
            ready("127.0.0.1", local, peer)
        local_host = transport if direction == "outbound" else "127.0.0.1"
        evidence = socket_evidence(remote, directory, "remote")
        if direction == "outbound":
            socket_evidence(local, directory, "local-outbound")
        samples = []
        # Sequential and concurrent streams, then new local connections over
        # reusable H2 transport; all upload/download hashes must match.
        for count in (1, 8, 1):
            with ThreadPoolExecutor(max_workers=count) as pool:
                samples.extend(pool.map(lambda index: sample(local_host, local, index), range(count)))
        # Capture a live, payload-bearing connection. Some clients close idle
        # transports immediately, so use an origin barrier instead of a sleep.
        server.observed.clear()
        server.pause.clear()
        with ThreadPoolExecutor(max_workers=1) as pool:
            pending = pool.submit(sample, local_host, local, 42)
            try:
                assert server.observed.wait(15), "live payload did not reach origin"
                established = subprocess.check_output(["ss", "-ntp", f"( sport = :{remote} or dport = :{remote} )"], text=True)
            finally:
                server.pause.set()
            samples.append(pending.result())
        (directory / "transport-connections.txt").write_text(established)
        if transport == "::1":
            assert "[::1]" in established, "missing actual IPv6 transport evidence"
        assert len(records) == len(samples), (len(records), len(samples))
        assert all(ipaddress.ip_address(row["local"][0]).version == (6 if ":" in destination else 4) for row in records)
        return dict(direction=direction, transport=transport, destination=destination, samples=samples,
                    origin_connections=records, listener=evidence, established=established)
    finally:
        group.stop()
        server.shutdown()
        server.server_close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xp", type=Path, required=True)
    parser.add_argument("--xray", type=Path, required=True)
    parser.add_argument("--mihomo", type=Path)
    parser.add_argument("--peer", choices=["xray", "mihomo"], required=True)
    parser.add_argument("--xray-evidence", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.xp, args.xray = args.xp.resolve(), args.xray.resolve()
    if args.mihomo:
        args.mihomo = args.mihomo.resolve()
    if args.peer == "mihomo":
        previous = json.loads(args.xray_evidence.read_text())
        assert previous["result"] == "passed" and previous["peer"] == "xray"
        assert previous["xp_sha256"] == digest(args.xp), "Xray evidence belongs to a different binary"
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    result = dict(result="failed", peer=args.peer, xp_sha256=digest(args.xp),
                  xray_sha256=digest(args.xray), cases=[])
    if args.peer == "mihomo":
        result["mihomo_sha256"] = digest(args.mihomo)
    try:
        assert "26.3.27" in subprocess.check_output([args.xray, "version"], text=True)
        if args.peer == "mihomo":
            assert "1.19.29" in subprocess.check_output([args.mihomo, "-v"], text=True)
        for direction in (("inbound", "outbound") if args.peer == "xray" else ("inbound",)):
            for transport in ("127.0.0.1", "::1"):
                for destination in ("127.0.0.1", "::1"):
                    result["cases"].append(run_case(args, output, transport, destination, direction))
        result["result"] = "passed"
    finally:
        (output / "result.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(dict(result=result["result"], cases=len(result["cases"]))))


if __name__ == "__main__":
    main()
