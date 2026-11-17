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

import pathlib
import signal
import subprocess
import sys
import tempfile
import time
import unittest


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
    raise SystemExit(0)
signal.signal(signal.SIGTERM, stop)
port = int(sys.argv[sys.argv.index("--port") + 1])
with socket.socket() as server:
    server.bind(("127.0.0.1", port))
    server.listen()
    while True:
        conn, _ = server.accept()
        with conn:
            conn.recv(1024)
            pid = os.getpid() if (root / "owned").exists() else 0
            info = f"process_id:{{pid}}\\r\\n".encode()
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


if __name__ == "__main__":
    unittest.main()
