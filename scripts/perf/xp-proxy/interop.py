#!/usr/bin/env python3
"""Real Xray first, Mihomo second. This does not replace performance acceptance."""

import argparse
import json
from pathlib import Path
import subprocess
from benchmark import Processes, configs, port, ready, PUBLIC, SHORT, UUID


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--xp", type=Path, required=True)
    parser.add_argument("--harness", type=Path, required=True)
    parser.add_argument("--xray", type=Path, required=True)
    parser.add_argument("--mihomo", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    processes = Processes(args.output)
    origin_port, remote, local, fallback = (port() for _ in range(4))
    results = []
    try:
        origin = processes.start(
            "origin",
            [args.harness, "--mode", "origin", "--addr", f"127.0.0.1:{origin_port}"],
        )
        ready(origin_port, origin)
        camouflage = processes.start(
            "fallback",
            ["python3", Path(__file__).with_name("tls_origin.py"), "--port", fallback, "--directory", args.output],
        )
        ready(fallback, camouflage)
        server, client, snapshot, outbound = configs(
            args.output, origin_port, remote, local, fallback
        )
        if args.mihomo:
            # Same pinned fields as tests/fixtures/mihomo-smux-v1.19.29.yaml.
            mihomo = dict(
                mode="rule",
                **{"log-level": "warning", "ipv6": False, "dns": {"enable": False}},
            )
            mihomo["tunnels"] = [
                dict(
                    network=["tcp"],
                    address=f"127.0.0.1:{local}",
                    target=f"127.0.0.1:{origin_port}",
                    proxy="xp-proxy",
                )
            ]
            mihomo["proxies"] = [
                dict(
                    name="xp-proxy",
                    type="vless",
                    server="127.0.0.1",
                    port=remote,
                    uuid=UUID,
                    network="xhttp",
                    tls=True,
                    udp=False,
                    alpn=["h2"],
                    servername="test.local",
                    **{
                        "client-fingerprint": "chrome",
                        "reality-opts": {"public-key": PUBLIC, "short-id": SHORT},
                        "xhttp-opts": {
                            "path": "/xp-xhttp",
                            "mode": "stream-one",
                            "reuse-settings": {
                                "max-connections": "1",
                                "max-concurrency": "0",
                                "c-max-reuse-times": "0",
                                "h-max-request-times": "0",
                                "h-max-reusable-secs": "0",
                                "h-keep-alive-period": -1,
                            },
                        },
                    },
                )
            ]
            mihomo["rules"] = ["MATCH,xp-proxy"]
            config = args.output / "mihomo.json"
            config.write_text(json.dumps(mihomo))
            subprocess.run(
                [str(args.mihomo), "-t", "-d", str(args.output), "-f", str(config)],
                check=True,
            )
        directions = ("inbound",) if args.mihomo else ("inbound", "outbound")
        for direction in directions:
            group = Processes(args.output)
            try:
                if direction == "inbound":
                    subject = group.start(
                        "xp-proxy",
                        [
                            args.xp,
                            "--control-socket",
                            args.output / "control.sock",
                            "--state-file",
                            args.output / "state.json",
                            "--snapshot",
                            snapshot,
                        ],
                    )
                    ready(remote, subject)
                    command = (
                        [
                            args.mihomo,
                            "-d",
                            args.output,
                            "-f",
                            args.output / "mihomo.json",
                        ]
                        if args.mihomo
                        else [args.xray, "run", "-config", client]
                    )
                    peer = group.start("client", command)
                    ready(local, peer)
                else:
                    peer = group.start("server", [args.xray, "run", "-config", server])
                    ready(remote, peer)
                    subject = group.start(
                        "xp-proxy", [args.xp, "--outbound-config", outbound]
                    )
                    ready(local, subject)
                for concurrency in (1, 8, 64):
                    for workload in ("latency", "upload", "download"):
                        result = json.loads(
                            subprocess.check_output(
                                [
                                    str(args.harness),
                                    "--mode",
                                    "load",
                                    "--addr",
                                    f"127.0.0.1:{local}",
                                    "--concurrency",
                                    str(concurrency),
                                    "--duration-ms",
                                    "10000",
                                    "--requests-per-client",
                                    "1",
                                    "--bulk-frame-bytes",
                                    "1048576",
                                    "--workload",
                                    workload,
                                ],
                                timeout=12,
                            )
                        )
                        results.append(
                            dict(
                                direction=direction,
                                concurrency=concurrency,
                                workload=workload,
                                **result,
                            )
                        )
                        if result["errors"] or not result["samples"]:
                            raise RuntimeError(f"interop failed: {results[-1]}")
            finally:
                group.stop()
    finally:
        processes.stop()
        (args.output / "interop.json").write_text(json.dumps(results, indent=2))
    print(
        json.dumps(
            dict(
                result="success",
                peer="Mihomo v1.19.29" if args.mihomo else "Xray 26.3.27",
                cases=len(results),
            )
        )
    )


if __name__ == "__main__":
    main()
