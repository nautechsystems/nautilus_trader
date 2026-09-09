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
Enforce direct dependency feature policy for maintained Rust manifests.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any


DependencyPolicy = tuple[bool, frozenset[str]]
Manifest = tuple[Path, dict[str, Any]]

WORKSPACE_DEPENDENCY_POLICIES: dict[str, DependencyPolicy] = {
    "alloy": (False, frozenset()),
    "arrow": (False, frozenset({"ffi", "ipc"})),
    "cosmrs": (False, frozenset()),
    "dydx-proto": (False, frozenset()),
    "madsim": (False, frozenset({"macros"})),
    "nautilus-infrastructure": (False, frozenset()),
    "nautilus-network": (False, frozenset()),
    "pyo3": (
        False,
        frozenset(
            {
                "hashbrown",
                "indexmap",
                "jiff-02",
                "macros",
                "multiple-pymethods",
                "rust_decimal",
            },
        ),
    ),
    "pyo3-async-runtimes": (False, frozenset({"attributes", "tokio-runtime"})),
    "rstest": (False, frozenset()),
    "sockudo-ws": (
        False,
        frozenset({"fastrand", "rustls-webpki-roots", "simd"}),
    ),
}

NETWORK_PACKAGE_FEATURES = {
    "nautilus-architect-ax": frozenset({"transport-sockudo"}),
    "nautilus-betfair": frozenset(),
    "nautilus-binance": frozenset({"transport-sockudo"}),
    "nautilus-bitmex": frozenset({"transport-sockudo"}),
    "nautilus-blockchain": frozenset({"transport-sockudo"}),
    "nautilus-bybit": frozenset({"transport-sockudo"}),
    "nautilus-coinbase": frozenset({"transport-sockudo"}),
    "nautilus-databento": frozenset(),
    "nautilus-deribit": frozenset({"transport-sockudo"}),
    "nautilus-derive": frozenset({"transport-sockudo"}),
    "nautilus-dydx": frozenset({"transport-sockudo"}),
    "nautilus-hyperliquid": frozenset({"transport-sockudo"}),
    "nautilus-interactive-brokers": frozenset(),
    "nautilus-kraken": frozenset({"transport-sockudo"}),
    "nautilus-lighter": frozenset({"transport-sockudo"}),
    "nautilus-live": frozenset(),
    "nautilus-okx": frozenset({"transport-sockudo"}),
    "nautilus-polymarket": frozenset({"transport-sockudo"}),
    "nautilus-pyo3": frozenset({"python", "transport-sockudo"}),
    "nautilus-tardis": frozenset(),
    "nautilus-testkit": frozenset(),
}

ALLOY_PACKAGE_FEATURES = {
    "nautilus-blockchain": frozenset({"contract", "signer-local"}),
    "nautilus-derive": frozenset({"signer-local", "sol-types"}),
    "nautilus-hyperliquid": frozenset({"signer-local", "sol-types"}),
    "nautilus-polymarket": frozenset(
        {"contract", "provider-http", "reqwest", "reqwest-rustls-tls", "signer-local"},
    ),
}

TEST_SUPPORT_DEV_PACKAGES = frozenset(
    {
        "nautilus-analysis",
        "nautilus-backtest",
        "nautilus-blockchain",
        "nautilus-common",
        "nautilus-data",
        "nautilus-event-store",
        "nautilus-execution",
        "nautilus-indicators",
        "nautilus-infrastructure",
        "nautilus-live",
        "nautilus-persistence",
        "nautilus-portfolio",
        "nautilus-risk",
        "nautilus-serialization",
        "nautilus-system",
        "nautilus-testkit",
        "nautilus-trading",
    },
)
MODEL_TEST_SUPPORT_FEATURES = frozenset(
    {
        "nautilus-model/test-support",
        "nautilus-model?/test-support",
    },
)

