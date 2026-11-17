#!/usr/bin/env python3
"""Serial, same-run candidate/main/Xray comparison. No compilation or retries."""

import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import socket
import resource as usage
import subprocess
import threading
import time

ROOT = Path(__file__).resolve().parent
CONFIG = json.loads((ROOT / "config.json").read_text())
# Public, disposable fixture identities. Never production credentials.
KEY = "YHPmunPVXtKbPtqztOXn5vc3PzuCWLUfCfMMjm882Hw"
PUBLIC = "V6BHiLNklUnIQ4CLpkiYtfhQO7zNJZtVfWzSz0muFVE"
UUID = "11111111-1111-4111-8111-111111111111"
SHORT = "0123456789abcdef"
XMUX = dict(
    maxConnections=1,
    maxConcurrency=0,
    cMaxReuseTimes=0,
    hMaxRequestTimes=0,
    hMaxReusableSecs=0,
    hKeepAlivePeriod=-1,
)


def digest(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify(directory, sha, role):
    manifest = json.loads((directory / "manifest.json").read_text())
    expected = dict(
        schema_version=1,
        sha=sha,
        target=CONFIG["target"],
        profile=CONFIG["profile"],
        toolchain=CONFIG["toolchain"],
        config_digest=digest(ROOT / "config.json"),
    )
    for key, value in expected.items():
        if manifest.get(key) != value:
            raise ValueError(f"{role}: manifest mismatch: {key}")
    required = {"xp-proxy", "perf_harness"} if role == "candidate" else {"xp-proxy"}
    if not required.issubset(manifest["checksums"]):
        raise ValueError(f"{role}: missing required binary checksums")
    for name, value in manifest["checksums"].items():
        if Path(name).name != name or digest(directory / name) != value:
            raise ValueError(f"{role}: checksum mismatch: {name}")
    return manifest


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def ready(number, process):
    deadline = time.monotonic() + 4
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"process exited {process.returncode}")
        try:
            with socket.create_connection(("127.0.0.1", number), timeout=0.1):
                return
        except OSError:
            time.sleep(0.01)
    raise RuntimeError("listener not ready")


def resource(pid):
    try:
        stat = Path(f"/proc/{pid}/stat").read_text().split()
        pss = next(
            int(line.split()[1])
            for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines()
            if line.startswith("Pss:")
        )
        return dict(
            cpu_seconds=(int(stat[13]) + int(stat[14])) / os.sysconf("SC_CLK_TCK"),
            pss_kib=pss,
            fd=len(list(Path(f"/proc/{pid}/fd").iterdir())),
        )
    except (OSError, StopIteration):
        return None


