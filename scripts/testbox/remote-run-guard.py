#!/usr/bin/env python3
"""Own testbox run directories through descriptor-relative operations."""

from __future__ import annotations

import base64
import fcntl
import os
import stat
import sys
from typing import NoReturn


DIRECTORY = getattr(os, "O_DIRECTORY", 0)
NOFOLLOW = getattr(os, "O_NOFOLLOW", 0)
OPEN_FLAGS = os.O_RDONLY | DIRECTORY | NOFOLLOW
SAFE_COMPONENTS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-"


def fail(message: str) -> NoReturn:
    print(message, file=sys.stderr)
    raise SystemExit(2)


def run_parts(root: str, run: str) -> tuple[str, list[str]]:
    root = os.path.normpath(root)
    run = os.path.normpath(run)
    if not os.path.isabs(root) or not os.path.isabs(run):
        fail("testbox paths must be absolute")
    if os.path.realpath(root) != root or os.path.islink(root):
        fail("testbox remote root is not a canonical directory")
    parts = os.path.relpath(run, root).split(os.sep)
    if any(part in ("", ".", "..") for part in parts):
        fail("testbox run path contains an invalid component")
    if not (
        (len(parts) == 4 and parts[0] == "agents" and parts[2] == "runs")
        or (len(parts) == 5 and parts[0] == "workspaces" and parts[3] == "runs")
    ):
        fail("testbox run path is outside the permitted layout")
    if any(any(character not in SAFE_COMPONENTS for character in part) for part in parts):
        fail("testbox run path contains an unsafe component")
    return root, parts


def claim_parts(root: str, claim: str) -> tuple[str, list[str]]:
    root = os.path.normpath(root)
    claim = os.path.normpath(claim)
    if not os.path.isabs(root) or not os.path.isabs(claim):
        fail("testbox paths must be absolute")
    if os.path.realpath(root) != root or os.path.islink(root):
        fail("testbox remote root is not a canonical directory")
    parts = os.path.relpath(claim, root).split(os.sep)
    if len(parts) != 3 or parts[:2] != ["agents", ".shared-testbox-subnet-claims"]:
        fail("testbox claim path is outside the permitted layout")
    if any(
        not part or any(character not in SAFE_COMPONENTS for character in part)
        for part in parts[2:]
    ):
        fail("testbox claim path contains an unsafe component")
    return root, parts


def open_root(root: str) -> int:
    if not NOFOLLOW:
        fail("testbox platform lacks O_NOFOLLOW")
    try:
        return os.open(root, OPEN_FLAGS)
    except OSError as exc:
        fail(f"cannot open testbox remote root safely: {exc}")


def open_parent(root_fd: int, parts: list[str], create: bool) -> int:
    current_fd = root_fd
    try:
        for part in parts:
            try:
                next_fd = os.open(part, OPEN_FLAGS, dir_fd=current_fd)
            except FileNotFoundError:
                if not create:
                    fail("testbox run parent does not exist")
                try:
                    os.mkdir(part, 0o700, dir_fd=current_fd)
                except FileExistsError:
                    pass
                try:
                    next_fd = os.open(part, OPEN_FLAGS, dir_fd=current_fd)
                except OSError as exc:
                    fail(f"testbox run parent is not a safe directory: {exc}")
            except OSError as exc:
                fail(f"testbox run parent is not a safe directory: {exc}")
            os.close(current_fd)
            current_fd = next_fd
        return current_fd
    except BaseException:
        os.close(current_fd)
        raise


def lock_parent(parent_fd: int) -> int:
    try:
        lock_fd = os.open(
            ".codex-run-lock",
            os.O_RDWR | os.O_CREAT | NOFOLLOW,
            0o600,
            dir_fd=parent_fd,
        )
        fcntl.flock(lock_fd, fcntl.LOCK_EX)
        return lock_fd
    except OSError as exc:
        fail(f"cannot lock testbox run parent safely: {exc}")


def remove_tree(parent_fd: int, name: str) -> None:
    directory_fd = os.open(name, OPEN_FLAGS, dir_fd=parent_fd)
    try:
        for entry in os.scandir(directory_fd):
            entry_name = entry.name
            try:
                entry_stat = os.stat(entry_name, dir_fd=directory_fd, follow_symlinks=False)
            except FileNotFoundError:
                continue
            if stat.S_ISDIR(entry_stat.st_mode):
                remove_tree(directory_fd, entry_name)
            else:
                os.unlink(entry_name, dir_fd=directory_fd)
    finally:
        os.close(directory_fd)
    os.rmdir(name, dir_fd=parent_fd)


