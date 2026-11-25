#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Runs serial offline fixtures against an owned disposable Redis process.

Requires native redis-server and prebuilt engine/memory binaries. Uses portable path
handling; low-priority execution requires POSIX (macOS/Linux, or WSL). Never point the
examples at a real trading Redis instance.

"""

import argparse
import hashlib
import itertools
import json
import os
import pathlib
import shutil
import signal
import socket
import subprocess
import sys
import time
from types import FrameType
from typing import NoReturn


MIN_EPHEMERAL_PORT = 20_000
MAX_REDIS_REPLY = 65_536
LONG_HISTORY = 36_000
DUPLICATE_RELOAD_EXIT = 2


parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
parser.add_argument("--binary-dir", type=pathlib.Path, required=True)
parser.add_argument("--output-dir", type=pathlib.Path, required=True)
parser.add_argument("--baseline-allocations", type=pathlib.Path)
parser.add_argument("--head-allocations", type=pathlib.Path)
parser.add_argument("--redis-server", default=shutil.which("redis-server"))
parser.add_argument(
    "--legacy-duplicate-reload",
    choices=("baseline", "head", "both"),
    help="Expect the historical nonvirtual duplicate reload failure for these older binaries only",
)
parser.add_argument(
    "--smoke",
    action="store_true",
    help="Run only small correctness fixtures and memory probes",
)
options = parser.parse_args()

ALLOCATIONS = {
    "baseline": options.baseline_allocations,
    "head": options.head_allocations,
}

if any(ALLOCATIONS.values()) and not all(ALLOCATIONS.values()):
    parser.error("supply both --baseline-allocations and --head-allocations")

for variant, executable in ALLOCATIONS.items():
    if executable is not None:
        ALLOCATIONS[variant] = executable.resolve()
        if not executable.is_file():
            parser.error(f"missing allocation binary: {variant}")

if not options.redis_server:
    parser.error("redis-server is not on PATH; supply --redis-server")

if not hasattr(os, "setpriority"):
    parser.error("low-priority execution requires POSIX; use Linux/WSL on Windows")
os.setpriority(os.PRIO_PROCESS, 0, max(15, os.getpriority(os.PRIO_PROCESS, 0)))
BIN = options.binary_dir.resolve()

for variant in ("head", "baseline"):
    for target in ("engine", "memory"):
        if not (BIN / f"{variant}-{target}").is_file():
            parser.error(f"missing binary: {variant}-{target}")
ROOT = options.output_dir.resolve()
ROOT.mkdir(parents=True, exist_ok=False)
ENV = dict(os.environ, NAUTILUS_WORKER_THREADS="1")
(ROOT / "provenance.json").write_text(
    json.dumps(
        {
            "smoke": options.smoke,
            "legacy_duplicate_reload": options.legacy_duplicate_reload,
            "allocation_binaries": {
                variant: hashlib.sha256(path.read_bytes()).hexdigest()
                for variant, path in ALLOCATIONS.items()
                if path is not None
            },
            "nice": os.getpriority(os.PRIO_PROCESS, 0),
            "binaries": {
                p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                for p in (
                    BIN / f"{v}-{t}" for v in ("head", "baseline") for t in ("engine", "memory")
                )
            },
        },
        indent=2,
    ),
)


def stop_on_termination(signum: int, _frame: FrameType | None) -> NoReturn:
    """
    Exit through cleanup when the runner receives SIGTERM.
    """
    raise SystemExit(128 + signum)


signal.signal(signal.SIGTERM, stop_on_termination)


def redis_command(port: int, *args: str) -> bytes | int:
    """
    Read one bounded Redis reply without changing database contents.
    """
    command = f"*{len(args)}\r\n".encode()
    for arg in args:
        encoded = arg.encode()
        command += f"${len(encoded)}\r\n".encode() + encoded + b"\r\n"
    with socket.create_connection(("127.0.0.1", port), timeout=0.2) as sock:
        sock.sendall(command)
        with sock.makefile("rb") as stream:
            header = stream.readline(4096)
            if header.startswith(b":"):
                return int(header[1:])

            if not header.startswith(b"$"):
                raise RuntimeError("Unexpected Redis response")
            length = int(header[1:])
            if not 0 <= length <= MAX_REDIS_REPLY:
                raise RuntimeError("Invalid Redis response length")
            payload = stream.read(length)
            if len(payload) != length or stream.read(2) != b"\r\n":
                raise RuntimeError("Incomplete Redis response")
            return payload


def stop_child(child: subprocess.Popen) -> None:
    """
    Reap only the child process created by this runner.
    """
    if child.poll() is None:
        child.terminate()
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=10)


def run_fixture(binary: str, args: list[str | int], tag: str) -> int:
    """
    Execute one fixture; subprocess.run reaps it on timeout or interruption.
    """
    started = time.monotonic()
    with (
        (ROOT / (tag + ".out")).open("x") as out,
        (ROOT / (tag + ".err")).open("x") as err,
    ):
        process = subprocess.run(  # noqa: S603 (explicit local binaries; no shell)
            [str(BIN / binary), *map(str, args)],
            stdout=out,
            stderr=err,
            env=ENV,
            timeout=180,
            check=False,
        )
    time.sleep(max(3, min(30, time.monotonic() - started)))
    return process.returncode


seen_run_ids: set[str] = set()


def redis_identity(port: int, pid: int) -> str:
    """
    Verify ownership before trusting the server instance identifier.
    """
    payload = redis_command(port, "INFO", "server")
    if not isinstance(payload, bytes):
        raise TypeError("Unexpected Redis INFO response")
    info = dict(line.split(b":", 1) for line in payload.splitlines() if b":" in line)
    if info.get(b"process_id") != str(pid).encode():
        raise RuntimeError("Redis port is not owned by the process we launched")
    run_id = info.get(b"run_id", b"").decode("ascii")
    if not run_id:
        raise RuntimeError("Missing Redis run_id")
    return run_id


def await_ready(server: subprocess.Popen, port: int, metadata: dict[str, object]) -> None:
    """
    Verify a fresh owned empty server before any fixture timing starts.
    """
    for _ in range(100):
        if server.poll() is not None:
            raise RuntimeError("Private Redis exited")
        try:
            run_id = redis_identity(port, server.pid)
            if run_id in seen_run_ids:
                raise RuntimeError("Reused Redis run_id")
            size = redis_command(port, "DBSIZE")
            metadata.update(run_id=run_id, initial_dbsize=size)
            if size != 0:
                raise RuntimeError("Private Redis database is not empty")

            if redis_identity(port, server.pid) != run_id or server.poll() is not None:
                raise RuntimeError("Redis identity changed during readiness")
            seen_run_ids.add(run_id)
            metadata["ready"] = True
        except OSError:
            time.sleep(0.05)
        else:
            return
    raise RuntimeError("Private Redis not ready")


def run(binary: str, args: list[str | int], tag: str) -> int:
    """
    Give each engine invocation its own verified empty Redis process.
    """
    if binary.endswith("-memory") or (args and args[1] == "memory"):
        return run_fixture(binary, args, tag)
    directory = ROOT / tag
    directory.mkdir(exist_ok=False)
    metadata: dict[str, object] = {"tag": tag, "directory": directory.name, "ready": False}
    server = None
    with (directory / "redis.log").open("x") as log:
        try:
            with socket.socket() as sock:
                sock.bind(("127.0.0.1", 0))
                port = sock.getsockname()[1]

            if port < MIN_EPHEMERAL_PORT:
                raise RuntimeError("Allocated port is outside the fixture port range")

            server = subprocess.Popen(  # noqa: S603 (explicit local Redis executable; no shell)
                [
                    options.redis_server,
                    "--bind",
                    "127.0.0.1",
                    "--port",
                    str(port),
                    "--save",
                    "",
                    "--appendonly",
                    "no",
                    "--protected-mode",
                    "yes",
                    "--dir",
                    str(directory),
                ],
                stdout=log,
                stderr=subprocess.STDOUT,
            )

            metadata.update(pid=server.pid, port=port)
            await_ready(server, port, metadata)
            fixture_args = list(args)
            fixture_args[2] = port
            code = run_fixture(binary, fixture_args, tag)
            metadata["fixture_exit"] = code
            return code
        finally:
            try:
                if server is not None:
                    stop_child(server)
                    metadata["redis_exit"] = server.returncode
            finally:
                (directory / "redis-metadata.json").write_text(json.dumps(metadata, indent=2))


def record_reopen_result(
    results: list[dict[str, object]],
    tag: str,
    code: int,
    fixture: tuple[int, str, str, str],
) -> None:
    """
    Keep the independently reproduced recovery failure distinct from passing samples.
    """
    n, backend, mode, first = fixture
    result: dict[str, object] = {"tag": tag, "code": code, "expected": 0, "outcome": "passed"}
    if backend == "redis-nonvirtual" and mode == "reopen-carry" and first == "unique-only":
        diagnostic = (ROOT / (tag + ".err")).read_text()
        marker = (
            "RELOAD_HISTORY_MISMATCH economics_equal=true other_fields_equal=true "
            f"expected_replay_events={n + 2} actual_replay_events=1"
        )

        if (
            code == -signal.SIGABRT
            and marker in diagnostic.splitlines()
            and "Redis lost complete replay state" in diagnostic
        ):
            result.update(
                expected=code,
                outcome="known_recovery_failure",
                economics_equal=True,
                other_fields_equal=True,
                replay_history_equal=False,
            )
    results.append(result)
    if code != result["expected"]:
        raise RuntimeError(f"{tag}: unexpected exit {code}")


results = []
try:
    # Full state checks and duplicates are separate from measured normal-flow runs.
    for variant, n, backend, start, first, mode in itertools.product(
        ["head", "baseline"],
        ([8] if options.smoke else [8, 36000]),
        ["redis-nonvirtual", "redis-virtual"],
        ["cold", "warm"],
        ["unique-only", "duplicate"],
        ["flip", "continue"],
    ):
        if mode == "continue" and first != "unique-only":
            continue
        tag = f"check-{variant}-{n}-{backend}-{start}-{first}-{mode}"
        args = [n, backend, 0, start, first, 3, "full", mode]
        code = run(variant + "-engine", args, tag)
        expected = (
            DUPLICATE_RELOAD_EXIT
            if backend == "redis-nonvirtual"
            and first == "duplicate"
            and options.legacy_duplicate_reload in (variant, "both")
            else 0
        )
        results.append({"tag": tag, "code": code, "expected": expected})
        if code != expected:
            raise RuntimeError(f"{tag}: unexpected exit {code}, expected {expected}")

        if expected == DUPLICATE_RELOAD_EXIT:
            error = (ROOT / (tag + ".err")).read_text()

            if (
                "RELOAD_ERROR Err(Duplicate fill event for position AUDUSD-REPEAT-FLIP: T-FLIP-2)"
                not in error
            ):
                raise RuntimeError(
                    f"{tag}: reload failure did not match the known duplicate diagnostic",
                )

    for round_id in range(0 if options.smoke else 3):
        variants = ["baseline", "head"] if round_id % 2 == 0 else ["head", "baseline"]
        for n, backend, start, mode in itertools.product(
            [8, 64, 36000],
            ["redis-nonvirtual", "redis-virtual"],
            ["cold", "warm"],
            ["continue", "flip"],
        ):
            for variant in variants:
                count = (20 if n == LONG_HISTORY else 100) if mode == "continue" else 5
                tag = f"measure-r{round_id}-{variant}-{n}-{backend}-{start}-{mode}"
                code = run(
                    variant + "-engine",
                    [n, backend, 0, start, "unique-only", count, "economics", mode],
                    tag,
                )
                results.append({"tag": tag, "code": code, "expected": 0})
                if code:
                    raise RuntimeError(f"{tag}: unexpected exit {code}")

    # Closed seed -> actual engine reopen, with both history-retention choices.
    for variant, n, backend, start, mode, first in itertools.product(
        ["head", "baseline"],
        [8] if options.smoke else [8, LONG_HISTORY],
        ["memory", "redis-nonvirtual"],
        ["cold", "warm"],
        ["reopen-discard", "reopen-carry"],
        ["unique-only", "duplicate", "middle", "latest"],
    ):
        tag = f"check-{variant}-{n}-{backend}-{start}-{first}-{mode}"
        code = run(variant + "-engine", [n, backend, 0, start, first, 1, "full", mode], tag)
        record_reopen_result(results, tag, code, (n, backend, mode, first))

    for round_id in range(0 if options.smoke else 3):
        variants = ["baseline", "head"] if round_id % 2 == 0 else ["head", "baseline"]
        for n, backend, start, mode in itertools.product(
            [8, LONG_HISTORY],
            ["memory", "redis-nonvirtual"],
            ["cold", "warm"],
            ["reopen-discard", "reopen-carry"],
        ):
            for variant in variants:
                tag = f"measure-r{round_id}-{variant}-{n}-{backend}-{start}-{mode}"
                code = run(
                    variant + "-engine",
                    [n, backend, 0, start, "unique-only", 1, "economics", mode],
                    tag,
                )
                record_reopen_result(results, tag, code, (n, backend, mode, "unique-only"))

    # Allocation instrumentation runs separately from ordinary latency measurements.
    if all(ALLOCATIONS.values()):
        for variant, n, start, mode, first in itertools.product(
            ["baseline", "head"],
            [8, LONG_HISTORY],
            ["cold", "warm"],
            ["reopen-discard", "reopen-carry"],
            ["unique-only", "duplicate", "middle", "latest"],
        ):
            tag = f"allocations-{variant}-{n}-memory-{start}-{first}-{mode}"
            code = run(
                str(ALLOCATIONS[variant]),
                [n, "memory", 0, start, first, 1, "full", mode],
                tag,
            )
            results.append({"tag": tag, "code": code, "expected": 0})
            if code:
                raise RuntimeError(f"{tag}: unexpected exit {code}")

    for variant in ["baseline", "head"]:
        code = run(variant + "-memory", [], "memory-" + variant)
        results.append({"tag": "memory-" + variant, "code": code, "expected": 0})
        if code:
            raise RuntimeError("Memory probe failed")
finally:
    (ROOT / "run-results.json").write_text(json.dumps(results, indent=2))
failures = sum(result["code"] != 0 for result in results)
sys.stdout.write(f"Completed {len(results)} runs; {failures} recorded failures (not passes)\n")