class Processes:
    def __init__(self, directory):
        self.directory, self.processes = directory, []

    def start(self, name, command):
        log = open(self.directory / f"{name}.log", "ab", buffering=0)
        process = subprocess.Popen(
            [str(x) for x in command],
            stdout=log,
            stderr=log,
            env={
                **os.environ,
                "GOMEMLIMIT": "48MiB",
                "GOGC": "100",
                "TOKIO_WORKER_THREADS": "2",
            },
        )
        log.close()
        self.processes.append(process)
        return process

    def stop(self):
        for process in reversed(self.processes):
            process.terminate()
        for process in reversed(self.processes):
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def configs(directory, origin, remote, local, fallback):
    def write(name, value):
        path = directory / f"{name}.json"
        path.write_text(json.dumps(value))
        return path

    server_stream = dict(
        network="xhttp",
        security="reality",
        realitySettings=dict(
            dest=f"127.0.0.1:{fallback}",
            serverNames=["test.local"],
            privateKey=KEY,
            shortIds=[SHORT],
        ),
        xhttpSettings=dict(path="/xp-xhttp", mode="stream-one", xmux=XMUX),
    )
    client_stream = dict(
        network="xhttp",
        security="reality",
        realitySettings=dict(
            serverName="test.local",
            publicKey=PUBLIC,
            shortId=SHORT,
            fingerprint="chrome",
        ),
        xhttpSettings=dict(path="/xp-xhttp", mode="stream-one", xmux=XMUX),
    )
    server = write(
        "server",
        dict(
            log=dict(loglevel="warning"),
            inbounds=[
                dict(
                    listen="127.0.0.1",
                    port=remote,
                    protocol="vless",
                    settings=dict(clients=[dict(id=UUID)], decryption="none"),
                    streamSettings=server_stream,
                )
            ],
            outbounds=[dict(protocol="freedom")],
            policy=dict(levels={"0": dict(bufferSize=0)}),
        ),
    )
    client = write(
        "client",
        dict(
            log=dict(loglevel="warning"),
            inbounds=[
                dict(
                    listen="127.0.0.1",
                    port=local,
                    protocol="dokodemo-door",
                    settings=dict(address="127.0.0.1", port=origin, network="tcp"),
                )
            ],
            outbounds=[
                dict(
                    protocol="vless",
                    settings=dict(
                        vnext=[
                            dict(
                                address="127.0.0.1",
                                port=remote,
                                users=[dict(id=UUID, encryption="none")],
                            )
                        ]
                    ),
                    streamSettings=client_stream,
                )
            ],
            policy=dict(levels={"0": dict(bufferSize=0)}),
        ),
    )
    snapshot = write(
        "snapshot",
        dict(
            protocol_version=2,
            generation=1,
            users=[dict(uuid=UUID)],
            listeners=[
                dict(
                    bind=f"127.0.0.1:{remote}",
                    path="/xp-xhttp",
                    reality=dict(
                        dest=f"127.0.0.1:{fallback}",
                        server_names=["test.local"],
                        short_ids=[SHORT],
                        private_key=KEY,
                    ),
                )
            ],
        ),
    )
    outbound = write(
        "outbound",
        dict(
            listen=f"127.0.0.1:{local}",
            remote=f"127.0.0.1:{remote}",
            server_name="test.local",
            server_public_key=PUBLIC,
            short_id=SHORT,
            private_key=PUBLIC,
            path="/xp-xhttp",
            uuid=UUID,
            destination_host="127.0.0.1",
            destination_port=origin,
        ),
    )
    return server, client, snapshot, outbound


