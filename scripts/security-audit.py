#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License, Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software distributed under the
#  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either
#  express or implied. See the License for the specific language governing permissions and
#  limitations under the License.
"""Run repository supply-chain checks from a typed local policy."""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import cast


__all__: tuple[str, ...] = ()

ADVISORY_ID = re.compile(r"^RUSTSEC-\d{4}-\d{4}$")
GROUP_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
PYTHON_VERSION = re.compile(r"^\d+\.\d+$")
REPORTED_VERSION = re.compile(
    r"(?<![0-9A-Za-z.])"
    r"([0-9]+\.[0-9]+\.[0-9]+"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?)"
    r"(?![0-9A-Za-z.+-])",
)
STABLE_VERSION = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+")
VULNERABILITY_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]*$")
CARGO_AUDIT_DENIES = frozenset({"unmaintained", "unsound", "warnings", "yanked"})
CARGO_DENY_CHECKS = frozenset({"advisories", "bans", "licenses", "sources"})
NODE_FULL_MODES = frozenset({"gate", "off", "report"})


class AuditError(Exception):
    """Report invalid policy, missing tools, or failed audit commands."""


@dataclass(frozen=True, slots=True)
class CargoAudit:
    lockfile: Path
    ignores: tuple[str, ...]
    denies: tuple[str, ...]
    report: bool


@dataclass(frozen=True, slots=True)
class CargoDeny:
    manifest: Path
    config: Path | None
    all_features: bool
    checks: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class CargoVet:
    manifest: Path
    store: Path | None


@dataclass(frozen=True, slots=True)
class PythonAudit:
    project: Path
    python: str
    all_extras: bool
    all_groups: bool
    groups: tuple[str, ...]
    ignores: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class NodeAudit:
    project: Path
    production: bool
    full: str
    signatures: bool


@dataclass(frozen=True, slots=True)
class OsvAudit:
    config: Path | None
    lockfiles: tuple[Path, ...]
    report: bool


@dataclass(frozen=True, slots=True)
class AuditPolicy:
    cargo_audits: tuple[CargoAudit, ...]
    cargo_denies: tuple[CargoDeny, ...]
    cargo_vets: tuple[CargoVet, ...]
    python_audits: tuple[PythonAudit, ...]
    node_audits: tuple[NodeAudit, ...]
    osv: OsvAudit | None


@dataclass(frozen=True, slots=True)
class Context:
    root: Path
    versions: dict[str, str]
    executables: dict[str, str]


@dataclass(frozen=True, slots=True)
class StepOptions:
    cwd: Path | None = None
    report: bool = False
    gate: bool = True


DEFAULT_STEP_OPTIONS = StepOptions()


def _write(message: str, *, error: bool = False) -> None:
    stream = sys.stderr if error else sys.stdout
    stream.write(f"{message}\n")


def _read_toml(path: Path) -> dict[str, object]:
    try:
        value = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise AuditError(f"{path}: {error}") from error
    if not isinstance(value, dict):
        raise AuditError(f"{path}: document must be a table")
    return cast("dict[str, object]", value)


def _reject_unknown(table: dict[str, object], allowed: set[str], context: str) -> None:
    unknown = sorted(table.keys() - allowed)
    if unknown:
        raise AuditError(f"{context}: unknown field(s): {', '.join(unknown)}")


def _required_string(table: dict[str, object], key: str, context: str) -> str:
    value = table.get(key)
    if not isinstance(value, str) or not value:
        raise AuditError(f"{context}.{key} must be a non-empty string")
    return value


def _string_list(
    table: dict[str, object],
    key: str,
    context: str,
    *,
    default: tuple[str, ...] = (),
) -> tuple[str, ...]:
    value = table.get(key)
    if value is None:
        return default
    if not isinstance(value, list) or any(not isinstance(item, str) or not item for item in value):
        raise AuditError(f"{context}.{key} must be an array of non-empty strings")
    if len(set(value)) != len(value):
        raise AuditError(f"{context}.{key} contains a duplicate")
    return tuple(value)


def _boolean(table: dict[str, object], key: str, context: str, *, default: bool) -> bool:
    value = table.get(key, default)
    if not isinstance(value, bool):
        raise AuditError(f"{context}.{key} must be a Boolean")
    return value


