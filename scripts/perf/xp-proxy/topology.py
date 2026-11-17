#!/usr/bin/env python3
"""Real SOCKS destinations over pinned Xray clients and one or many H2 transports."""

import argparse
import asyncio
import hashlib
import json
from pathlib import Path
import socket
import struct
import time
from benchmark import Processes, configs, port, ready


def transports(remote):
    # Count outgoing loopback sockets once, not their accepted server halves.
    paths = [Path('/proc/net/tcp'), Path('/proc/net/tcp6')]
    if not paths[0].exists():
        return None
    return sum(
        int(line.split()[2].split(':')[1], 16) == remote and line.split()[3] == '01'
        for path in paths if path.exists() for line in path.read_text().splitlines()[1:]
    )


async def connect(proxy, target, domain=False):
    reader, writer = await asyncio.open_connection('127.0.0.1', proxy)
    try:
        writer.write(b'\x05\x01\x00')
        await writer.drain()
        if await reader.readexactly(2) != b'\x05\x00':
            raise RuntimeError('SOCKS authentication failed')
        address = b'\x03\x09localhost' if domain else b'\x01\x7f\x00\x00\x01'
        writer.write(b'\x05\x01\x00' + address + struct.pack('!H', target))
        await writer.drain()
        response = await reader.readexactly(4)
        if response[:3] != b'\x05\x00\x00':
            raise ConnectionError('SOCKS target refused')
        size = {1: 4, 4: 16}.get(response[3])
        if response[3] == 3:
            size = (await reader.readexactly(1))[0]
        if size is None:
            raise RuntimeError('invalid SOCKS reply address')
        await reader.readexactly(size + 2)
        return reader, writer
    except BaseException:
        writer.close()
        await writer.wait_closed()
        raise


