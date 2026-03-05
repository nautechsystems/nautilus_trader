from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import pytest


SCRIPT_UNDER_TEST = Path("scripts/ci/check-flux-leakage.sh")
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


def _base_redis_schema_doc() -> str:
    return "# Redis Schema\nNo legacy mapping is supported.\n\n"


def _init_temp_repo(tmp_path: Path, redis_schema_doc: str) -> Path:
    repo_root = tmp_path / "repo"
    repo_root.mkdir()

    source_repo_root = _locate_source_repo_root()
    script_contents = (source_repo_root / SCRIPT_UNDER_TEST).read_text()

    script_path = repo_root / SCRIPT_UNDER_TEST
    script_path.parent.mkdir(parents=True)
    script_path.write_text(script_contents)

    (repo_root / "nautilus_trader/flux").mkdir(parents=True)
    (repo_root / "docs/flux").mkdir(parents=True)
    (repo_root / "docs/flux/params.md").write_text("# Params\n")
    (repo_root / "docs/flux/bridge.md").write_text("# Bridge\n")
    (repo_root / "docs/flux/api.md").write_text("# API\n")
    (repo_root / "docs/flux/redis_schema.md").write_text(redis_schema_doc)

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
