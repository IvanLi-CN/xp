#!/usr/bin/env python3
"""Single-variable Xray scaling diagnostics, never substituted for formal results."""

import argparse
import json
import subprocess
import threading
import time
from pathlib import Path
from benchmark import Processes, configs, port, ready, resource


def main(args):
    args.output.mkdir(parents=True, exist_ok=True)
    rows = []
    variants = ['control16', 'control64', 'subject-memory', 'peer-memory', 'pool4', 'long-slot']
    try:
        for sweep in range(2):
            for direction in ('inbound', 'outbound'):
                for variant in (variants if sweep == 0 else list(reversed(variants))):
                    directory = args.output / f'{sweep}-{direction}-{variant}'
                    directory.mkdir()
                    group = Processes(directory)
                    try:
                        origin, remote, local, fallback = (port() for _ in range(4))
                        p = group.start('origin', [args.harness, '--mode', 'origin',
                                                '--addr', f'127.0.0.1:{origin}'])
                        ready(origin, p)
                        p = group.start('fallback', ['python3', Path(__file__).with_name(
                            'tls_origin.py'), '--port', fallback, '--directory', directory])
                        ready(fallback, p)
                        server, client, _, _ = configs(directory, origin, remote, local, fallback)
                        if variant == 'pool4':
                            value = json.loads(client.read_text())
                            value['outbounds'][0]['streamSettings']['xhttpSettings']['xmux'][
                                'maxConnections'] = 4
                            client.write_text(json.dumps(value))
                        subject_memory = '128MiB' if variant == 'subject-memory' else '48MiB'
                        peer_memory = '128MiB' if variant == 'peer-memory' else '48MiB'
                        processes = {}
                        for role, config, number in (
                            ('server', server, remote), ('client', client, local)
                        ):
                            subject_role = 'server' if direction == 'inbound' else 'client'
                            memory = subject_memory if role == subject_role else peer_memory
                            processes[role] = group.start(role, [args.xray, 'run', '-config', config],
                                                          env={'GOMEMLIMIT': memory,
                                                               'GODEBUG': 'gctrace=1'})
                            ready(number, processes[role])
                        subject = processes[subject_role]
                        peer = processes['client' if subject_role == 'server' else 'server']
                        base = [str(args.harness), '--mode', 'load', '--addr', f'127.0.0.1:{local}']
                        warmup = json.loads(subprocess.check_output(base + [
                            '--concurrency', '1', '--duration-ms', '10000',
                            '--requests-per-client', '1', '--workload', 'latency'], timeout=12))
                        if warmup['errors']:
                            raise RuntimeError('diagnostic warmup failed')
                        for workload in ('upload', 'download'):
                            samples, done = [], threading.Event()
                            def observe():
                                while not done.wait(0.05):
                                    samples.append(dict(t=time.monotonic(), subject=resource(
                                        subject.pid), peer=resource(peer.pid)))
                            watcher = threading.Thread(target=observe)
                            watcher.start()
                            try:
                                result = json.loads(subprocess.check_output(base + [
                                    '--concurrency', '16' if variant == 'control16' else '64',
                                    '--duration-ms', '6000' if variant == 'long-slot' else '2000',
                                    '--workload', workload], timeout=8))
                            finally:
                                done.set()
                                watcher.join()
                            row = dict(sweep=sweep, direction=direction, variant=variant,
                                       subject_memory=subject_memory, peer_memory=peer_memory,
                                       workload=workload, telemetry=samples, **result)
                            rows.append(row)
                            print(json.dumps({k:v for k,v in row.items() if k not in (
                                'telemetry',)}), flush=True)
                    finally:
                        group.stop()
    finally:
        (args.output / 'scaling.json').write_text(json.dumps(dict(
            diagnostic_only=True, rows=rows, cases_expected=48,
            result='success' if len(rows) == 48 and all(not r['errors'] for r in rows)
            else 'incomplete_or_failed',
        ), indent=2))
    if len(rows) != 48 or any(r['errors'] for r in rows):
        raise SystemExit(1)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--harness', type=Path, required=True)
    parser.add_argument('--xray', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    main(parser.parse_args())
