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
Checks runner refusal and child cleanup without compiling or running benchmarks.
"""

import json
import pathlib
import runpy
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch


RUNNER = pathlib.Path(__file__).with_name("benchmark-position-replay.py")


def require(*, condition: bool, message: str) -> None:
    """
    Fail a safety check even when Python assertions are disabled.
    """
    if not condition:
        raise AssertionError(message)


class RunnerSafetyTests(unittest.TestCase):
    """
    Exercise the runner using isolated fake executables.
    """

    def test_refusal_and_cleanup(self) -> None:
        """
        Reject foreign Redis and existing output, and clean up on termination.
        """
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            engine = (
                f"#!{sys.executable}\nimport pathlib,time\n"
                f"pathlib.Path({str(root / 'engine-started')!r}).touch()\n"
                "time.sleep(60)\n"
            )

            for variant in ("head", "baseline"):
                for target in ("engine", "memory"):
                    p = root / f"{variant}-{target}"
                    p.write_text(engine)
                    p.chmod(0o700)
            server = root / "fake-redis"
            server.write_text(f"""#!{sys.executable}
import os, pathlib, signal, socket, sys
root = pathlib.Path({str(root)!r})
def stop(signum, frame):
    (root / "server-stopped").touch()
    (root / f"stopped-{{os.getpid()}}").touch()
    raise SystemExit(0)
signal.signal(signal.SIGTERM, stop)
port = int(sys.argv[sys.argv.index("--port") + 1])
with socket.socket() as server:
    server.bind(("127.0.0.1", port))
    server.listen()
    while True:
        conn, _ = server.accept()
        with conn:
            request = conn.recv(1024)
            if b"DBSIZE" in request:
                size = 1 if (root / "nonempty").exists() else 0
                conn.sendall(f":{{size}}\\r\\n".encode())
                continue
            pid = os.getpid() if (root / "owned").exists() else 0
            info = f"process_id:{{pid}}\\r\\nrun_id:{{pid}}\\r\\n".encode()
            conn.sendall(b"$" + str(len(info)).encode() + b"\\r\\n" + info + b"\\r\\n")