def _table_list(parent: dict[str, object], key: str, context: str) -> list[dict[str, object]]:
    value = parent.get(key, [])
    if not isinstance(value, list) or any(not isinstance(item, dict) for item in value):
        raise AuditError(f"{context}.{key} must be an array of tables")
    return cast("list[dict[str, object]]", value)


def _path(root: Path, value: str, context: str, *, directory: bool = False) -> Path:
    if not value or "\\" in value:
        raise AuditError(f"{context} must be a non-empty repository-relative POSIX path")
    candidate = Path(value)
    if candidate.is_absolute() or ".." in candidate.parts:
        raise AuditError(f"{context} must stay within the repository")
    try:
        resolved = (root / candidate).resolve(strict=True)
    except OSError as error:
        raise AuditError(f"{context}: {error}") from error
    if not resolved.is_relative_to(root):
        raise AuditError(f"{context} resolves outside the repository")
    if directory and not resolved.is_dir():
        raise AuditError(f"{context} must name a directory")
    if not directory and not resolved.is_file():
        raise AuditError(f"{context} must name a regular file")
    return resolved.relative_to(root)


def _optional_path(
    root: Path,
    table: dict[str, object],
    key: str,
    context: str,
    *,
    directory: bool = False,
) -> Path | None:
    value = table.get(key)
    if value is None:
        return None
    if not isinstance(value, str):
        raise AuditError(f"{context}.{key} must be a string")
    return _path(root, value, f"{context}.{key}", directory=directory)


def _parse_cargo_audits(root: Path, cargo: dict[str, object]) -> tuple[CargoAudit, ...]:
    audits = []
    for index, table in enumerate(_table_list(cargo, "audit", "cargo")):
        context = f"cargo.audit[{index}]"
        _reject_unknown(table, {"deny", "ignore", "lockfile", "report"}, context)
        ignores = _string_list(table, "ignore", context)
        invalid_ignores = [value for value in ignores if ADVISORY_ID.fullmatch(value) is None]
        if invalid_ignores:
            raise AuditError(f"{context}.ignore contains an invalid advisory id")
        denies = _string_list(table, "deny", context)
        invalid_denies = sorted(set(denies) - CARGO_AUDIT_DENIES)
        if invalid_denies:
            raise AuditError(f"{context}.deny has invalid values: {', '.join(invalid_denies)}")
        audits.append(
            CargoAudit(
                lockfile=_path(
                    root,
                    _required_string(table, "lockfile", context),
                    f"{context}.lockfile",
                ),
                ignores=ignores,
                denies=denies,
                report=_boolean(table, "report", context, default=False),
            ),
        )
    return tuple(audits)


def _parse_cargo_denies(root: Path, cargo: dict[str, object]) -> tuple[CargoDeny, ...]:
    audits = []
    for index, table in enumerate(_table_list(cargo, "deny", "cargo")):
        context = f"cargo.deny[{index}]"
        _reject_unknown(
            table,
            {"all-features", "checks", "config", "manifest"},
            context,
        )
        checks = _string_list(
            table,
            "checks",
            context,
            default=("advisories", "licenses", "sources", "bans"),
        )
        invalid_checks = sorted(set(checks) - CARGO_DENY_CHECKS)
        if invalid_checks:
            raise AuditError(f"{context}.checks has invalid values: {', '.join(invalid_checks)}")
        audits.append(
            CargoDeny(
                manifest=_path(
                    root,
                    _required_string(table, "manifest", context),
                    f"{context}.manifest",
                ),
                config=_optional_path(root, table, "config", context),
                all_features=_boolean(table, "all-features", context, default=False),
                checks=checks,
            ),
        )
    return tuple(audits)


def _parse_cargo_vets(root: Path, cargo: dict[str, object]) -> tuple[CargoVet, ...]:
    audits = []
    for index, table in enumerate(_table_list(cargo, "vet", "cargo")):
        context = f"cargo.vet[{index}]"
        _reject_unknown(table, {"manifest", "store"}, context)
        audits.append(
            CargoVet(
                manifest=_path(
                    root,
                    _required_string(table, "manifest", context),
                    f"{context}.manifest",
                ),
                store=_optional_path(root, table, "store", context, directory=True),
            ),
        )
    return tuple(audits)