QUICKSTART_DEPENDENCY_POLICIES: dict[str, DependencyPolicy] = {
    "nautilus-lighter": (False, frozenset({"high-precision"})),
    "nautilus-live": (False, frozenset({"node"})),
    "nautilus-testkit": (False, frozenset({"high-precision", "testers"})),
    "tokio": (False, frozenset({"macros", "rt-multi-thread"})),
}


def _dependency_state(declaration: object) -> DependencyPolicy:
    if isinstance(declaration, str):
        return True, frozenset()
    if not isinstance(declaration, dict):
        raise TypeError(f"dependency declaration has unsupported type {type(declaration).__name__}")

    features = declaration.get("features", [])
    if not isinstance(features, list) or not all(isinstance(feature, str) for feature in features):
        raise TypeError("dependency features must be a list of strings")

    uses_default_features = declaration.get("default-features", True)
    if not isinstance(uses_default_features, bool):
        raise TypeError("default-features must be a boolean")
    return uses_default_features, frozenset(features)


def _describe_policy(policy: DependencyPolicy) -> str:
    uses_default_features, features = policy
    defaults = "enabled" if uses_default_features else "disabled"
    return f"defaults {defaults}, features {sorted(features)!r}"


def _check_dependency_policies(
    manifest_path: Path,
    dependencies: object,
    policies: dict[str, DependencyPolicy],
) -> list[str]:
    if not isinstance(dependencies, dict):
        return [f"{manifest_path}: dependency table is missing or invalid"]

    violations = []
    for dependency, expected in policies.items():
        declaration = dependencies.get(dependency)
        if declaration is None:
            violations.append(f"{manifest_path}: dependency {dependency!r} is missing")
            continue
        try:
            actual = _dependency_state(declaration)
        except TypeError as e:
            violations.append(f"{manifest_path}: dependency {dependency!r}: {e}")
            continue
        if actual != expected:
            violations.append(
                f"{manifest_path}: dependency {dependency!r} must use "
                f"{_describe_policy(expected)}, was {_describe_policy(actual)}",
            )
    return violations


def _package_name(manifest: Manifest) -> str | None:
    package = manifest[1].get("package", {})
    if not isinstance(package, dict):
        return None
    name = package.get("name")
    return name if isinstance(name, str) else None


def _check_consumer_features(  # noqa: C901
    manifests: list[Manifest],
    dependency: str,
    expected_features: dict[str, frozenset[str]],
) -> list[str]:
    consumers: dict[str, tuple[Path, bool, set[str]]] = {}
    violations = []

    for path, data in manifests:
        for section, dependencies in _dependency_sections(data):
            if section != "dependencies" and not section.endswith(".dependencies"):
                continue
            if not isinstance(dependencies, dict) or dependency not in dependencies:
                continue
            package_name = _package_name((path, data))
            if package_name is None:
                violations.append(f"{path}: package name is missing")
                continue
            try:
                declaration = dependencies[dependency]
                uses_default_features, features = _dependency_state(declaration)
            except TypeError as e:
                violations.append(f"{path}: {section} dependency {dependency!r}: {e}")
                continue
            if (
                isinstance(declaration, dict)
                and declaration.get("workspace") is True
                and "default-features" not in declaration
            ):
                uses_default_features = False
            consumer_path, consumer_defaults, consumer_features = consumers.setdefault(
                package_name,
                (path, False, set()),
            )
            consumers[package_name] = (
                consumer_path,
                consumer_defaults or uses_default_features,
                consumer_features,
            )
            consumer_features.update(features)

    for package_name in sorted(consumers.keys() - expected_features.keys()):
        path, _, _ = consumers[package_name]
        violations.append(
            f"{path}: {package_name} must classify its {dependency!r} feature policy",
        )
    violations.extend(
        f"{package_name}: expected {dependency!r} dependency declaration is missing"
        for package_name in sorted(expected_features.keys() - consumers.keys())
    )
    for package_name in sorted(consumers.keys() & expected_features.keys()):
        path, uses_default_features, actual_features = consumers[package_name]
        actual = uses_default_features, frozenset(actual_features)
        expected = False, expected_features[package_name]
        if actual != expected:
            violations.append(
                f"{path}: {package_name} dependency {dependency!r} must use "
                f"{_describe_policy(expected)}, was {_describe_policy(actual)}",
            )
    return violations