""")
            server.chmod(0o700)

            command = [
                sys.executable,
                str(RUNNER),
                "--binary-dir",
                str(root),
                "--redis-server",
                str(server),
                "--smoke",
                "--output-dir",
            ]

            refused = subprocess.run(  # noqa: S603 (test-created local executables; no shell)
                [*command, str(root / "wrong-pid")],
                capture_output=True,
                text=True,
                timeout=20,
                check=False,
            )
            require(condition=refused.returncode != 0, message="Foreign Redis must be refused")
            require(condition="not owned" in refused.stderr, message="Missing ownership diagnostic")
            require(
                condition=(root / "server-stopped").exists(),
                message="Redis child was not stopped",
            )
            require(
                condition=not (root / "engine-started").exists(),
                message="Fixture started against foreign Redis",
            )
            refused = subprocess.run(  # noqa: S603 (test-created local executables; no shell)
                [*command, str(root)],
                capture_output=True,
                text=True,
                timeout=20,
                check=False,
            )
            require(
                condition="FileExistsError" in refused.stderr,
                message="Existing output must be refused",
            )
            (root / "owned").touch()
            (root / "nonempty").touch()
            refused = subprocess.run(  # noqa: S603 (test-created local executables; no shell)
                [*command, str(root / "nonempty-output")],
                capture_output=True,
                text=True,
                timeout=20,
                check=False,
            )
            require(condition="not empty" in refused.stderr, message="Nonempty Redis accepted")
            require(condition=not (root / "engine-started").exists(), message="Fixture started")
            (root / "nonempty").unlink()
            self.check_isolation(root, command)
            (root / "server-stopped").unlink()
            with subprocess.Popen(  # noqa: S603 (test-created local executables; no shell)
                [*command, str(root / "terminated")],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            ) as runner:
                try:
                    for _ in range(200):
                        if (root / "engine-started").exists():
                            break
                        time.sleep(0.05)
                    else:
                        self.fail("Fixture did not start")
                    runner.send_signal(signal.SIGTERM)
                    require(
                        condition=runner.wait(timeout=15) == 128 + signal.SIGTERM,
                        message="Unexpected exit status",
                    )
                    require(
                        condition=(root / "server-stopped").exists(),
                        message="Redis child was not stopped",
                    )
                    require(
                        condition=(root / "terminated" / "run-results.json").exists(),
                        message="Partial results were not saved",
                    )
                finally:
                    if runner.poll() is None:
                        runner.kill()
                        runner.wait()

    def check_recovery_classification(
        self,
        namespace: dict[str, object],
        output: pathlib.Path,
    ) -> None:
        """
        Accept only the exact recovery failure signature and fixture.
        """
        record = namespace["record_reopen_result"]
        marker = (
            "RELOAD_HISTORY_MISMATCH economics_equal=true other_fields_equal=true "
            "expected_replay_events=10 actual_replay_events=1"
        )
        diagnostic = marker + "\nassertion failed: Redis lost complete replay state\n"
        fixture = (8, "redis-nonvirtual", "reopen-carry", "unique-only")
        (output / "classification.err").write_text(diagnostic)
        results = []
        record(results, "classification", -signal.SIGABRT, fixture)
        require(
            condition=results[0]["outcome"] == "known_recovery_failure",
            message="Exact recovery failure was not recorded distinctly",
        )
        require(condition=results[0]["expected"] != 0, message="Recovery failure labeled success")
        require(
            condition=results[0]["replay_history_equal"] is False,
            message="Lost replay history was mislabeled equal",
        )

        cases = [
            ("missing marker", "Redis lost complete replay state", -signal.SIGABRT, fixture),
            ("missing assertion", marker, -signal.SIGABRT, fixture),
            ("wrong marker", diagnostic.replace("MISMATCH", "OTHER"), -signal.SIGABRT, fixture),
            (
                "economics unequal",
                diagnostic.replace("economics_equal=true", "economics_equal=false"),
                -signal.SIGABRT,
                fixture,
            ),
            (
                "other fields unequal",
                diagnostic.replace("other_fields_equal=true", "other_fields_equal=false"),
                -signal.SIGABRT,
                fixture,
            ),
            (
                "wrong expected history",
                diagnostic.replace("expected_replay_events=10", "expected_replay_events=11"),
                -signal.SIGABRT,
                fixture,
            ),
            (
                "wrong actual history",
                diagnostic.replace("actual_replay_events=1", "actual_replay_events=10"),
                -signal.SIGABRT,
                fixture,
            ),
            ("wrong exit", diagnostic, 2, fixture),
            ("different signal", diagnostic, -signal.SIGSEGV, fixture),
            (
                "wrong backend",
                diagnostic,
                -signal.SIGABRT,
                (8, "memory", "reopen-carry", "unique-only"),
            ),
            (
                "wrong mode",
                diagnostic,
                -signal.SIGABRT,
                (8, "redis-nonvirtual", "reopen-discard", "unique-only"),
            ),
            (
                "wrong first",
                diagnostic,
                -signal.SIGABRT,
                (8, "redis-nonvirtual", "reopen-carry", "duplicate"),
            ),
            (
                "wrong seed",
                diagnostic,
                -signal.SIGABRT,
                (64, "redis-nonvirtual", "reopen-carry", "unique-only"),
            ),
        ]

        for name, error, code, scenario in cases:
            with self.subTest(name=name):
                (output / "classification.err").write_text(error)
                rejected = []
                try:
                    record(rejected, "classification", code, scenario)
                except RuntimeError:
                    require(
                        condition=not any(
                            r["outcome"] == "known_recovery_failure" for r in rejected
                        ),
                        message="Unexpected failure was whitelisted",
                    )
                else:
                    self.fail(f"Unexpected recovery result accepted: {name}")

    def check_isolation(self, root: pathlib.Path, command: list[str]) -> None:
        """
        Check the smoke matrix with stubbed fixtures and fake servers.
        """
        seen_ports = []

        def fixture(argv: list[str], **kwargs: object) -> subprocess.CompletedProcess:
            if argv[0].endswith("-memory"):
                require(condition=(root / "server-stopped").exists(), message="Redis still alive")
                return subprocess.CompletedProcess(argv, 0)

            if argv[2] != "memory":
                seen_ports.append(int(argv[3]))
            duplicate = (
                argv[0].endswith("baseline-engine")
                and argv[2] == "redis-nonvirtual"
                and argv[5] == "duplicate"
                and argv[-1] == "flip"
            )

            if duplicate:
                kwargs["stderr"].write(
                    "RELOAD_ERROR Err(Duplicate fill event for position "
                    "AUDUSD-REPEAT-FLIP: T-FLIP-2)",
                )
            return subprocess.CompletedProcess(argv, 2 if duplicate else 0)

        output = root / "isolation"
        original_sleep = time.sleep
        with (
            patch.object(
                sys,
                "argv",
                [
                    str(RUNNER),
                    *command[2:],
                    str(output),
                    "--legacy-duplicate-reload",
                    "baseline",
                    "--baseline-allocations",
                    str(root / "baseline-engine"),
                    "--head-allocations",
                    str(root / "head-engine"),
                ],
            ),
            patch("subprocess.run", side_effect=fixture),
            patch("os.setpriority"),
            patch("time.sleep", side_effect=lambda seconds: original_sleep(min(seconds, 0.01))),
        ):
            namespace = runpy.run_path(str(RUNNER), run_name="__main__")
        self.check_recovery_classification(namespace, output)
        records = [json.loads(p.read_text()) for p in output.glob("*/redis-metadata.json")]
        require(condition=len(records) == len(seen_ports), message="Redis count mismatch")
        require(condition=len(records) > 1, message="Did not exercise consecutive fixtures")
        require(
            condition=len({r["run_id"] for r in records}) == len(records),
            message="Redis instance reused",
        )
        require(
            condition=all(r["ready"] and r["initial_dbsize"] == 0 for r in records),
            message="Fixture did not start against an empty verified server",
        )
        require(condition=not list(output.glob("memory-*/*")), message="Memory started Redis")
        require(
            condition=all(
                "redis_exit" in r and (root / f"stopped-{r['pid']}").exists() for r in records
            ),
            message="Redis cleanup missing",
        )
        results = json.loads((output / "run-results.json").read_text())
        for row in results:
            tag = row["tag"]
            if tag.endswith("duplicate-flip") and "redis-nonvirtual" in tag:
                expected = 2 if tag.startswith("check-baseline-") else 0
                require(
                    condition=row["code"] == row["expected"] == expected,
                    message="Legacy expectation leaked across binary variants",
                )
        allocation_count = sum(r["tag"].startswith("allocations-") for r in results)
        expected_allocations = 64
        require(
            condition=allocation_count == expected_allocations,
            message="Allocation matrix missing",
        )
        timeout_output = root / "timeout"
        with (
            patch.object(sys, "argv", [str(RUNNER), *command[2:], str(timeout_output)]),
            patch("subprocess.run", side_effect=subprocess.TimeoutExpired("fixture", 180)),
            patch("os.setpriority"),
        ):
            try:
                runpy.run_path(str(RUNNER), run_name="__main__")
            except subprocess.TimeoutExpired:
                pass
            else:
                self.fail("Fixture timeout was swallowed")
        record = json.loads(next(timeout_output.glob("*/redis-metadata.json")).read_text())
        require(
            condition="redis_exit" in record and (root / f"stopped-{record['pid']}").exists(),
            message="Redis leaked after fixture timeout",
        )


if __name__ == "__main__":
    unittest.main()
