from __future__ import annotations

import subprocess
from pathlib import Path

import pytest


SCRIPT_UNDER_TEST = Path("scripts/ci/check-flux-leakage.sh")
ALLOWLIST_START = "<!-- leakage-allowlist:start maker_poc_migration -->"
ALLOWLIST_END = "<!-- leakage-allowlist:end maker_poc_migration -->"


def _base_redis_schema_doc() -> str:
    return "\n".join(
        [
            "# Redis Schema",
            ALLOWLIST_START,
            "Legacy key mapping: maker_poc.state -> flux:v1:state:{strategy_id}",
            ALLOWLIST_END,
            "",
        ],
    )


def _init_temp_repo(tmp_path: Path, redis_schema_doc: str) -> Path:
    repo_root = tmp_path / "repo"
    repo_root.mkdir()

    source_repo_root = Path(__file__).resolve().parents[3]
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

    subprocess.run(["git", "init", "-q"], cwd=repo_root, check=True)
    return repo_root


def _run_check(repo_root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(SCRIPT_UNDER_TEST)],
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


def test_check_flux_leakage_fails_when_banned_names_are_outside_allowlist(tmp_path: Path):
    redis_schema_doc = "\n".join(
        [
            "# Redis Schema",
            "This mapping still uses maker_poc and must fail.",
            ALLOWLIST_START,
            "Legacy key mapping: maker_poc.state -> flux:v1:state:{strategy_id}",
            ALLOWLIST_END,
            "",
        ],
    )
    repo_root = _init_temp_repo(tmp_path, redis_schema_doc)

    result = _run_check(repo_root)

    assert result.returncode != 0
    assert "outside allowlist markers" in result.stderr


@pytest.mark.parametrize(
    ("redis_schema_doc", "error_snippet"),
    [
        (
            "\n".join(
                [
                    "# Redis Schema",
                    ALLOWLIST_START,
                    "Legacy key mapping: maker_poc.state -> flux:v1:state:{strategy_id}",
                    "",
                ],
            ),
            "Expected exactly one redis_schema allowlist start/end marker pair",
        ),
        (
            "\n".join(
                [
                    "# Redis Schema",
                    ALLOWLIST_END,
                    ALLOWLIST_START,
                    "Legacy key mapping: maker_poc.state -> flux:v1:state:{strategy_id}",
                    "",
                ],
            ),
            "allowlist markers are out of order",
        ),
    ],
)
def test_check_flux_leakage_fails_when_allowlist_markers_are_invalid(
    tmp_path: Path,
    redis_schema_doc: str,
    error_snippet: str,
):
    repo_root = _init_temp_repo(tmp_path, redis_schema_doc)

    result = _run_check(repo_root)

    assert result.returncode != 0
    assert error_snippet in result.stderr
