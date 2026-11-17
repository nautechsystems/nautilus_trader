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
LONG_HISTORY = 36_000
DUPLICATE_RELOAD_EXIT = 2


parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
parser.add_argument("--binary-dir", type=pathlib.Path, required=True)
parser.add_argument("--output-dir", type=pathlib.Path, required=True)
parser.add_argument("--redis-server", default=shutil.which("redis-server"))
parser.add_argument(
    "--smoke",
    action="store_true",
    help="Run only small correctness fixtures and memory probes",
)
options = parser.parse_args()
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


def run(binary: str, args: list[str | int], tag: str) -> int:
    """
    Run one caller-selected local fixture and retain its output.
    """
    if server.poll() is not None:
        raise RuntimeError("Private Redis exited before fixture launch")
    started = time.monotonic()
    with (
        (ROOT / (tag + ".out")).open("x") as out,
        (ROOT / (tag + ".err")).open("x") as err,
    ):
        p = subprocess.run(  # noqa: S603 (explicit local binaries, argv only; no shell)
            [str(BIN / binary), *map(str, args)],
            stdout=out,
            stderr=err,
            env=ENV,
            timeout=180,
            check=False,
        )
    time.sleep(max(3, min(30, time.monotonic() - started)))
    return p.returncode


with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]

if port < MIN_EPHEMERAL_PORT:
    raise RuntimeError("Allocated port is outside the fixture port range")
log = (ROOT / "redis.log").open("x")

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
        str(ROOT),
    ],
    stdout=log,
    stderr=subprocess.STDOUT,
)

results = []
try:
    for _ in range(100):
        if server.poll() is not None:
            raise RuntimeError("Private Redis exited")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2) as sock:
                sock.sendall(b"*2\r\n$4\r\nINFO\r\n$6\r\nserver\r\n")
                with sock.makefile("rb") as stream:
                    header = stream.readline()
                    if not header.startswith(b"$"):
                        raise RuntimeError("Unexpected Redis INFO response")
                    payload = stream.read(int(header[1:]))
                info = dict(line.split(b":", 1) for line in payload.splitlines() if b":" in line)
                if info.get(b"process_id") != str(server.pid).encode():
                    raise RuntimeError("Redis port is not owned by the process we launched")
            break
        except OSError:
            time.sleep(0.05)
    else:
        raise RuntimeError("Private Redis not ready")

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
        args = [n, backend, port, start, first, 3, "full", mode]
        code = run(variant + "-engine", args, tag)
        expected = (
            DUPLICATE_RELOAD_EXIT if backend == "redis-nonvirtual" and first == "duplicate" else 0
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
                    [n, backend, port, start, "unique-only", count, "economics", mode],
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
    try:
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait(timeout=10)
    finally:
        log.close()
        (ROOT / "run-results.json").write_text(json.dumps(results, indent=2))
sys.stdout.write(f"Completed {len(results)} runs\n")
