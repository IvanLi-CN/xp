#!/usr/bin/env python3
"""Linux-only native TCP fixtures inside disposable mount/network namespaces.

Requires root, iproute2, util-linux, OpenSSL and an already-built xp-proxy.
Does not change the enclosing VM's hosts file, sysctls or network setup.
"""
import argparse
import hashlib
import json
from pathlib import Path
import socket
import shutil
import ssl
import subprocess
import sys
import time
import tempfile
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "perf" / "xp-proxy"))
from address_family import authority, origin, ready, receive, socket_evidence
from benchmark import Processes, configs, port, UUID


def control(path, operation, request_generation=1, **fields):
    request = json.dumps(dict(protocol_version=2, request_id=str(uuid.uuid4()), generation=request_generation,
                              operation=dict(operation=operation, **fields))).encode()
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(8)
        stream.connect(str(path))
        stream.sendall(len(request).to_bytes(4, "big") + request)
        response = json.loads(receive(stream, int.from_bytes(receive(stream, 4), "big")))
    return response


def raw(stream, host, number, user=UUID):
    try:
        address = socket.inet_pton(socket.AF_INET, host)
        kind = 1
    except OSError:
        try:
            address = socket.inet_pton(socket.AF_INET6, host)
            kind = 3
        except OSError:
            address = bytes([len(host)]) + host.encode()
            kind = 2
    data = bytes(range(256)) * 512
    # Xray may defer the VLESS response header until application data arrives.
    stream.sendall(b"\x00" + uuid.UUID(user).bytes + b"\x00\x01" + number.to_bytes(2, "big") + bytes([kind]) + address + len(data).to_bytes(4, "big") + data)
    assert receive(stream, 2) == b"\x00\x00"
    reply = receive(stream, 32 + len(data))
    assert reply[:32] == hashlib.sha256(data).digest()
    assert reply[32:] == bytes(byte ^ 0xa5 for byte in data)


