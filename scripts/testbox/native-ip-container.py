#!/usr/bin/env python3
"""Candidate-binary IPv6 smoke in the official image; not container capability enablement."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import socket
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "perf/xp-proxy"))
from address_family import origin
from benchmark import configs, port

spec = importlib.util.spec_from_file_location("native_ip_faults", Path(__file__).with_name("native-ip-faults.py"))
faults = importlib.util.module_from_spec(spec)
spec.loader.exec_module(faults)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xp", type=Path, required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    server, connections = origin("::")
    remote = port()
    _, _, snapshot_path, _ = configs(output, server.server_address[1], remote, port(), port())
    snapshot = json.loads(snapshot_path.read_text())
    snapshot["listeners"][0]["bind"] = f"0.0.0.0:{remote}"
    snapshot_path.write_text(json.dumps(snapshot))
    container = None
    records = []
    try:
        image = json.loads(subprocess.check_output(["docker", "image", "inspect", args.image]))[0]
        container = subprocess.check_output([
            "docker", "run", "-d", "--network", "host", "--no-healthcheck",
            "-v", f"{args.xp.resolve()}:/usr/local/bin/xp-proxy:ro",
            "-v", f"{output}:/fixture", "--entrypoint", "tini", args.image,
            "--", "/usr/local/bin/xp-proxy", "--control-socket", "/fixture/control.sock",
            "--state-file", "/fixture/state.json", "--snapshot", "/fixture/snapshot.json",
        ], text=True).strip()
        for restart in [False, True]:
            if restart:
                subprocess.run(["docker", "restart", container], check=True, capture_output=True, timeout=20)
            for _ in range(100):
                try:
                    response = faults.control(output / "control.sock", "status")
                    if response["state"] == "active":
                        break
                except OSError:
                    pass
                time.sleep(.05)
            else:
                raise RuntimeError("container did not activate explicit fixture snapshot")
            listening = subprocess.check_output(["ss", "-lntp", f"sport = :{remote}"], text=True)
            assert len(listening.strip().splitlines()) == 2 and '"xp-proxy"' in listening, listening
            for ingress in ["127.0.0.1", "::1"]:
                for destination in ["127.0.0.1", "::1"]:
                    with socket.create_connection((ingress, remote), timeout=5) as stream:
                        faults.raw(stream, destination, server.server_address[1])
            records.append(dict(restart=restart, listener=listening))
        actual = subprocess.check_output(["docker", "exec", container, "sha256sum", "/usr/local/bin/xp-proxy"], text=True).split()[0]
        assert actual == hashlib.sha256(args.xp.read_bytes()).hexdigest()
        (output / "result.json").write_text(json.dumps(dict(
            result="passed", scope="candidate binary in official image; native capability remains unsupported",
            image_id=image["Id"], repo_digests=image["RepoDigests"], candidate_sha256=actual,
            samples=records, origin_connections=connections,
        ), indent=2))
    finally:
        if container:
            (output / "container.log").write_bytes(subprocess.check_output(["docker", "logs", container], stderr=subprocess.STDOUT))
            subprocess.run(["docker", "stop", container], check=True, capture_output=True, timeout=20)
            subprocess.run(["docker", "rm", container], check=True, capture_output=True)
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
