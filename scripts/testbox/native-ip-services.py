#!/usr/bin/env python3
"""Exercise installed host services in a disposable, already-bootstrapped Linux guest.

Run xp-ops init/bootstrap first. Uses the official runtime-switch command and
service assets; never run against a production node. Containers use the same
traffic helper separately without advertising unsupported native capability.
"""
import argparse
import hashlib
import json
from pathlib import Path
import socket
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "perf/xp-proxy"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from address_family import origin
from benchmark import configs, port, UUID
import importlib.util

spec = importlib.util.spec_from_file_location("native_ip_faults", Path(__file__).with_name("native-ip-faults.py"))
faults = importlib.util.module_from_spec(spec)
spec.loader.exec_module(faults)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", choices=["systemd", "openrc"], required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    records = []

    def command(argv):
        result = subprocess.run(argv, capture_output=True, text=True, timeout=90)
        records.append(dict(command=argv, code=result.returncode, stdout=result.stdout, stderr=result.stderr))
        if result.returncode:
            raise RuntimeError(records[-1])
        return result.stdout

    def service(action, name):
        command(["systemctl", action, name] if args.backend == "systemd" else ["rc-service", name, action])

    remote = port()
    server, connections = origin("::")
    _, _, snapshot_path, _ = configs(args.output, server.server_address[1], remote, port(), port())
    snapshot = json.loads(snapshot_path.read_text())
    snapshot["listeners"][0]["bind"] = f"0.0.0.0:{remote}"
    control = Path("/run/xp-proxy/xp-proxy.sock")

    def replay(generation):
        snapshot["generation"] = generation
        response = faults.control(control, "prepare", generation, snapshot=snapshot)
        assert response["error"] is None, response
        response = faults.control(control, "activate", generation)
        assert response["state"] == "active" and response["error"] is None, response

    def traffic(owner):
        listening = command(["ss", "-lntp", f"sport = :{remote}"])
        assert len(listening.strip().splitlines()) == 2, listening
        assert f'"{owner}"' in listening, listening
        for ingress in ["127.0.0.1", "::1"]:
            for destination in ["127.0.0.1", "::1"]:
                with socket.create_connection((ingress, remote), timeout=5) as stream:
                    faults.raw(stream, destination, server.server_address[1])

    try:
        command(["xp-ops", "runtime", "switch", "--to", "xp-proxy", "--yes"])
        service("stop", "xp")  # Keep the isolated manually injected snapshot stable.
        replay(1000)
        traffic("xp-proxy")
        service("restart", "xp-proxy")
        for _ in range(100):
            try:
                response = faults.control(control, "status")
                break
            except (OSError, RuntimeError):
                time.sleep(.05)
        assert response["state"] == "idle", response
        replay(1001)
        traffic("xp-proxy")
        # The handoff uses a real Xray listener on the same port, with explicit
        # fixture credentials. No canary, DNS or production settings are used.
        Path("/etc/xray/config.json").write_text(json.dumps(dict(
            log=dict(loglevel="warning"),
            inbounds=[dict(listen="0.0.0.0", port=remote, protocol="vless",
                           settings=dict(clients=[dict(id=UUID)], decryption="none"))],
            outbounds=[dict(protocol="freedom")],
        )))
        command(["xp-ops", "runtime", "switch", "--to", "xray", "--yes"])
        service("stop", "xp")
        traffic("xray")
        command(["xp-ops", "runtime", "switch", "--to", "xp-proxy", "--yes"])
        service("stop", "xp")
        replay(1002)
        traffic("xp-proxy")
        result = dict(result="passed", backend=args.backend, commands=records, origin_connections=connections,
                      xp_sha256=hashlib.sha256(Path("/usr/local/bin/xp").read_bytes()).hexdigest(),
                      xp_proxy_sha256=hashlib.sha256(Path("/usr/local/bin/xp-proxy").read_bytes()).hexdigest())
        (args.output / "result.json").write_text(json.dumps(result, indent=2))
    finally:
        for name in ["xp", "xp-proxy", "xray"]:
            try:
                service("stop", name)
            except Exception:
                pass
        server.shutdown()
        server.server_close()
        (args.output / "commands.json").write_text(json.dumps(records, indent=2))


if __name__ == "__main__":
    main()
