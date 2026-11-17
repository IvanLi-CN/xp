"""Temporary scheduler differential, removed after evidence collection."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import threading
import time
from benchmark import Processes, configs, digest, port, ready, resource

OUT = Path('c8-diagnostic')
OUT.mkdir(exist_ok=True)
XP = Path('candidate/xp-proxy').resolve()
HARNESS = Path('candidate/perf_harness').resolve()
XRAY = Path('fixture/xray-linux-x86_64').resolve()
assert digest(XP) == 'c38f1648b73f005e16660504eda952b9222a12547237966ac45b6fc2f8c1f275'
assert digest(HARNESS) == 'b5ac5650fb8ae16371562eef8227625f35b3bf6b4b4c9cd103da0f7a362e8d1a'
assert digest(XRAY) == 'ba15b5b3658d3b89008d601e730859d06a5db3691320f0130fd91a37bc63412d'
CORES = sorted(os.sched_getaffinity(0))
assert len(CORES) >= 4
rows = []
report = dict(candidate_sha='3ad68b697ad6fe7af11d14bb21e1a9639bdd8405',
              uname=list(os.uname()), cores=CORES, rows=rows,
              note='Diagnostic only; never changes original performance gate')

def stats(pid):
    value = resource(pid)
    if value is None:
        return None
    value['threads'] = {}
    stat = Path(f'/proc/{pid}/stat').read_text().split()
    value['user_ticks'], value['system_ticks'] = int(stat[13]), int(stat[14])
    for task in Path(f'/proc/{pid}/task').glob('*'):
        try:
            fields = (task / 'schedstat').read_text().split()
            value['threads'][task.name] = [int(x) for x in fields]
        except OSError:
            pass
    return value

def pin(process, core):
    # Set every existing thread; new threads inherit the parent's affinity.
    for task in Path(f'/proc/{process.pid}/task').glob('*'):
        try:
            os.sched_setaffinity(int(task.name), {core})
        except ProcessLookupError:
            pass

for mode, repeats in [('history', 4), ('history-one-worker', 4)]:
    for repeat in range(repeats):
        for name in (('candidate', 'Xray') if repeat % 2 == 0 else ('Xray', 'candidate')):
            directory = OUT / f'{mode}-{repeat}-{name}'
            directory.mkdir()
            group = Processes(directory)
            try:
                origin_port, remote, local, fallback = (port() for _ in range(4))
                origin = group.start('origin', [HARNESS, '--mode', 'origin',
                                               '--addr', f'127.0.0.1:{origin_port}'])
                ready(origin_port, origin)
                camouflage = group.start('fallback', ['python3',
                    'scripts/perf/xp-proxy/tls_origin.py', '--port', fallback,
                    '--directory', directory])
                ready(fallback, camouflage)
                server, client, snapshot, _ = configs(directory, origin_port, remote, local, fallback)
                command = ([XP, '--control-socket', directory / 'control.sock',
                            '--state-file', directory / 'state.json', '--snapshot', snapshot]
                           if name == 'candidate' else [XRAY, 'run', '-config', server])
                if name == 'candidate' and mode == 'history-one-worker':
                    command = ['env', 'TOKIO_WORKER_THREADS=1', *command]
                subject = group.start('subject', command)
                ready(remote, subject)
                peer = group.start('peer', [XRAY, 'run', '-config', client])
                ready(local, peer)
                warmup = json.loads(subprocess.check_output([str(HARNESS), '--mode', 'load',
                    '--addr', f'127.0.0.1:{local}', '--requests-per-client', '1',
                    '--duration-ms', '10000'], timeout=12))
                assert not warmup['errors']
                if mode in ('pinned', 'contended'):
                    for proc, core in [(subject, CORES[0]), (peer, CORES[1]),
                                       (origin, CORES[2]), (camouflage, CORES[3])]:
                        pin(proc, core)
                if mode == 'contended':
                    burner = group.start('burner', ['python3', '-c', 'while True: pass'])
                    pin(burner, CORES[0])
                for c, w in [(1, 'latency'), (1, 'upload'), (1, 'download'), (8, 'latency')]:
                    priming = json.loads(subprocess.check_output([str(HARNESS), '--mode', 'load',
                        '--addr', f'127.0.0.1:{local}', '--concurrency', str(c),
                        '--workload', w, '--duration-ms', '2000'], timeout=5))
                    assert not priming['errors']
                    time.sleep(.02)
                telemetry = []
                done = threading.Event()
                def sample():
                    while not done.wait(.025):
                        telemetry.append(dict(t=time.monotonic(),
                            **{k: stats(p.pid) for k, p in [('subject', subject),
                                ('peer', peer), ('origin', origin), ('generator', load)]}))
                cmd = [str(HARNESS), '--mode', 'load', '--addr', f'127.0.0.1:{local}',
                       '--concurrency', '8', '--workload', 'upload', '--duration-ms', '2000']
                if mode in ('pinned', 'contended'):
                    cmd = ['taskset', '-c', str(CORES[3]), *cmd]
                load = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        env={**os.environ, 'TOKIO_WORKER_THREADS': '2'})
                observer = threading.Thread(target=sample)
                observer.start()
                try:
                    stdout, stderr = load.communicate(timeout=5)
                    assert load.returncode == 0, stderr.decode()
                    result = json.loads(stdout)
                finally:
                    done.set()
                    observer.join()
                row = dict(mode=mode, repeat=repeat, program=name, telemetry=telemetry, **result)
                rows.append(row)
                (OUT / 'diagnostic.json').write_text(json.dumps(report))
                print(json.dumps({k:v for k,v in row.items() if k!='telemetry'}), flush=True)
            finally:
                group.stop()
