#!/usr/bin/env python3
"""Own testbox run directories through descriptor-relative operations."""

from __future__ import annotations

import base64
import ctypes
import errno
import fcntl
import os
import secrets
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


def validate_directory(directory_fd: int, owner_required: bool, allow_sticky: bool) -> None:
    directory_stat = os.fstat(directory_fd)
    if not stat.S_ISDIR(directory_stat.st_mode):
        fail("testbox path component is not a directory")
    if owner_required:
        if directory_stat.st_uid != os.geteuid():
            fail("testbox path component is not owned by the current user")
    elif directory_stat.st_uid not in (0, os.geteuid()):
        fail("testbox root ancestor is not trusted")
    if directory_stat.st_mode & 0o022 and not (
        allow_sticky and directory_stat.st_mode & stat.S_ISVTX
    ):
        fail("testbox path component is writable by another user")


def root_parts(root: str) -> tuple[str, list[str]]:
    root = os.path.normpath(root)
    if not os.path.isabs(root) or root == os.sep:
        fail("testbox remote root must be a dedicated absolute directory")
    parts = os.path.relpath(root, os.sep).split(os.sep)
    if any(
        not part or any(character not in SAFE_COMPONENTS for character in part)
        for part in parts
    ):
        fail("testbox remote root contains an unsafe component")
    return root, parts


def ensure_root(root: str) -> None:
    root, parts = root_parts(root)
    current_fd = os.open(os.sep, OPEN_FLAGS)
    try:
        for index, part in enumerate(parts):
            try:
                next_fd = os.open(part, OPEN_FLAGS, dir_fd=current_fd)
            except FileNotFoundError:
                try:
                    os.mkdir(part, 0o700, dir_fd=current_fd)
                except FileExistsError:
                    pass
                next_fd = os.open(part, OPEN_FLAGS, dir_fd=current_fd)
            validate_directory(
                next_fd,
                owner_required=index == len(parts) - 1,
                allow_sticky=index != len(parts) - 1,
            )
            os.close(current_fd)
            current_fd = next_fd
    except OSError as exc:
        fail(f"cannot prepare testbox remote root safely: {exc}")
    finally:
        os.close(current_fd)
    print(root)


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
    _, parts = root_parts(root)
    current_fd = os.open(os.sep, OPEN_FLAGS)
    try:
        for index, part in enumerate(parts):
            next_fd = os.open(part, OPEN_FLAGS, dir_fd=current_fd)
            validate_directory(
                next_fd,
                owner_required=index == len(parts) - 1,
                allow_sticky=index != len(parts) - 1,
            )
            os.close(current_fd)
            current_fd = next_fd
        return current_fd
    except OSError as exc:
        os.close(current_fd)
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
            validate_directory(next_fd, owner_required=True, allow_sticky=False)
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
        lock_stat = os.fstat(lock_fd)
        if lock_stat.st_uid != os.geteuid() or lock_stat.st_mode & 0o022:
            fail("testbox run lock is writable by another user")
        return lock_fd
    except OSError as exc:
        fail(f"cannot lock testbox run parent safely: {exc}")


def same_directory(parent_fd: int, name: str, directory_fd: int) -> None:
    expected = os.fstat(directory_fd)
    try:
        actual = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    except OSError as exc:
        fail(f"testbox directory changed during cleanup: {exc}")
    if (
        not stat.S_ISDIR(actual.st_mode)
        or actual.st_dev != expected.st_dev
        or actual.st_ino != expected.st_ino
    ):
        fail("testbox directory changed during cleanup")


def rename_noreplace(parent_fd: int, source: str, target: str) -> bool:
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform == "linux":
        rename = getattr(libc, "renameat2", None)
        flag = 1
    elif sys.platform == "darwin":
        rename = getattr(libc, "renameatx_np", None)
        flag = 4
    else:
        fail("testbox cleanup requires an atomic no-replace rename primitive")
    if rename is None:
        fail("testbox cleanup requires an atomic no-replace rename primitive")
    rename.argtypes = [
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_uint,
    ]
    rename.restype = ctypes.c_int
    result = rename(
        parent_fd,
        os.fsencode(source),
        parent_fd,
        os.fsencode(target),
        flag,
    )
    if result == 0:
        return True
    error = ctypes.get_errno()
    if error == errno.EEXIST:
        return False
    raise OSError(error, os.strerror(error))


def remove_directory_entry(parent_fd: int, name: str, directory_fd: int) -> None:
    same_directory(parent_fd, name, directory_fd)
    for attempt in range(16):
        quarantine = (
            f".codex-run-quarantine-{os.getpid()}-"
            f"{secrets.token_hex(16)}-{attempt}"
        )
        if not rename_noreplace(parent_fd, name, quarantine):
            continue
        try:
            same_directory(parent_fd, quarantine, directory_fd)
            os.rmdir(quarantine, dir_fd=parent_fd)
        except BaseException:
            if not rename_noreplace(parent_fd, quarantine, name):
                fail("cannot restore testbox cleanup quarantine")
            raise
        return
    fail("cannot allocate a testbox cleanup quarantine name")


def restore_marker(run_fd: int, marker_data: bytes) -> None:
    marker_fd = os.open(
        ".codex-run-id",
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | NOFOLLOW,
        0o600,
        dir_fd=run_fd,
    )
    try:
        os.write(marker_fd, marker_data)
        os.fsync(marker_fd)
    finally:
        os.close(marker_fd)