def _parse_python(root: Path, value: object) -> tuple[PythonAudit, ...]:
    if value is None:
        return ()
    if not isinstance(value, list) or any(not isinstance(item, dict) for item in value):
        raise AuditError("python must be an array of tables")
    audits = []
    value = cast("list[dict[str, object]]", value)
    for index, table in enumerate(value):
        context = f"python[{index}]"
        _reject_unknown(
            table,
            {"all-extras", "all-groups", "groups", "ignore-vulns", "project", "python"},
            context,
        )
        python = _required_string(table, "python", context)
        if PYTHON_VERSION.fullmatch(python) is None:
            raise AuditError(f"{context}.python must be a major.minor version")
        groups = _string_list(table, "groups", context)
        if any(GROUP_NAME.fullmatch(group) is None for group in groups):
            raise AuditError(f"{context}.groups contains an invalid group name")
        all_groups = _boolean(table, "all-groups", context, default=False)
        if all_groups and groups:
            raise AuditError(f"{context} cannot set both all-groups and groups")
        ignores = _string_list(table, "ignore-vulns", context)
        if any(VULNERABILITY_ID.fullmatch(item) is None for item in ignores):
            raise AuditError(f"{context}.ignore-vulns contains an invalid vulnerability id")
        audits.append(
            PythonAudit(
                project=_path(
                    root,
                    _required_string(table, "project", context),
                    f"{context}.project",
                    directory=True,
                ),
                python=python,
                all_extras=_boolean(table, "all-extras", context, default=False),
                all_groups=all_groups,
                groups=groups,
                ignores=ignores,
            ),
        )
    return tuple(audits)


def _parse_node(root: Path, value: object) -> tuple[NodeAudit, ...]:
    if value is None:
        return ()
    if not isinstance(value, list) or any(not isinstance(item, dict) for item in value):
        raise AuditError("node must be an array of tables")
    audits = []
    value = cast("list[dict[str, object]]", value)
    for index, table in enumerate(value):
        context = f"node[{index}]"
        _reject_unknown(table, {"full", "production", "project", "signatures"}, context)
        full = table.get("full", "off")
        if not isinstance(full, str) or full not in NODE_FULL_MODES:
            raise AuditError(f"{context}.full must be one of: gate, off, report")
        production = _boolean(table, "production", context, default=True)
        signatures = _boolean(table, "signatures", context, default=False)
        if not production and full == "off" and not signatures:
            raise AuditError(f"{context} must enable at least one audit")
        audits.append(
            NodeAudit(
                project=_path(
                    root,
                    _required_string(table, "project", context),
                    f"{context}.project",
                    directory=True,
                ),
                production=production,
                full=full,
                signatures=signatures,
            ),
        )
    return tuple(audits)


def _parse_osv(root: Path, value: object) -> OsvAudit | None:
    if value is None:
        return None
    if not isinstance(value, dict):
        raise AuditError("osv must be a table")
    value = cast("dict[str, object]", value)
    _reject_unknown(value, {"config", "lockfiles", "report"}, "osv")
    lockfiles = _string_list(value, "lockfiles", "osv")
    if not lockfiles:
        raise AuditError("osv.lockfiles must contain at least one path")
    return OsvAudit(
        config=_optional_path(root, value, "config", "osv"),
        lockfiles=tuple(_path(root, item, "osv.lockfiles") for item in lockfiles),
        report=_boolean(value, "report", "osv", default=False),
    )


def _read_policy(root: Path, config_path: Path) -> AuditPolicy:
    data = _read_toml(config_path)
    _reject_unknown(data, {"cargo", "node", "osv", "python", "version"}, str(config_path))
    if data.get("version") != 1:
        raise AuditError(f"{config_path}: version must be 1")
    cargo = data.get("cargo", {})
    if not isinstance(cargo, dict):
        raise AuditError("cargo must be a table")
    cargo = cast("dict[str, object]", cargo)
    _reject_unknown(cargo, {"audit", "deny", "vet"}, "cargo")
    policy = AuditPolicy(
        cargo_audits=_parse_cargo_audits(root, cargo),
        cargo_denies=_parse_cargo_denies(root, cargo),
        cargo_vets=_parse_cargo_vets(root, cargo),
        python_audits=_parse_python(root, data.get("python")),
        node_audits=_parse_node(root, data.get("node")),
        osv=_parse_osv(root, data.get("osv")),
    )
    if not any(
        (
            policy.cargo_audits,
            policy.cargo_denies,
            policy.cargo_vets,
            policy.python_audits,
            policy.node_audits,
            policy.osv,
        ),
    ):
        raise AuditError(f"{config_path}: no audits are configured")
    return policy


