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
Test scoped release vetting and failure propagation.
"""

from __future__ import annotations

import io
import json
import runpy
import subprocess
import sys
import tomllib
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
AUDITOR = runpy.run_path(str(ROOT / "scripts/security-audit.py"))
FUZZ_MANIFEST = "crates/adapters/lighter/fuzz/pornin/Cargo.toml"
VET_CONFIG = ".cargo/vet.toml"


def _assert_equal(actual: object, expected: object) -> None:
    if actual != expected:
        raise AssertionError(f"Expected {expected!r}, received {actual!r}")


def _test_scoped_checks(failing_manifest: str | None) -> None:
    store = ROOT / ".supply-chain"
    originals = {path.name: path.read_bytes() for path in store.iterdir() if path.is_file()}
    config = tomllib.loads(originals["config.toml"].decode())
    expected = dict(config)
    expected["policy"] = {"nautilus-lighter": config["policy"]["nautilus-lighter"]}
    version = AUDITOR["_read_versions"](ROOT, {"cargo-vet"})["cargo-vet"]

    checks = []
    scoped_stores = []

    def run_process(args: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        _assert_equal(args[0], "cargo")
        _assert_equal(kwargs["root"], ROOT)
        if args[1:] == ["vet", "--version"]:
            return subprocess.CompletedProcess(args, 0, f"cargo-vet {version}", "")
        if args[1] == "metadata":
            _assert_equal(
                args[2:],
                [
                    "--manifest-path",
                    FUZZ_MANIFEST,
                    "--all-features",
                    "--locked",
                    "--format-version",
                    "1",
                ],
            )
            return subprocess.CompletedProcess(
                args,
                0,
                json.dumps({"packages": [{"name": "nautilus-lighter"}]}),
                "",
            )
        _assert_equal(args[1:3], ["vet", "--manifest-path"])
        manifest = args[3]
        checks.append(manifest)
        if manifest == FUZZ_MANIFEST:
            _assert_equal(args[4], "--store-path")
            scoped = Path(args[5])
            scoped_stores.append(scoped)
            _assert_equal(tomllib.loads((scoped / "config.toml").read_text()), expected)
            for name in ("audits.toml", "imports.lock"):
                _assert_equal((scoped / name).read_bytes(), originals[name])
            _assert_equal(args[6:], ["--locked"])
        else:
            _assert_equal(args[4:], ["--locked"])
        code = 255 if manifest == failing_manifest else 0
        return subprocess.CompletedProcess(args, code, "", "vet failure" if code else "")

    def resolve_executable(name: str) -> str:
        _assert_equal(name, "cargo")
        return name

    with (
        patch.object(sys, "argv", ["security-audit.py", "run", "--config", VET_CONFIG]),
        patch.dict(
            AUDITOR["main"].__globals__,
            {
                "_run_process": run_process,
                "_resolve_executable": resolve_executable,
            },
        ),
        patch("sys.stdout", new_callable=io.StringIO),
        patch("sys.stderr", new_callable=io.StringIO) as stderr,
    ):
        result = AUDITOR["main"]()

    _assert_equal(result, 0 if failing_manifest is None else 1)
    _assert_equal(checks, ["Cargo.toml", FUZZ_MANIFEST])
    _assert_equal(len(scoped_stores), 1)
    _assert_equal(scoped_stores[0].exists(), expected=False)
    if failing_manifest is not None:
        _assert_equal(f"cargo vet {failing_manifest} failed" in stderr.getvalue(), expected=True)
    _assert_equal(
        {path.name: path.read_bytes() for path in store.iterdir() if path.is_file()},
        originals,
    )


if __name__ == "__main__":
    policy = tomllib.loads((ROOT / "security-audit.toml").read_text())
    release_policy = tomllib.loads((ROOT / VET_CONFIG).read_text())
    _assert_equal(
        release_policy,
        {"version": policy["version"], "cargo": {"vet": policy["cargo"]["vet"]}},
    )
    for failing_manifest in (None, "Cargo.toml", FUZZ_MANIFEST):
        _test_scoped_checks(failing_manifest)