def remove_tree(
    parent_fd: int,
    name: str,
    directory_fd: int | None = None,
    marker_data: bytes | None = None,
) -> None:
    owns_fd = directory_fd is None
    if directory_fd is None:
        directory_fd = os.open(name, OPEN_FLAGS, dir_fd=parent_fd)
    same_directory(parent_fd, name, directory_fd)
    try:
        for entry in os.scandir(directory_fd):
            entry_name = entry.name
            if marker_data is not None and entry_name == ".codex-run-id":
                continue
            try:
                entry_stat = os.stat(entry_name, dir_fd=directory_fd, follow_symlinks=False)
            except FileNotFoundError:
                continue
            if stat.S_ISDIR(entry_stat.st_mode):
                remove_tree(directory_fd, entry_name)
            else:
                os.unlink(entry_name, dir_fd=directory_fd)
    finally:
        same_directory(parent_fd, name, directory_fd)
    try:
        if marker_data is None:
            remove_directory_entry(parent_fd, name, directory_fd)
        else:
            os.unlink(".codex-run-id", dir_fd=directory_fd)
            try:
                remove_directory_entry(parent_fd, name, directory_fd)
            except BaseException:
                restore_marker(directory_fd, marker_data)
                raise
    finally:
        if owns_fd:
            os.close(directory_fd)


def marker_matches(run_fd: int, expected: str) -> bytes:
    if not expected:
        fail("testbox run marker is required")
    try:
        marker_fd = os.open(
            ".codex-run-id",
            os.O_RDONLY | os.O_NONBLOCK | NOFOLLOW,
            dir_fd=run_fd,
        )
    except OSError as exc:
        fail(f"cannot open testbox run marker safely: {exc}")
    try:
        marker_stat = os.fstat(marker_fd)
        if (
            not stat.S_ISREG(marker_stat.st_mode)
            or marker_stat.st_uid != os.geteuid()
            or marker_stat.st_mode & 0o077
        ):
            fail("testbox run marker is not a private regular file")
        marker_data = os.read(marker_fd, 256)
    finally:
        os.close(marker_fd)
    try:
        marker = marker_data.decode("utf-8").strip()
    except UnicodeDecodeError as exc:
        fail(f"testbox run marker is not valid UTF-8: {exc}")
    if marker != expected:
        fail("testbox run marker does not match the owning run")
    return marker_data


def create_run(root: str, run: str, expected: str) -> None:
    if not expected:
        fail("testbox run marker is required")
    root, parts = run_parts(root, run)
    root_fd = open_root(root)
    parent_fd = open_parent(root_fd, parts[:-1], create=True)
    lock_fd = lock_parent(parent_fd)
    created = False
    run_fd: int | None = None
    try:
        try:
            os.mkdir(parts[-1], 0o700, dir_fd=parent_fd)
            created = True
        except FileExistsError:
            fail("testbox run directory already exists")
        run_fd = os.open(parts[-1], OPEN_FLAGS, dir_fd=parent_fd)
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
    except BaseException:
        if created and run_fd is not None:
            try:
                remove_tree(parent_fd, parts[-1], run_fd)
            except OSError:
                pass
        elif created:
            try:
                entry_stat = os.stat(parts[-1], dir_fd=parent_fd, follow_symlinks=False)
                if stat.S_ISDIR(entry_stat.st_mode):
                    os.rmdir(parts[-1], dir_fd=parent_fd)
            except OSError:
                pass
        raise
    finally:
        if run_fd is not None:
            os.close(run_fd)
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
            marker_data = marker_matches(run_fd, expected)
            if remove:
                remove_tree(parent_fd, parts[-1], run_fd, marker_data)
        finally:
            os.close(run_fd)
    finally:
        os.close(lock_fd)
        os.close(parent_fd)


def cleanup_claim(root: str, claim: str, lock: bool) -> None:
    root, parts = claim_parts(root, claim)
    root_fd = open_root(root)
    parent_fd = open_parent(root_fd, parts[:-1], create=False)
    lock_fd = lock_parent(parent_fd) if lock else None
    try:
        try:
            claim_fd = os.open(parts[-1], OPEN_FLAGS, dir_fd=parent_fd)
            try:
                remove_tree(parent_fd, parts[-1], claim_fd)
            finally:
                os.close(claim_fd)
        except FileNotFoundError:
            pass
    finally:
        if lock_fd is not None:
            os.close(lock_fd)
        os.close(parent_fd)


def decode_env_paths() -> tuple[str, str]:
    try:
        root = base64.b64decode(os.environ["TESTBOX_REMOTE_ROOT_B64"]).decode()
        run = base64.b64decode(os.environ["REMOTE_RUN_B64"]).decode()
    except (KeyError, ValueError, UnicodeDecodeError) as exc:
        raise SystemExit(f"invalid encoded testbox path: {exc}") from exc
    return root, run


def decode_env_root() -> str:
    try:
        return base64.b64decode(os.environ["TESTBOX_REMOTE_ROOT_B64"]).decode()
    except (KeyError, ValueError, UnicodeDecodeError) as exc:
        raise SystemExit(f"invalid encoded testbox root: {exc}") from exc


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
        if mode == "ensure-root":
            ensure_root(decode_env_root())
            return
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
        cleanup_claim(root, run, lock=True)
    elif mode == "claim-cleanup-locked":
        cleanup_claim(root, run, lock=False)
    else:
        raise SystemExit(f"unknown testbox guard mode: {mode}")


if __name__ == "__main__":
    main()