def _catalog_path(root: Path) -> Path:
    shared = root / ".nautilus-engineering" / "tools.toml"
    return shared if shared.is_file() else root / "tools.toml"


def _read_versions(root: Path, names: set[str]) -> dict[str, str]:
    catalog_path = _catalog_path(root)
    catalog = _read_toml(catalog_path)
    local_path = root / "tools.toml"
    local = _read_toml(local_path) if local_path != catalog_path and local_path.is_file() else {}
    versions = {}
    for name in sorted(names):
        if name in local:
            raise AuditError(
                f"duplicate [{name}] table in shared catalog and {local_path.relative_to(root)}",
            )
        section = catalog.get(name)
        if not isinstance(section, dict):
            raise AuditError(f"{catalog_path}: missing [{name}] table")
        version = section.get("version")
        if not isinstance(version, str) or not version:
            raise AuditError(f"{catalog_path}: [{name}].version must be a non-empty string")
        if STABLE_VERSION.fullmatch(version) is None:
            raise AuditError(
                f"{catalog_path}: [{name}].version must be a stable X.Y.Z release",
            )
        versions[name] = version
    return versions


def _resolve_executable(name: str) -> str:
    executable = shutil.which(name)
    if executable is None:
        raise AuditError(f"required command is not installed: {name}")
    return str(Path(executable).resolve())


