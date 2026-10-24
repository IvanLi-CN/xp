#!/usr/bin/env python3
"""Validate or remove a testbox run without following replaceable path entries."""

from __future__ import annotations

import os
import shutil
import sys
import base64
from typing import NoReturn


DIRECTORY = getattr(os, "O_DIRECTORY", 0)
NOFOLLOW = getattr(os, "O_NOFOLLOW", 0)
OPEN_FLAGS = os.O_RDONLY | DIRECTORY | NOFOLLOW


def fail(message: str) -> "NoReturn":
    print(message, file=sys.stderr)
    raise SystemExit(2)


def run_parts(root: str, run: str) -> tuple[str, list[str]]:
    root = os.path.normpath(root)
    run = os.path.normpath(run)
    if not os.path.isabs(root) or not os.path.isabs(run):
        fail("testbox paths must be absolute")
    if os.path.realpath(root) != root or os.path.islink(root):
        fail("testbox remote root is not a canonical directory")
    relative = os.path.relpath(run, root)
    parts = relative.split(os.sep)
    if any(part in ("", ".", "..") for part in parts):
        fail("testbox run path contains an invalid component")
    if not (
        len(parts) == 4
        and parts[0] == "agents"
        and parts[2] == "runs"
        or len(parts) == 5
        and parts[0] == "workspaces"
        and parts[3] == "runs"
    ):
        fail("testbox run path is outside the permitted layout")
    safe = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-"
    if any(any(character not in safe for character in part) for part in parts):
        fail("testbox run path contains an unsafe component")
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
                next_fd = os.open(part, OPEN_FLAGS, dir_fd=current_fd)
            except OSError as exc:
                fail(f"testbox run parent is not a safe directory: {exc}")
            os.close(current_fd)
            current_fd = next_fd
        return current_fd
    except BaseException:
        os.close(current_fd)
        raise


def check_run(root: str, run: str, expected: str, remove: bool, create: bool) -> None:
    root, parts = run_parts(root, run)
    root_fd = open_root(root)
    parent_fd = open_parent(root_fd, parts[:-1], create=create)
    try:
        try:
            run_fd = os.open(parts[-1], OPEN_FLAGS, dir_fd=parent_fd)
        except FileNotFoundError:
            if remove:
                fail("testbox run directory does not exist")
            return
        except OSError as exc:
            fail(f"testbox run directory is not safe: {exc}")
        try:
            if expected:
                marker_fd = os.open(".codex-run-id", os.O_RDONLY | NOFOLLOW, dir_fd=run_fd)
                try:
                    marker = os.read(marker_fd, 256).decode("utf-8").strip()
                finally:
                    os.close(marker_fd)
                if marker != expected:
                    fail("testbox run marker does not match the owning run")
            if remove:
                shutil.rmtree(parts[-1], dir_fd=parent_fd)
        finally:
            os.close(run_fd)
    finally:
        os.close(parent_fd)


def main() -> None:
    if len(sys.argv) == 2:
        mode = sys.argv[1]
        try:
            root = base64.b64decode(os.environ["TESTBOX_REMOTE_ROOT_B64"]).decode()
            run = base64.b64decode(os.environ["REMOTE_RUN_B64"]).decode()
        except (KeyError, ValueError, UnicodeDecodeError) as exc:
            raise SystemExit(f"invalid encoded testbox path: {exc}") from exc
    elif len(sys.argv) == 4:
        mode, root, run = sys.argv[1:4]
    else:
        raise SystemExit("usage: remote-run-guard.py MODE ROOT RUN")
    encoded_run_id = os.environ.get("RUN_ID_B64")
    if encoded_run_id is None:
        expected = os.environ.get("RUN_ID", "")
    else:
        try:
            expected = base64.b64decode(encoded_run_id).decode()
        except (ValueError, UnicodeDecodeError) as exc:
            raise SystemExit(f"invalid encoded testbox run marker: {exc}") from exc
    if mode == "guard":
        check_run(root, run, expected, remove=False, create=True)
        print(os.path.normpath(run))
    elif mode == "verify":
        check_run(root, run, expected, remove=False, create=False)
    elif mode == "cleanup":
        check_run(root, run, expected, remove=True, create=False)
    else:
        raise SystemExit(f"unknown testbox guard mode: {mode}")


if __name__ == "__main__":
    main()