def _dependency_sections(data: dict[str, Any]) -> list[tuple[str, object]]:
    sections = [
        ("dependencies", data.get("dependencies", {})),
        ("dev-dependencies", data.get("dev-dependencies", {})),
        ("build-dependencies", data.get("build-dependencies", {})),
    ]
    targets = data.get("target", {})
    if not isinstance(targets, dict):
        return sections
    for target, target_data in targets.items():
        if not isinstance(target_data, dict):
            continue
        sections.extend(
            (f"target.{target}.{section}", target_data.get(section, {}))
            for section in (
                "dependencies",
                "dev-dependencies",
                "build-dependencies",
            )
        )
    return sections


def _check_test_support(  # noqa: C901, PLR0912
    manifests: list[Manifest],
    expected_dev_packages: frozenset[str],
) -> list[str]:
    dev_packages = set()
    forwarders = set()
    testkit_has_support = False
    violations = []

    for path, data in manifests:
        package_name = _package_name((path, data))
        if package_name is None:
            continue

        features = data.get("features", {})
        if isinstance(features, dict):
            for feature, values in features.items():
                if isinstance(values, list) and any(
                    isinstance(value, str) and value in MODEL_TEST_SUPPORT_FEATURES
                    for value in values
                ):
                    forwarders.add((package_name, feature))

        for section, dependencies in _dependency_sections(data):
            if not isinstance(dependencies, dict):
                continue
            declaration = dependencies.get("nautilus-model")
            if declaration is None:
                continue
            try:
                _, dependency_features = _dependency_state(declaration)
            except TypeError as e:
                violations.append(f"{path}: dependency 'nautilus-model': {e}")
                continue
            if "test-support" not in dependency_features:
                continue
            if section.endswith("dev-dependencies"):
                dev_packages.add(package_name)
            elif package_name == "nautilus-testkit" and section == "dependencies":
                testkit_has_support = True
            else:
                violations.append(
                    f"{path}: nautilus-model/test-support must be a dev dependency",
                )

    violations.extend(
        f"{package_name}: test-support dev dependency is not classified"
        for package_name in sorted(dev_packages - expected_dev_packages)
    )
    violations.extend(
        f"{package_name}: expected test-support dev dependency is missing"
        for package_name in sorted(expected_dev_packages - dev_packages)
    )
    if not testkit_has_support:
        violations.append("nautilus-testkit: normal dependency must enable test-support")

    expected_forwarders = {("nautilus-trader", "test-support")}
    for package_name, feature in sorted(forwarders - expected_forwarders):
        violations.append(
            f"{package_name}: feature {feature!r} must not forward nautilus-model/test-support",
        )
    for package_name, feature in sorted(expected_forwarders - forwarders):
        violations.append(
            f"{package_name}: feature {feature!r} must forward nautilus-model/test-support",
        )
    return violations


def _tracked_paths(repo_root: Path, filename: str) -> list[Path]:
    git = shutil.which("git")
    if git is None:
        raise FileNotFoundError("git is required")
    result = subprocess.run(  # noqa: S603
        [
            str(Path(git).resolve()),
            "-c",
            "core.fsmonitor=false",
            "-C",
            str(repo_root),
            "ls-files",
            "-z",
            "--cached",
            "--",
            filename,
            f":(glob)**/{filename}",
        ],
        check=True,
        stdout=subprocess.PIPE,
    )
    paths = []
    for raw_path in result.stdout.split(b"\0"):
        if not raw_path:
            continue
        path = repo_root / os.fsdecode(raw_path)
        if path.is_file():
            paths.append(path)
    return paths