def _run_process(
    args: list[str],
    *,
    root: Path,
    cwd: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(  # noqa: S603
            args,
            cwd=cwd or root,
            check=False,
            capture_output=True,
            encoding="utf-8",
            errors="replace",
        )
    except OSError as error:
        raise AuditError(f"could not execute {args[0]}: {error}") from error


def _output(result: subprocess.CompletedProcess[str]) -> str:
    return "\n".join(part.strip() for part in (result.stdout, result.stderr) if part.strip())


def _reported_version(output: str) -> str | None:
    match = REPORTED_VERSION.search(output)
    return match.group(1) if match is not None else None


def _check_version(context: Context, label: str, args: list[str], expected: str) -> None:
    result = _run_process(args, root=context.root)
    output = _output(result)
    if result.returncode != 0:
        raise AuditError(f"could not read {label} version:\n{output}")
    reported = _reported_version(output)
    if reported != expected:
        found = reported or output or "no output"
        raise AuditError(f"{label} version mismatch: expected {expected}, found {found}")
    _write(f"ok   {label} {expected}")


# Tool selection stays together so version checks cannot drift from enabled audit sections
def _prepare_context(root: Path, policy: AuditPolicy) -> Context:  # noqa: C901, PLR0912
    tools = set()
    executables = {}
    if policy.cargo_audits or policy.cargo_denies or policy.cargo_vets:
        executables["cargo"] = _resolve_executable("cargo")
    if policy.cargo_audits:
        tools.add("cargo-audit")
    if policy.cargo_denies:
        tools.add("cargo-deny")
    if policy.cargo_vets:
        tools.add("cargo-vet")
    if policy.python_audits:
        tools.update({"pip-audit", "uv"})
        executables["uv"] = _resolve_executable("uv")
    if policy.node_audits:
        executables["npm"] = _resolve_executable("npm")
    if policy.osv is not None:
        tools.add("osv-scanner")
        executables["osv-scanner"] = _resolve_executable("osv-scanner")
    versions = _read_versions(root, tools)
    context = Context(root=root, versions=versions, executables=executables)
    cargo = executables.get("cargo")
    if policy.cargo_audits and cargo is not None:
        _check_version(
            context,
            "cargo-audit",
            [cargo, "audit", "--version"],
            versions["cargo-audit"],
        )
    if policy.cargo_denies and cargo is not None:
        _check_version(context, "cargo-deny", [cargo, "deny", "--version"], versions["cargo-deny"])
    if policy.cargo_vets and cargo is not None:
        _check_version(context, "cargo-vet", [cargo, "vet", "--version"], versions["cargo-vet"])
    uv = executables.get("uv")
    if policy.python_audits and uv is not None:
        _check_version(context, "uv", [uv, "--version"], versions["uv"])
        _check_version(
            context,
            "pip-audit",
            [
                uv,
                "run",
                "--no-project",
                "--no-build",
                "--with",
                f"pip-audit=={versions['pip-audit']}",
                "--",
                "pip-audit",
                "--version",
            ],
            versions["pip-audit"],
        )
    if policy.node_audits:
        npm = executables["npm"]
        result = _run_process([npm, "--version"], root=root)
        if result.returncode != 0:
            output = _output(result) or "no output"
            raise AuditError(f"could not run npm: {output}")
        _write("ok   npm is available")
    if policy.osv is not None:
        osv = executables["osv-scanner"]
        _check_version(context, "osv-scanner", [osv, "--version"], versions["osv-scanner"])
    return context


def _run_step(
    context: Context,
    label: str,
    args: list[str],
    *,
    options: StepOptions = DEFAULT_STEP_OPTIONS,
) -> None:
    _write(f"run  {label}")
    result = _run_process(args, root=context.root, cwd=options.cwd)
    output = _output(result)
    if output and (options.report or result.returncode != 0):
        _write(output, error=result.returncode != 0)
    if result.returncode != 0 and options.gate:
        raise AuditError(f"{label} failed with exit code {result.returncode}")
    if result.returncode != 0:
        _write(f"warn {label} reported findings", error=True)
    else:
        _write(f"ok   {label}")


# Each Cargo command is assembled beside its typed policy to keep flags and paths auditable
def _run_cargo(context: Context, policy: AuditPolicy) -> list[str]:  # noqa: C901
    failures = []
    cargo = context.executables.get("cargo")
    if cargo is None:
        return failures
    for audit in policy.cargo_audits:
        label = f"cargo audit {audit.lockfile.as_posix()}"
        args = [cargo, "audit", "--color", "never", "--file", audit.lockfile.as_posix()]
        for advisory in audit.ignores:
            args.extend(("--ignore", advisory))
        for warning in audit.denies:
            args.extend(("--deny", warning))
        try:
            _run_step(context, label, args, options=StepOptions(report=audit.report))
        except AuditError as error:
            failures.append(str(error))
    for audit in policy.cargo_denies:
        label = f"cargo deny {audit.manifest.as_posix()}"
        args = [cargo, "deny", "--manifest-path", audit.manifest.as_posix()]
        if audit.config is not None:
            args.extend(("--config", audit.config.as_posix()))
        if audit.all_features:
            args.append("--all-features")
        args.append("--locked")
        args.extend(("check", *audit.checks))
        try:
            _run_step(context, label, args)
        except AuditError as error:
            failures.append(str(error))
    for audit in policy.cargo_vets:
        label = f"cargo vet {audit.manifest.as_posix()}"
        args = [cargo, "vet", "--manifest-path", audit.manifest.as_posix()]
        if audit.store is not None:
            args.extend(("--store-path", audit.store.as_posix()))
        args.append("--locked")
        try:
            _run_step(context, label, args)
        except AuditError as error:
            failures.append(str(error))
    return failures


def _run_python_audit(context: Context, audit: PythonAudit) -> None:
    uv = context.executables["uv"]
    with tempfile.TemporaryDirectory(prefix="nautilus-security-audit-") as temp:
        temp_root = Path(temp)
        requirements = temp_root / "requirements.txt"
        export = [
            uv,
            "export",
            "--project",
            audit.project.as_posix(),
            "--python",
            audit.python,
            "--no-emit-local",
            "--frozen",
        ]
        if audit.all_extras:
            export.append("--all-extras")
        if audit.all_groups:
            export.append("--all-groups")
        for group in audit.groups:
            export.extend(("--group", group))
        result = _run_process(export, root=context.root)
        if result.returncode != 0:
            output = _output(result)
            if output:
                _write(output, error=True)
            raise AuditError(
                f"uv export {audit.project.as_posix()} failed with exit code {result.returncode}",
            )
        requirements.write_text(result.stdout, encoding="utf-8")
        cache = temp_root / "cache"
        cache.mkdir()
        args = [
            uv,
            "run",
            "--python",
            audit.python,
            "--no-project",
            "--no-build",
            "--with",
            f"pip-audit=={context.versions['pip-audit']}",
            "--",
            "pip-audit",
            "--cache-dir",
            str(cache),
            "--progress-spinner",
            "off",
            "--disable-pip",
            "--require-hashes",
            "-r",
            str(requirements),
        ]
        for vulnerability in audit.ignores:
            args.extend(("--ignore-vuln", vulnerability))
        _run_step(context, f"pip-audit {audit.project.as_posix()}", args)


def _run_python(context: Context, policy: AuditPolicy) -> list[str]:
    failures = []
    for audit in policy.python_audits:
        try:
            _run_python_audit(context, audit)
        except AuditError as error:
            failures.append(str(error))
    return failures


def _run_node(context: Context, policy: AuditPolicy) -> list[str]:
    failures = []
    npm = context.executables.get("npm")
    if npm is None:
        return failures
    for audit in policy.node_audits:
        cwd = context.root / audit.project
        commands = []
        if audit.production:
            commands.append(("npm audit production", [npm, "audit", "--omit=dev"], False, True))
        if audit.full != "off":
            commands.append(
                (
                    "npm audit full",
                    [npm, "audit"],
                    audit.full == "report",
                    audit.full == "gate",
                ),
            )
        if audit.signatures:
            commands.append(
                (
                    "npm audit signatures",
                    [npm, "audit", "signatures", "--min-release-age=0"],
                    False,
                    True,
                ),
            )
        for label, args, report, gate in commands:
            try:
                _run_step(
                    context,
                    f"{label} {audit.project.as_posix()}",
                    args,
                    options=StepOptions(cwd=cwd, report=report, gate=gate),
                )
            except AuditError as error:
                failures.append(str(error))
    return failures


def _run_osv(context: Context, policy: AuditPolicy) -> list[str]:
    if policy.osv is None:
        return []
    args = [context.executables["osv-scanner"], "scan", "source"]
    if policy.osv.config is not None:
        args.append(f"--config={policy.osv.config.as_posix()}")
    args.extend(f"--lockfile={path.as_posix()}" for path in policy.osv.lockfiles)
    try:
        _run_step(
            context,
            "osv-scanner",
            args,
            options=StepOptions(report=policy.osv.report),
        )
    except AuditError as error:
        return [str(error)]
    return []


def _run_audits(context: Context, policy: AuditPolicy) -> None:
    failures = [
        *_run_cargo(context, policy),
        *_run_python(context, policy),
        *_run_node(context, policy),
        *_run_osv(context, policy),
    ]
    if failures:
        details = "\n".join(f"- {failure}" for failure in failures)
        raise AuditError(f"{len(failures)} audit step(s) failed:\n{details}")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check-tools", "run", "validate"))
    parser.add_argument("--config", type=Path, default=Path("security-audit.toml"))
    parser.add_argument("--root", type=Path)
    return parser.parse_args()


def _config_path(root: Path, value: Path) -> Path:
    candidate = value if value.is_absolute() else root / value
    relative = candidate.resolve().relative_to(root)
    return root / _path(root, relative.as_posix(), "config")


def main() -> int:
    args = _parse_args()
    root = (args.root or Path(__file__).resolve().parents[1]).resolve()
    if not root.is_dir():
        _write(f"Security audit failed: repository root is not a directory: {root}", error=True)
        return 1
    try:
        config = _config_path(root, args.config)
        policy = _read_policy(root, config)
        if args.command == "validate":
            _write(f"Security audit policy is valid: {config.relative_to(root)}")
            return 0
        context = _prepare_context(root, policy)
        if args.command == "check-tools":
            _write("All required supply-chain tools match the central catalog")
            return 0
        _run_audits(context, policy)
    except (AuditError, ValueError) as error:
        _write(f"Security audit failed: {error}", error=True)
        return 1
    _write("All configured supply-chain audits passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
