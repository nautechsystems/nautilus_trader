#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Test the dependency feature policy checker.
"""

from __future__ import annotations

import runpy
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING
from typing import Any
from unittest.mock import patch


if TYPE_CHECKING:
    from collections.abc import Callable


CHECKER_PATH = Path(__file__).with_name("check_dependency_features.py")
CHECKER = runpy.run_path(str(CHECKER_PATH))


def _assert_equal(actual: object, expected: object) -> None:
    if actual != expected:
        raise AssertionError(f"Expected {expected!r}, received {actual!r}")


def _manifest(path: str, name: str, data: dict[str, Any]) -> tuple[Path, dict[str, Any]]:
    return Path(path), {"package": {"name": name}, **data}


def _test_workspace_dependency_policy() -> None:
    check: Callable[..., list[str]] = CHECKER["_check_dependency_policies"]
    policy = {"arrow": (False, frozenset({"ffi", "ipc"}))}
    valid = {"arrow": {"default-features": False, "features": ["ipc", "ffi"]}}
    _assert_equal(check(Path("Cargo.toml"), valid, policy), [])

    expanded = {"arrow": {"default-features": False, "features": ["ipc", "ffi", "json"]}}
    violations = check(Path("Cargo.toml"), expanded, policy)
    _assert_equal(len(violations), 1)
    if "'json'" not in violations[0]:
        raise AssertionError(f"Expanded feature was not reported: {violations[0]}")

    defaults_enabled = {"arrow": {"default-features": True, "features": ["ipc", "ffi"]}}
    violations = check(Path("Cargo.toml"), defaults_enabled, policy)
    _assert_equal(len(violations), 1)
    if "defaults enabled" not in violations[0]:
        raise AssertionError(f"Enabled defaults were not reported: {violations[0]}")


def _test_consumer_classification() -> None:
    check: Callable[..., list[str]] = CHECKER["_check_consumer_features"]
    policies = {
        "nautilus-data": frozenset({"transport-sockudo"}),
        "nautilus-http": frozenset(),
    }
    manifests = [
        _manifest(
            "crates/adapters/data/Cargo.toml",
            "nautilus-data",
            {
                "dependencies": {
                    "nautilus-network": {
                        "workspace": True,
                        "features": ["transport-sockudo"],
                    },
                },
            },
        ),
        _manifest(
            "crates/adapters/http/Cargo.toml",
            "nautilus-http",
            {"dependencies": {"nautilus-network": {"workspace": True}}},
        ),
    ]
    _assert_equal(
        check(manifests, "nautilus-network", policies),
        [],
    )

    manifests[0][1]["dependencies"]["nautilus-network"]["features"].append("proxy")
    violations = check(manifests, "nautilus-network", policies)
    _assert_equal(len(violations), 1)
    if "'proxy'" not in violations[0]:
        raise AssertionError(f"Expanded consumer feature was not reported: {violations[0]}")
    manifests[0][1]["dependencies"]["nautilus-network"]["features"].pop()

    manifests[0][1]["dependencies"]["nautilus-network"]["default-features"] = True
    violations = check(manifests, "nautilus-network", policies)
    _assert_equal(len(violations), 1)
    if "defaults enabled" not in violations[0]:
        raise AssertionError(f"Consumer defaults were not reported: {violations[0]}")
    manifests[0][1]["dependencies"]["nautilus-network"].pop("default-features")

    manifests[0][1]["target"] = {
        "cfg(windows)": {
            "dependencies": {
                "nautilus-network": {
                    "workspace": True,
                    "features": ["proxy"],
                },
            },
        },
    }
    violations = check(manifests, "nautilus-network", policies)
    _assert_equal(len(violations), 1)
    if "'proxy'" not in violations[0]:
        raise AssertionError(f"Target-specific feature was not reported: {violations[0]}")
    manifests[0][1].pop("target")

    manifests.append(
        _manifest(
            "crates/bindings/Cargo.toml",
            "nautilus-bindings",
            {"target": {"cfg(unix)": {"dependencies": {"nautilus-network": {"workspace": True}}}}},
        ),
    )
    violations = check(manifests, "nautilus-network", policies)
    _assert_equal(len(violations), 1)
    if "must classify" not in violations[0]:
        raise AssertionError(f"Unclassified consumer was not reported: {violations[0]}")


def _test_test_support_policy() -> None:
    check: Callable[..., list[str]] = CHECKER["_check_test_support"]
    manifests = [
        _manifest(
            "crates/core/Cargo.toml",
            "nautilus-core",
            {
                "dev-dependencies": {
                    "nautilus-model": {
                        "workspace": True,
                        "features": ["test-support"],
                    },
                },
            },
        ),
        _manifest(
            "crates/testkit/Cargo.toml",
            "nautilus-testkit",
            {
                "dependencies": {
                    "nautilus-model": {
                        "workspace": True,
                        "features": ["test-support"],
                    },
                },
            },
        ),
        _manifest(
            "crates/Cargo.toml",
            "nautilus-trader",
            {"features": {"test-support": ["nautilus-model/test-support"]}},
        ),
    ]
    _assert_equal(check(manifests, frozenset({"nautilus-core"})), [])

    manifests[0][1]["dependencies"] = manifests[0][1].pop("dev-dependencies")
    violations = check(manifests, frozenset({"nautilus-core"}))
    if not any("must be a dev dependency" in violation for violation in violations):
        raise AssertionError(f"Production test-support use was not reported: {violations!r}")
    if not any(
        "expected test-support dev dependency is missing" in violation for violation in violations
    ):
        raise AssertionError(
            f"Missing test-support dev dependency was not reported: {violations!r}",
        )

    manifests[0][1]["dev-dependencies"] = manifests[0][1].pop("dependencies")
    manifests[1][1]["features"] = {"extra": ["nautilus-model?/test-support"]}
    violations = check(manifests, frozenset({"nautilus-core"}))
    _assert_equal(len(violations), 1)
    if not any("feature 'extra' must not forward" in violation for violation in violations):
        raise AssertionError(f"Weak feature forwarding was not reported: {violations!r}")


def _test_tracked_paths() -> None:
    tracked_paths: Callable[..., list[Path]] = CHECKER["_tracked_paths"]
    with tempfile.TemporaryDirectory() as directory:
        repo = Path(directory)
        git = shutil.which("git")
        if git is None:
            raise RuntimeError("git is required")
        git = str(Path(git).resolve())
        subprocess.run(  # noqa: S603
            [git, "init", "--quiet", str(repo)],
            check=True,
        )

        tracked = repo / "Cargo.toml"
        tracked.write_text("[workspace]\n")
        subprocess.run(  # noqa: S603
            [git, "-C", str(repo), "add", "Cargo.toml"],
            check=True,
        )
        untracked = repo / "scratch" / "Cargo.toml"
        untracked.parent.mkdir()
        untracked.write_text("not valid TOML")

        _assert_equal(tracked_paths(repo, "Cargo.toml"), [tracked])
        tracked.unlink()
        _assert_equal(tracked_paths(repo, "Cargo.toml"), [])


def _test_lockfile_metadata_uses_cargo_proxy() -> None:
    check: Callable[..., list[str]] = CHECKER["_check_lockfiles"]
    checker_globals = check.__globals__
    with tempfile.TemporaryDirectory() as directory:
        repo = Path(directory)
        manifest = repo / "Cargo.toml"
        manifest.write_text("[workspace]\n")
        lockfile = repo / "Cargo.lock"
        lockfile.touch()
        cargo = repo / "bin" / ("cargo.exe" if sys.platform == "win32" else "cargo")
        resolved = repo / "bin" / ("rustup.exe" if sys.platform == "win32" else "rustup")
        proxy_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="error: proxy invocation failed\n",
            stderr="",
        )
        cargo_result = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="Updating registry\nerror: lockfile changed\n",
        )

        with (
            patch.dict(checker_globals, {"_tracked_paths": lambda *_: [lockfile]}),
            patch.object(checker_globals["shutil"], "which", return_value=str(cargo)),
            patch.object(
                checker_globals["subprocess"],
                "run",
                side_effect=[proxy_result, cargo_result],
            ) as run,
            patch.object(Path, "resolve", return_value=resolved),
        ):
            proxy_violations = check(repo)
            cargo_violations = check(repo)

        _assert_equal([call.args[0][0] for call in run.call_args_list], [str(cargo)] * 2)
        _assert_equal(
            proxy_violations,
            ["Cargo.lock: cargo metadata --locked failed: error: proxy invocation failed"],
        )
        _assert_equal(
            cargo_violations,
            ["Cargo.lock: cargo metadata --locked failed: error: lockfile changed"],
        )


def _test_hook_trigger() -> None:
    config = CHECKER_PATH.parents[1].joinpath(".pre-commit-config.yaml").read_text()
    start = config.index("      - id: dependency-feature-policy")
    end = config.find("      - id:", start + 7)
    hook = config[start:] if end == -1 else config[start:end]
    for expected in (
        "Cargo\\.toml",
        "Cargo\\.lock",
        "check_dependency_features\\.py",
        "test_check_dependency_features\\.py",
    ):
        if expected not in hook:
            raise AssertionError(f"Hook trigger is missing {expected!r}")
    if "\\.rs" in hook:
        raise AssertionError("Rust source changes must not trigger the dependency feature policy")


def main() -> None:
    """
    Run the dependency feature policy script tests.
    """
    _test_workspace_dependency_policy()
    _test_consumer_classification()
    _test_test_support_policy()
    _test_tracked_paths()
    _test_lockfile_metadata_uses_cargo_proxy()
    _test_hook_trigger()
    sys.stdout.write("Dependency feature policy tests passed\n")


if __name__ == "__main__":
    main()
