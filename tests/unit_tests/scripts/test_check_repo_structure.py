from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import pytest


SCRIPT_UNDER_TEST = Path("tooling/ci/check-repo-structure.sh")
REQUIRED_BINARIES = ("git", "bash", "rg")
BINARIES = {binary: shutil.which(binary) for binary in REQUIRED_BINARIES}
MISSING_BINARIES = [binary for binary, path in BINARIES.items() if path is None]

GIT = BINARIES.get("git") or "git"
BASH = BINARIES.get("bash") or "bash"

pytestmark = pytest.mark.skipif(
    bool(MISSING_BINARIES),
    reason=f"Missing required binaries: {', '.join(MISSING_BINARIES)}",
)


def _locate_source_repo_root() -> Path:
    for parent in Path(__file__).resolve().parents:
        if (parent / ".git").exists() and (parent / SCRIPT_UNDER_TEST).is_file():
            return parent
    raise FileNotFoundError(
        f"Could not locate repository root with {SCRIPT_UNDER_TEST} while walking parent directories",
    )


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def _symlink(path: Path, target: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.symlink_to(target)


def _init_temp_repo(tmp_path: Path) -> Path:
    repo_root = tmp_path / "repo"
    repo_root.mkdir()

    source_repo_root = _locate_source_repo_root()
    script_contents = (source_repo_root / SCRIPT_UNDER_TEST).read_text()

    script_path = repo_root / SCRIPT_UNDER_TEST
    _write(script_path, script_contents)

    _write(repo_root / "README.md", "# Repo\n")
    _write(repo_root / "CONTRIBUTING.md", "# Contributing\n")
    _write(repo_root / "docs/repo/structure.md", "# Structure\n")
    _write(repo_root / "docs/repo/workflows.md", "# Workflows\n")
    _write(repo_root / "docs/repo/standards.md", "# Standards\n")
    _write(repo_root / "docs/developer_guide/index.md", "# Developer guide\n")
    _write(repo_root / "fluxboard/README.md", "# Fluxboard\n")
    _write(repo_root / "tooling/README.md", "# Tooling\n")
    _write(repo_root / "ops/README.md", "# Ops\n")
    _write(repo_root / "scripts/README.md", "# Legacy scripts\n")
    _write(repo_root / ".pre-commit-config.yaml", "repos: []\n")

    _write(repo_root / "tooling/ci/check-flux-leakage.sh", "#!/usr/bin/env bash\n")
    _write(repo_root / "ops/scripts/deploy/tokenmm_stack.sh", "#!/usr/bin/env bash\n")
    _write(repo_root / "ops/scripts/deploy/install_tokenmm_systemd.sh", "#!/usr/bin/env bash\n")

    _write(repo_root / "systems/flux/docs/api.md", "# API\n")
    _write(repo_root / "systems/flux/docs/bridge.md", "# Bridge\n")
    _write(repo_root / "systems/flux/docs/params.md", "# Params\n")
    _write(repo_root / "systems/flux/docs/redis_schema.md", "# Redis schema\n")
    _write(repo_root / "apps/fluxboard/docs/tokenmm_runbook.md", "# Runbook\n")
    _write(repo_root / "nautilus_trader/flux/__init__.py", '"""shim"""\n')

    _symlink(repo_root / "docs/flux", "../systems/flux/docs")
    _symlink(repo_root / "docs/fluxboard", "../apps/fluxboard/docs")
    _symlink(repo_root / "scripts/ci/check-flux-leakage.sh", "../../tooling/ci/check-flux-leakage.sh")
    _symlink(repo_root / "scripts/deploy/tokenmm_stack.sh", "../../ops/scripts/deploy/tokenmm_stack.sh")
    _symlink(
        repo_root / "scripts/deploy/install_tokenmm_systemd.sh",
        "../../ops/scripts/deploy/install_tokenmm_systemd.sh",
    )

    subprocess.run([GIT, "init", "-q"], cwd=repo_root, check=True)  # noqa: S603
    return repo_root


def _run_check(repo_root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603
        [BASH, str(SCRIPT_UNDER_TEST)],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )


def test_check_repo_structure_passes_for_canonical_layout(tmp_path: Path):
    repo_root = _init_temp_repo(tmp_path)

    result = _run_check(repo_root)

    assert result.returncode == 0, result.stderr
    assert "[repo-structure] OK" in result.stdout


def test_check_repo_structure_fails_for_real_file_under_legacy_scripts(tmp_path: Path):
    repo_root = _init_temp_repo(tmp_path)
    _write(repo_root / "scripts/ci/rogue.sh", "#!/usr/bin/env bash\n")

    result = _run_check(repo_root)

    assert result.returncode != 0
    assert "legacy scripts/ compatibility tree" in result.stderr


def test_check_repo_structure_fails_for_legacy_flux_implementation_file(tmp_path: Path):
    repo_root = _init_temp_repo(tmp_path)
    _write(repo_root / "nautilus_trader/flux/api.py", "value = 1\n")

    result = _run_check(repo_root)

    assert result.returncode != 0
    assert "legacy nautilus_trader/flux compatibility path" in result.stderr


def test_check_repo_structure_fails_for_legacy_path_reference_in_active_docs(tmp_path: Path):
    repo_root = _init_temp_repo(tmp_path)
    _write(repo_root / "README.md", "See scripts/deploy/tokenmm_stack.sh for details.\n")

    result = _run_check(repo_root)

    assert result.returncode != 0
    assert "legacy repo path references" in result.stderr