def run(args):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    subprocess.run(["ip", "link", "set", "lo", "up"], check=True)
    hosts = output / "hosts"
    hosts.write_text("127.0.0.1 localhost xp-v4.test xp-dual.test\n::1 localhost xp-v6.test xp-dual.test\n")
    subprocess.run(["mount", "--bind", str(hosts), "/etc/hosts"], check=True)
    subprocess.run(["sysctl", "-w", "net.ipv6.bindv6only=1"], check=True)
    group = Processes(output)
    origins = []
    results = []
    remote = port()
    _, _, snapshot_path, _ = configs(output, 1, remote, port(), port())
    snapshot = json.loads(snapshot_path.read_text())
    snapshot["listeners"][0]["bind"] = f"0.0.0.0:{remote}"
    snapshot_path.write_text(json.dumps(snapshot))
    sock, state = output / "control.sock", output / "state.json"

    def start(name, snapshot_arg=True):
        command = [args.xp, "--control-socket", sock, "--state-file", state]
        if snapshot_arg:
            command += ["--snapshot", snapshot_path]
        return group.start(name, command)

    def transfer(ingress, target, number, user=UUID):
        with socket.create_connection((ingress, remote), timeout=4) as stream:
            raw(stream, target, number, user)

    def configure(value):
        response = control(sock, "prepare", value["generation"], snapshot=value)
        assert response["error"] is None, response
        return control(sock, "activate", value["generation"])

    try:
        process = start("xp-proxy")
        for host in ["127.0.0.1", "::1"]:
            ready(host, remote, process)
        socket_evidence(remote, output, "bindv6only-one")
        for hostname, bind in [("xp-v4.test", "127.0.0.1"), ("xp-v6.test", "::1"), ("xp-dual.test", "::")]:
            server, records = origin(bind)
            origins.append(server)
            for host in ["127.0.0.1", "::1"]:
                transfer(host, hostname, server.server_address[1])
            assert len(records) == 2
            if hostname == "xp-v6.test":
                assert all(row["local"][0] == "::1" for row in records)
            results.append(dict(case=hostname, connections=records))
        server, _ = origin("::1")
        origins.append(server)
        try:
            transfer("::1", "::1", server.server_address[1], str(uuid.uuid4()))
        except (OSError, RuntimeError, AssertionError):
            results.append(dict(case="bad-uuid", rejected=True))
        else:
            raise AssertionError("wrong credentials accepted")
        held = socket.socket(socket.AF_INET6)
        held.bind(("::1", 0))
        held.listen()
        candidate = json.loads(json.dumps(snapshot))
        candidate["generation"] = 2
        extra = dict(candidate["listeners"][0])
        extra["bind"] = f"0.0.0.0:{held.getsockname()[1]}"
        candidate["listeners"].append(extra)
        response = configure(candidate)
        assert response["error"] and response["generation"] == 1, response
        for host in ["127.0.0.1", "::1"]:
            transfer(host, "::1", server.server_address[1])
        held.close()
        response = control(sock, "activate", 2)
        assert response["error"] is None and response["generation"] == 2, response
        response = control(sock, "rollback", 2, generation=2)
        assert response["error"] is None and response["generation"] == 1, response
        # A live stream remains usable while drain rejects new streams.
        held_stream = socket.create_connection(("::1", remote), timeout=4)
        held_stream.sendall(b"\x00" + uuid.UUID(UUID).bytes + b"\x00\x01" + server.server_address[1].to_bytes(2, "big") + b"\x03" + socket.inet_pton(socket.AF_INET6, "::1"))
        assert receive(held_stream, 2) == b"\x00\x00"
        response = control(sock, "drain")
        assert response["state"] == "draining", response
        try:
            transfer("::1", "::1", server.server_address[1])
        except (OSError, RuntimeError):
            pass
        else:
            raise AssertionError("draining listener accepted a new request")
        data = b"existing-stream"
        held_stream.sendall(len(data).to_bytes(4, "big") + data)
        assert receive(held_stream, 32) == hashlib.sha256(data).digest()
        receive(held_stream, len(data))
        held_stream.close()
        assert control(sock, "stop")["state"] == "stopped"
        held = socket.socket(socket.AF_INET6)
        held.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        held.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 0)
        held.bind(("::", remote))
        held.close()
        process.terminate()
        process.wait(timeout=5)
        # Restart without a replay must remain idle even with persisted state.
        process = start("restart-idle", False)
        for _ in range(100):
            try:
                response = control(sock, "status")
                break
            except (OSError, RuntimeError):
                time.sleep(.02)
        assert response["state"] == "idle", response
        assert configure(snapshot)["error"] is None
        for host in ["127.0.0.1", "::1"]:
            transfer(host, "::1", server.server_address[1])
        results.append(dict(case="conflict-rollback-drain-restart", passed=True))
        assert control(sock, "stop")["error"] is None
        # Exercise configured IPv6 fallback addressing without altering its
        # existing byte-relay / scheme semantics.
        for scheme in ["http", "h2c", "https", "h2"]:
            tls = scheme in ("https", "h2")
            number = port()
            if tls:
                peer = group.start(f"fallback-{scheme}", ["python3", Path(__file__).parents[1] / "perf/xp-proxy/tls_origin.py", "--host", "::1", "--port", number, "--directory", output])
            else:
                peer = group.start(f"fallback-{scheme}", [sys.executable, "-m", "http.server", str(number), "--bind", "::1", "--directory", output])
            ready("::1", number, peer)
            value = json.loads(json.dumps(snapshot))
            value["generation"] = 3 + ["http", "h2c", "https", "h2"].index(scheme)
            target = authority("::1", number)
            value["listeners"][0]["fallbacks"] = [dict(target=f"{scheme}://{target}", authorities=[target])]
            assert configure(value)["error"] is None
            with socket.create_connection(("::1", remote), timeout=4) as stream:
                if tls:
                    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
                    context.check_hostname = False
                    context.verify_mode = ssl.CERT_NONE
                    context.set_alpn_protocols(["h2"])
                    with context.wrap_socket(stream, server_hostname="test.local") as secured:
                        assert secured.selected_alpn_protocol() == "h2"
                else:
                    stream.sendall(b"GET /hosts HTTP/1.0\r\nHost: [::1]\r\n\r\n")
                    assert b"200" in stream.recv(4096)
            results.append(dict(case=f"ipv6-{scheme}-fallback", passed=True))
            peer.terminate()
            peer.wait(timeout=5)
        assert control(sock, "stop")["error"] is None
        subprocess.run(["sysctl", "-w", "net.ipv6.conf.all.disable_ipv6=1"], check=True)
        ipv4_origin, _ = origin("127.0.0.1")
        origins.append(ipv4_origin)
        value = json.loads(json.dumps(snapshot))
        value["generation"] = 7
        assert configure(value)["error"] is None
        transfer("127.0.0.1", "127.0.0.1", ipv4_origin.server_address[1])
        socket_evidence(remote, output, "ipv6-disabled")
        value["generation"] = 8
        value["listeners"][0]["bind"] = authority("::1", remote)
        assert configure(value)["error"] is not None
        transfer("127.0.0.1", "127.0.0.1", ipv4_origin.server_address[1])
        results.append(dict(case="ipv6-disabled-and-explicit-fail-closed", passed=True))
        assert control(sock, "stop")["error"] is None
        # A rejected prepared payload remains retryable by the existing control
        # contract. Restart clears preparation before this independent fixture.
        process.terminate()
        process.wait(timeout=5)
        process = start("restart-v6-only", False)
        for _ in range(100):
            try:
                response = control(sock, "status")
                break
            except (OSError, RuntimeError):
                time.sleep(.02)
        assert response["state"] == "idle", response
        subprocess.run(["sysctl", "-w", "net.ipv6.conf.all.disable_ipv6=0"], check=True)
        subprocess.run(["ip", "addr", "del", "127.0.0.1/8", "dev", "lo"], check=True)
        ipv6_origin, _ = origin("::1")
        origins.append(ipv6_origin)
        value = json.loads(json.dumps(snapshot))
        value["generation"] = 9
        assert configure(value)["error"] is None
        transfer("::1", "::1", ipv6_origin.server_address[1])
        results.append(dict(case="ipv6-only-loopback-network", passed=True))
        subprocess.run(["sysctl", "-w", "net.ipv4.ip_unprivileged_port_start=1024"], check=True)
        with tempfile.TemporaryDirectory(prefix="xp-proxy-ipv6-permission-") as temporary:
            directory = Path(temporary)
            directory.chmod(0o755)
            binary = directory / "xp-proxy"
            shutil.copyfile(args.xp, binary)
            binary.chmod(0o755)
            config = json.loads((output / "outbound.json").read_text())
            config["listen"] = "0.0.0.0:80"
            path = directory / "outbound.json"
            path.write_text(json.dumps(config))
            result = subprocess.run(["setpriv", "--reuid=65534", "--regid=65534", "--clear-groups",
                                     str(binary), "--outbound-config", str(path)],
                                    capture_output=True, text=True, timeout=10)
            assert result.returncode != 0 and "Permission denied" in result.stderr, result
            assert "listener_ipv6_unavailable" not in result.stderr, result
            results.append(dict(case="permission-fails-without-ipv4-retry", error=result.stderr))
        (output / "result.json").write_text(json.dumps(dict(result="passed", xp_sha256=hashlib.sha256(args.xp.read_bytes()).hexdigest(), cases=results), indent=2))
    finally:
        group.stop()
        for server in origins:
            server.shutdown()
            server.server_close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xp", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    args.xp = args.xp.resolve()
    if args.inside:
        run(args)
    else:
        subprocess.run(["unshare", "--net", "--mount", "--fork", "--propagation", "private",
                        sys.executable, str(Path(__file__).resolve()), "--inside", "--xp", str(args.xp), "--output", str(args.output.resolve())], check=True)