async def scenario(args, program, clients, mixed, directory):
    directory.mkdir(parents=True, exist_ok=True)
    group = Processes(directory)
    servers, writers, results = [], [], []
    slow_seen, slow_release, hold = asyncio.Event(), asyncio.Event(), asyncio.Event()
    handlers = set()
    targets, identities = [], {}

    async def accept(reader, writer, identity, slow=False, reset=False):
        task = asyncio.current_task()
        handlers.add(task)
        try:
            header = await reader.readexactly(4)
            length = struct.unpack('!I', header)[0]
            if length > 131072:
                raise ValueError('oversized topology payload')
            payload = await reader.readexactly(length)
            if reset:
                writer.transport.abort()
                return
            if slow:
                slow_seen.set()
                await slow_release.wait()
            writer.write(identity + hashlib.sha256(payload).digest() + payload)
            await writer.drain()
            await reader.read()
        except (OSError, asyncio.IncompleteReadError):
            pass
        finally:
            writer.close()
            await writer.wait_closed()
            handlers.discard(task)

    async def origin(identity, **options):
        server = await asyncio.start_server(
            lambda r, w: accept(r, w, identity, **options), '127.0.0.1', 0
        )
        servers.append(server)
        number = server.sockets[0].getsockname()[1]
        identities[number] = identity
        return number

    async def exchange(proxy, target, sequence, keep=False):
        reader, writer = await connect(proxy, target, domain=sequence % 2 == 0)
        writers.append(writer)
        payload = hashlib.sha256(str(sequence).encode()).digest() * 2048
        writer.write(struct.pack('!I', len(payload)) + payload)
        await writer.drain()
        reply = await reader.readexactly(16 + 32 + len(payload))
        if reply != identities[target] + hashlib.sha256(payload).digest() + payload:
            raise RuntimeError('wrong destination or cross-stream payload contamination')
        if keep:
            results.append(sequence)
            await hold.wait()
        writer.close()
        await writer.wait_closed()

    tasks = []
    try:
        for i in range(8):
            targets.append(await origin(f'origin-{i:09d}'.encode()))
        slow = await origin(b'slow-origin-0000', slow=True)
        reset = await origin(b'reset-origin-000', reset=True)
        # Keep a bound non-listening socket so another process cannot claim the refusal target.
        refused = socket.socket()
        refused.bind(('127.0.0.1', 0))
        remote, fallback = port(), port()
        p = group.start('fallback', [
            'python3', Path(__file__).with_name('tls_origin.py'), '--port', fallback,
            '--directory', directory,
        ])
        await asyncio.to_thread(ready, fallback, p)
        server, _, snapshot, _ = configs(directory, targets[0], remote, port(), fallback)
        command = ([args.xray, 'run', '-config', server] if program == 'Xray' else [
            args.xp, '--control-socket', directory / 'control.sock',
            '--state-file', directory / 'state.json', '--snapshot', snapshot,
        ])
        p = group.start('server', command)
        await asyncio.to_thread(ready, remote, p)
        proxies = []
        for i in range(clients):
            client_dir = directory / f'client-{i}'
            client_dir.mkdir()
            local = port()
            _, client, _, _ = configs(client_dir, targets[0], remote, local, fallback)
            config = json.loads(client.read_text())
            config['inbounds'][0] = dict(
                listen='127.0.0.1', port=local, protocol='socks',
                settings=dict(auth='noauth', udp=False),
            )
            client.write_text(json.dumps(config))
            p = group.start(f'client-{i}', [args.xray, 'run', '-config', client])
            await asyncio.to_thread(ready, local, p)
            proxies.append(local)
        # One bounded warmup per transport, no retries of failed business requests.
        await asyncio.wait_for(asyncio.gather(*(
            exchange(proxy, targets[0], 1000 + i) for i, proxy in enumerate(proxies)
        )), 12)
        expected_failures = 0
        if mixed:
            stalled = asyncio.create_task(exchange(proxies[0], slow, 2000))
            tasks.append(stalled)
            await asyncio.wait_for(slow_seen.wait(), 5)
            for target in (refused.getsockname()[1], reset):
                try:
                    await asyncio.wait_for(exchange(proxies[0], target, 2001), 5)
                except (OSError, asyncio.IncompleteReadError):
                    expected_failures += 1
                else:
                    raise AssertionError('failed target unexpectedly returned valid payload')
        for i in range(clients * 16):
            tasks.append(asyncio.create_task(exchange(
                proxies[i % clients], targets[(i // clients) % 8], i, keep=True
            )))
        async def complete():
            while len(results) != clients * 16:
                for task in tasks:
                    if task.done() and task.exception():
                        raise task.exception()
                await asyncio.sleep(0.01)
        await asyncio.wait_for(complete(), 5)
        count = transports(remote)
        if count is not None and count != clients:
            raise AssertionError(f'expected {clients} H2 transports, got {count}')
        # Fast targets finished while the slow target was still held by an event.
        isolated = not mixed or (not stalled.done() and not slow_release.is_set())
        if not isolated:
            raise AssertionError('slow-target barrier was not maintained')
        slow_release.set()
        hold.set()
        await asyncio.wait_for(asyncio.gather(*tasks), 5)
        return dict(program=program, clients=clients, targets=8, streams=len(results),
                    transports=count, mixed=mixed, expected_failures=expected_failures,
                    fast_completed_before_slow_release=isolated, result='success')
    finally:
        slow_release.set()
        hold.set()
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        for writer in writers:
            writer.close()
        group.stop()
        for server in servers:
            server.close()
            await server.wait_closed()
        for task in list(handlers):
            task.cancel()
        await asyncio.gather(*list(handlers), return_exceptions=True)
        if 'refused' in locals():
            refused.close()


async def main(args):
    args.output.mkdir(parents=True, exist_ok=True)
    report = dict(cases=[], result='error')
    try:
        for program in ('Xray', 'xp-proxy'):
            for clients, mixed in ((1, False), (4, False), (4, True)):
                name = f'{program}-{clients}-{mixed}'
                value = await scenario(args, program, clients, mixed, args.output / name)
                report['cases'].append(value)
                print(json.dumps(value), flush=True)
        report['result'] = 'success'
    finally:
        (args.output / 'topology.json').write_text(json.dumps(report, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--xp', type=Path, required=True)
    parser.add_argument('--xray', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    asyncio.run(main(parser.parse_args()))