def compare(rows):
    failures = []
    cases = itertools.product(
        sorted({r["direction"] for r in rows}),
        sorted({r["concurrency"] for r in rows}),
        ("latency", "upload", "download"),
    )
    for direction, concurrency, workload in cases:
        subset = [
            r
            for r in rows
            if (r["direction"], r["concurrency"], r["workload"])
            == (direction, concurrency, workload)
        ]
        label = f"{direction}/c{concurrency}/{workload}"
        grouped = {
            name: [r for r in subset if r["program"] == name]
            for name in ("candidate", "main", "Xray")
        }
        if any(len(values) != 2 for values in grouped.values()):
            failures.append(f"{label}: missing sweep")
            continue
        for name, values in grouped.items():
            if any(
                r.get("errors")
                or r["samples"] == 0
                or (workload != "latency" and r.get("confirmed_bytes", 0) < 1024 * 1024)
                or (workload == "latency" and r["samples"] < 100)
                or not r["resources_released"]
                for r in values
            ):
                failures.append(f"{label}/{name}: invalid measurement")
        for baseline in ("main", "Xray"):
            # Compare each opposite-order sweep; do not hide a failed round by averaging.
            for a, b in zip(grouped["candidate"], grouped[baseline]):
                if workload == "latency":
                    if any(
                        a[key] is None or b[key] is None or a[key] > b[key] + 5
                        for key in ("p50_ms", "p95_ms")
                    ):
                        failures.append(f"{label}: latency exceeds {baseline} +5ms")
                elif a["mbps"] < b["mbps"] * 0.9:
                    failures.append(f"{label}: throughput below 90% of {baseline}")
    return failures


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--baseline-sha", required=True)
    parser.add_argument("--xray", type=Path, required=True)
    parser.add_argument("--profile", choices=["light", "full"], default="light")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    manifests = {
        "candidate": verify(args.candidate, args.candidate_sha, "candidate"),
        "main": verify(args.baseline, args.baseline_sha, "main"),
    }
    if digest(args.xray) != CONFIG["xray_sha256"]:
        raise ValueError("untrusted Xray fixture")
    start = time.monotonic()
    rows, telemetry, direct = [], [], []
    report = dict(
        schema_version=1,
        profile=args.profile,
        candidate_sha=args.candidate_sha,
        main_sha=args.baseline_sha,
        independent_main=args.candidate_sha != args.baseline_sha,
        manifests=manifests,
        xray=CONFIG,
        environment=dict(
            cpu_count=os.cpu_count(),
            affinity=sorted(os.sched_getaffinity(0)),
            uname=list(os.uname()),
            cpu_allocation="shared runner; independent processes; serial subjects; no exclusive cores",
        ),
        measurement="error",
        rows=rows,
        telemetry=telemetry,
        direct=direct,
    )
    harness = args.candidate / "perf_harness"
    processes = Processes(args.output)
    origin_port, remote, local, fallback = (port() for _ in range(4))

    def load(address, concurrency, workload):
        process = subprocess.Popen(
            [
                str(harness),
                "--mode",
                "load",
                "--addr",
                address,
                "--concurrency",
                str(concurrency),
                "--workload",
                workload,
                "--duration-ms",
                str(CONFIG["slot_ms"]),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env={**os.environ, "TOKIO_WORKER_THREADS": "2"},
        )
        samples = []
        stopped = threading.Event()

        def sample():
            while not stopped.wait(0.05):
                samples.append(
                    dict(
                        t=time.monotonic() - start,
                        generator=resource(process.pid),
                        origin=resource(origin.pid),
                    )
                )

        observer = threading.Thread(target=sample)
        before = usage.getrusage(usage.RUSAGE_CHILDREN)
        observer.start()
        try:
            stdout, stderr = process.communicate(timeout=4)
            if process.returncode:
                raise RuntimeError(stderr.decode())
            result = json.loads(stdout)
            after = usage.getrusage(usage.RUSAGE_CHILDREN)
            result["generator_cpu_seconds"] = (
                after.ru_utime + after.ru_stime - before.ru_utime - before.ru_stime
            )
            result["generator_telemetry"] = samples
            return result
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            stopped.set()
            observer.join()

    try:
        origin = processes.start(
            "origin",
            [harness, "--mode", "origin", "--addr", f"127.0.0.1:{origin_port}"],
        )
        ready(origin_port, origin)
        camouflage = processes.start(
            "fallback",
            ["python3", ROOT / "tls_origin.py", "--port", fallback, "--directory", args.output],
        )
        ready(fallback, camouflage)
        report["crypto_capacity"] = json.loads(
            subprocess.check_output(
                [
                    str(harness),
                    "--mode",
                    "crypto",
                    "--addr",
                    "127.0.0.1:0",
                    "--duration-ms",
                    "1000",
                ],
                timeout=3,
            )
        )
        server, client, snapshot, outbound = configs(
            args.output, origin_port, remote, local, fallback
        )
        for workload in ("latency", "upload", "download"):
            direct.append(
                dict(
                    workload=workload,
                    **load(
                        f"127.0.0.1:{origin_port}", max(CONFIG[args.profile]), workload
                    ),
                )
            )
        for sweep, order in enumerate(
            (("candidate", "main", "Xray"), ("Xray", "main", "candidate"))
        ):
            for direction in (
                ("inbound",) if args.profile == "light" else ("inbound", "outbound")
            ):
                for name in order:
                    group = Processes(args.output)
                    try:
                        binary = (
                            args.xray
                            if name == "Xray"
                            else (
                                args.candidate if name == "candidate" else args.baseline
                            )
                            / "xp-proxy"
                        )
                        if direction == "inbound":
                            command = (
                                [binary, "run", "-config", server]
                                if name == "Xray"
                                else [
                                    binary,
                                    "--control-socket",
                                    args.output / "control.sock",
                                    "--state-file",
                                    args.output / "state.json",
                                    "--snapshot",
                                    snapshot,
                                ]
                            )
                            subject = group.start(f"{name}-{direction}", command)
                            ready(remote, subject)
                            peer = group.start(
                                "xray-client", [args.xray, "run", "-config", client]
                            )
                            ready(local, peer)
                        else:
                            peer = group.start(
                                "xray-server", [args.xray, "run", "-config", server]
                            )
                            ready(remote, peer)
                            subject = group.start(
                                f"{name}-{direction}",
                                [binary, "run", "-config", client]
                                if name == "Xray"
                                else [binary, "--outbound-config", outbound],
                            )
                            ready(local, subject)
                        # Warm the H2 transport and observe its legitimate idle sockets.
                        warmup = json.loads(
                            subprocess.check_output(
                                [
                                    str(harness),
                                    "--mode",
                                    "load",
                                    "--addr",
                                    f"127.0.0.1:{local}",
                                    "--concurrency",
                                    "1",
                                    "--workload",
                                    "latency",
                                    "--duration-ms",
                                    "300",
                                ],
                                timeout=3,
                            )
                        )
                        time.sleep(0.02)
                        idle = resource(subject.pid)
                        for concurrency, workload in itertools.product(
                            CONFIG[args.profile], ("latency", "upload", "download")
                        ):
                            done = threading.Event()

                            def observe():
                                while not done.wait(0.05):
                                    telemetry.append(
                                        dict(
                                            t=time.monotonic() - start,
                                            program=name,
                                            direction=direction,
                                            subject=resource(subject.pid),
                                            peer=resource(peer.pid),
                                            origin=resource(origin.pid),
                                        )
                                    )

                            monitor = threading.Thread(target=observe)
                            monitor.start()
                            try:
                                result = load(
                                    f"127.0.0.1:{local}", concurrency, workload
                                )
                            except Exception as error:
                                result = dict(
                                    samples=0,
                                    mbps=0,
                                    p50_ms=None,
                                    p95_ms=None,
                                    errors=[str(error)],
                                )
                            finally:
                                done.set()
                                monitor.join()
                            # Idle H2 transport is legitimate. Extra per-stream sockets must disappear.
                            cleanup_deadline = time.monotonic() + 0.15
                            after = resource(subject.pid)
                            while (
                                after is not None
                                and idle is not None
                                and after["fd"] > idle["fd"]
                                and time.monotonic() < cleanup_deadline
                            ):
                                time.sleep(0.005)
                                after = resource(subject.pid)
                            result["resources_released"] = (
                                after is not None
                                and idle is not None
                                and after["fd"] <= idle["fd"]
                            )
                            result["warmup_errors"] = warmup["errors"]
                            rows.append(
                                dict(
                                    program=name,
                                    direction=direction,
                                    concurrency=concurrency,
                                    workload=workload,
                                    sweep=sweep,
                                    idle=idle,
                                    after=after,
                                    **result,
                                )
                            )
                    finally:
                        group.stop()
        failures = compare(rows)
        for row in rows:
            control = next(r for r in direct if r["workload"] == row["workload"])
            if control["errors"] or (
                row["workload"] != "latency" and control["mbps"] < row["mbps"] * 1.2
            ):
                failures.append("direct capacity has insufficient headroom")
        report.update(
            measurement="failure" if failures else "success", failures=failures
        )
    except Exception as error:
        report["error"] = str(error)
    finally:
        processes.stop()
        report["execute_seconds"] = time.monotonic() - start
        (args.output / "measurement.json").write_text(json.dumps(report, indent=2))
        summary = f"## xp-proxy performance ({args.profile})\n\nMeasurement: **{report['measurement']}**\n\nCandidate: `{args.candidate_sha}`; main: `{args.baseline_sha}`.\n\n"
        summary += "|Program|Direction|Concurrency|Load|Sweep|Samples|Mbps|p50 ms|p95 ms|Errors|\n|---|---|---:|---|---:|---:|---:|---:|---:|---:|\n"
        for row in rows:
            summary += f"|{row['program'].replace('candidate', 'xp-proxy (candidate)').replace('main', 'xp-proxy (main)')}|{row['direction']}|{row['concurrency']}|{row['workload']}|{row['sweep']}|{row['samples']}|{row['mbps']:.2f}|{row['p50_ms']}|{row['p95_ms']}|{len(row['errors'])}|\n"
        summary += "\n" + "\n".join(
            f"- {failure}" for failure in report.get("failures", [])
        )
        (args.output / "summary.md").write_text(summary)
    return 0 if report["measurement"] == "success" else 1


if __name__ == "__main__":
    raise SystemExit(main())