def _load_manifests(repo_root: Path) -> list[Manifest]:
    manifests = []
    for path in _tracked_paths(repo_root, "Cargo.toml"):
        with path.open("rb") as file:
            data = tomllib.load(file)
        manifests.append((path.relative_to(repo_root), data))
    return manifests


def _check_lockfiles(repo_root: Path) -> list[str]:
    cargo = shutil.which("cargo")
    if cargo is None:
        raise FileNotFoundError("cargo is required")
    cargo = str(Path(cargo).absolute())
    violations = []
    for lockfile in _tracked_paths(repo_root, "Cargo.lock"):
        manifest = lockfile.with_name("Cargo.toml")
        relative_lockfile = lockfile.relative_to(repo_root)
        if not manifest.is_file():
            violations.append(f"{relative_lockfile}: sibling Cargo.toml is missing")
            continue
        result = subprocess.run(  # noqa: S603
            [
                cargo,
                "metadata",
                "--locked",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
                str(manifest),
            ],
            cwd=repo_root,
            check=False,
            capture_output=True,
            text=True,
        )
        if result.returncode == 0:
            continue
        stderr = result.stderr.strip().splitlines()
        stdout = result.stdout.strip().splitlines()
        detail = stderr[-1] if stderr else stdout[0] if stdout else "cargo metadata failed"
        violations.append(f"{relative_lockfile}: cargo metadata --locked failed: {detail}")
    return violations


def _find_manifest(manifests: list[Manifest], relative_path: str) -> Manifest | None:
    for manifest in manifests:
        if manifest[0].as_posix() == relative_path:
            return manifest
    return None


def _check_repository(repo_root: Path) -> list[str]:
    manifests = _load_manifests(repo_root)
    violations = []

    root_manifest = _find_manifest(manifests, "Cargo.toml")
    if root_manifest is None:
        violations.append("Cargo.toml: workspace manifest is missing")
    else:
        workspace = root_manifest[1].get("workspace", {})
        dependencies = workspace.get("dependencies", {}) if isinstance(workspace, dict) else {}
        violations.extend(
            _check_dependency_policies(
                root_manifest[0],
                dependencies,
                WORKSPACE_DEPENDENCY_POLICIES,
            ),
        )

    violations.extend(
        _check_consumer_features(
            manifests,
            "nautilus-network",
            NETWORK_PACKAGE_FEATURES,
        ),
    )
    violations.extend(
        _check_consumer_features(
            manifests,
            "alloy",
            ALLOY_PACKAGE_FEATURES,
        ),
    )
    violations.extend(_check_test_support(manifests, TEST_SUPPORT_DEV_PACKAGES))

    quickstart = _find_manifest(
        manifests,
        "examples/quickstarts/lighter-rust-data-client/Cargo.toml",
    )
    if quickstart is None:
        violations.append("Lighter Rust quickstart manifest is missing")
    else:
        violations.extend(
            _check_dependency_policies(
                quickstart[0],
                quickstart[1].get("dependencies", {}),
                QUICKSTART_DEPENDENCY_POLICIES,
            ),
        )

    violations.extend(_check_lockfiles(repo_root))
    return violations


def main() -> int:
    """
    Check the repository dependency feature policy.
    """
    repo_root = Path(__file__).resolve().parents[1]
    try:
        violations = _check_repository(repo_root)
    except (OSError, subprocess.SubprocessError, tomllib.TOMLDecodeError) as e:
        sys.stderr.write(f"Dependency feature policy check failed: {e}\n")
        return 1

    if violations:
        sys.stderr.write("Dependency feature policy violations:\n")
        for violation in violations:
            sys.stderr.write(f"  {violation}\n")
        return 1

    sys.stdout.write("Dependency feature policy checks passed\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
