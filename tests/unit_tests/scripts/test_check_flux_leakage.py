from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import pytest


SCRIPT_UNDER_TEST = Path("tooling/ci/check-flux-leakage.sh")
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


def _base_redis_schema_doc() -> str:
    return "# Redis Schema\nNo legacy mapping is supported.\n\n"


def _init_temp_repo(tmp_path: Path, redis_schema_doc: str) -> Path:
    repo_root = tmp_path / "repo"
    repo_root.mkdir()

    source_repo_root = _locate_source_repo_root()
    script_contents = (source_repo_root / SCRIPT_UNDER_TEST).read_text()

    script_path = repo_root / SCRIPT_UNDER_TEST
    _write(script_path, script_contents)

    _write(repo_root / "nautilus_trader/flux/__init__.py", '"""shim"""\n')
    _write(repo_root / "systems/flux/docs/params.md", "# Params\n")
    _write(repo_root / "systems/flux/docs/bridge.md", "# Bridge\n")
    _write(repo_root / "systems/flux/docs/api.md", "# API\n")
    _write(repo_root / "systems/flux/docs/redis_schema.md", redis_schema_doc)
    _write(repo_root / "apps/fluxboard/docs/tokenmm_contract.md", "# Contract\n")
    _write(repo_root / "apps/fluxboard/docs/tokenmm_socket_contract.md", "# Socket contract\n")
    _write(repo_root / "apps/fluxboard/docs/tokenmm_runbook.md", "# Runbook\n")

    _symlink(repo_root / "docs/flux", "../systems/flux/docs")
    _symlink(repo_root / "docs/fluxboard", "../apps/fluxboard/docs")

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


def test_check_flux_leakage_passes_when_banned_names_are_inside_allowlist_only(tmp_path: Path):
    repo_root = _init_temp_repo(tmp_path, _base_redis_schema_doc())

    result = _run_check(repo_root)

    assert result.returncode == 0, result.stderr
    assert "[flux-leakage] OK" in result.stdout


def test_check_flux_leakage_fails_when_banned_names_are_present(tmp_path: Path):
    redis_schema_doc = "# Redis Schema\nThis document references chainsaw and must fail.\n\n"
    repo_root = _init_temp_repo(tmp_path, redis_schema_doc)

    result = _run_check(repo_root)

    assert result.returncode != 0
    assert "Found forbidden" in result.stderr
