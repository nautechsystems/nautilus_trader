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
Run tick-to-trade benchmarks with the selected Python and its shared library.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "python"))

from generate_stubs import python_libdir_env  # noqa: E402 - Reuse the Python build's loader setup


def _main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument(
        "--profile",
        choices=("bench-lto", "bench", "nextest"),
        help="Cargo profile (default: bench-lto); explicit profile label when reusing binaries",
    )
    parser.add_argument("--binaries-metadata", type=Path)
    parser.add_argument(
        "--run-ignored",
        choices=("default", "ignored-only", "all"),
        default="ignored-only",
        help="Select timing cases (ignored-only), harness checks (default), or both (all)",
    )
    args, nextest_args = parser.parse_known_args()

    env = python_libdir_env()
    env["PYO3_PYTHON"] = sys.executable
    profile = args.profile or ("bench-lto" if args.binaries_metadata is None else "unknown")
    env["NAUTILUS_TTT_PROFILE"] = profile
    env.setdefault("CARGO_BUILD_JOBS", "1")
    env.setdefault("NAUTILUS_WORKER_THREADS", "2")
    env["NEXTEST_TEST_THREADS"] = "1"
    command = ["cargo", "nextest", "run"]
    if args.binaries_metadata is None:
        command.extend(
            [
                "--locked",
                "--cargo-profile",
                profile,
                "-p",
                "nautilus-live",
                "-p",
                "nautilus-bybit",
                "--bench",
                "tick_to_trade",
                "--features",
                "python",
            ],
        )
    else:
        command.extend(["--binaries-metadata", str(args.binaries_metadata.resolve())])

    command.extend(
        [
            "--run-ignored",
            args.run_ignored,
            "-E",
            "binary(tick_to_trade)",
            "--retries",
            "0",
            "--success-output",
            "immediate",
            "--failure-output",
            "immediate",
            *nextest_args,
        ],
    )
    return subprocess.run(command, cwd=ROOT, env=env, check=False).returncode  # noqa: S603


if __name__ == "__main__":
    sys.exit(_main())