def marker_matches(run_fd: int, expected: str) -> None:
    if not expected:
        fail("testbox run marker is required")
    try:
        marker_fd = os.open(".codex-run-id", os.O_RDONLY | NOFOLLOW, dir_fd=run_fd)
    except OSError as exc:
        fail(f"cannot open testbox run marker safely: {exc}")
    try:
        marker = os.read(marker_fd, 256).decode("utf-8").strip()
    finally:
        os.close(marker_fd)
    if marker != expected:
        fail("testbox run marker does not match the owning run")


def create_run(root: str, run: str, expected: str) -> None:
    if not expected:
        fail("testbox run marker is required")
    root, parts = run_parts(root, run)
    root_fd = open_root(root)
    parent_fd = open_parent(root_fd, parts[:-1], create=True)
    lock_fd = lock_parent(parent_fd)
    created = False
    try:
        try:
            os.mkdir(parts[-1], 0o700, dir_fd=parent_fd)
            created = True
        except FileExistsError:
            fail("testbox run directory already exists")
        run_fd = os.open(parts[-1], OPEN_FLAGS, dir_fd=parent_fd)
        try:
            marker_fd = os.open(
                ".codex-run-id",
                os.O_WRONLY | os.O_CREAT | os.O_EXCL | NOFOLLOW,
                0o600,
                dir_fd=run_fd,
            )
            try:
                os.write(marker_fd, f"{expected}\n".encode())
                os.fsync(marker_fd)
            finally:
                os.close(marker_fd)
        finally:
            os.close(run_fd)
    except BaseException:
        if created:
            try:
                remove_tree(parent_fd, parts[-1])
            except OSError:
                pass
        raise
    finally:
        os.close(lock_fd)
        os.close(parent_fd)
    print(os.path.normpath(run))


def verify_or_cleanup(root: str, run: str, expected: str, remove: bool) -> None:
    root, parts = run_parts(root, run)
    root_fd = open_root(root)
    parent_fd = open_parent(root_fd, parts[:-1], create=False)
    lock_fd = lock_parent(parent_fd)
    try:
        try:
            run_fd = os.open(parts[-1], OPEN_FLAGS, dir_fd=parent_fd)
        except FileNotFoundError:
            fail("testbox run directory does not exist")
        except OSError as exc:
            fail(f"testbox run directory is not safe: {exc}")
        try:
            marker_matches(run_fd, expected)
            if remove:
                remove_tree(parent_fd, parts[-1])
        finally:
            os.close(run_fd)
    finally:
        os.close(lock_fd)
        os.close(parent_fd)


def cleanup_claim(root: str, claim: str) -> None:
    root, parts = claim_parts(root, claim)
    root_fd = open_root(root)
    parent_fd = open_parent(root_fd, parts[:-1], create=False)
    lock_fd = lock_parent(parent_fd)
    try:
        try:
            remove_tree(parent_fd, parts[-1])
        except FileNotFoundError:
            pass
    finally:
        os.close(lock_fd)
        os.close(parent_fd)


def decode_env_paths() -> tuple[str, str]:
    try:
        root = base64.b64decode(os.environ["TESTBOX_REMOTE_ROOT_B64"]).decode()
        run = base64.b64decode(os.environ["REMOTE_RUN_B64"]).decode()
    except (KeyError, ValueError, UnicodeDecodeError) as exc:
        raise SystemExit(f"invalid encoded testbox path: {exc}") from exc
    return root, run


def expected_marker() -> str:
    encoded_run_id = os.environ.get("RUN_ID_B64")
    if encoded_run_id is None:
        return os.environ.get("RUN_ID", "")
    try:
        return base64.b64decode(encoded_run_id).decode()
    except (ValueError, UnicodeDecodeError) as exc:
        raise SystemExit(f"invalid encoded testbox run marker: {exc}") from exc


def main() -> None:
    if len(sys.argv) == 2:
        mode = sys.argv[1]
        root, run = decode_env_paths()
    elif len(sys.argv) == 4:
        mode, root, run = sys.argv[1:4]
    else:
        raise SystemExit("usage: remote-run-guard.py MODE ROOT RUN")
    expected = expected_marker()
    if mode == "guard":
        create_run(root, run, expected)
    elif mode == "verify":
        verify_or_cleanup(root, run, expected, remove=False)
    elif mode == "cleanup":
        verify_or_cleanup(root, run, expected, remove=True)
    elif mode == "claim-cleanup":
        cleanup_claim(root, run)
    else:
        raise SystemExit(f"unknown testbox guard mode: {mode}")


if __name__ == "__main__":
    main()
