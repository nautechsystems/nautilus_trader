from __future__ import annotations

import os
import subprocess
from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _write_env(path: Path, lines: list[str]) -> None:
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def test_rebuild_flux_pulse_sudoers_rejects_invalid_service_id(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    sudoers_path = tmp_path / "sudoers.d" / "flux-pulse"

    _write_env(
        env_dir / "foo,bar#.env",
        [
            "PULSE_ENABLED=1",
            "PULSE_DESCRIPTION=malicious",
        ],
    )

    script_path = _repo_root() / "ops/scripts/deploy/rebuild_flux_pulse_sudoers.sh"
    result = subprocess.run(  # noqa: S603 - test executes a repo-controlled helper script path
        ["/usr/bin/bash", str(script_path)],
        check=False,
        capture_output=True,
        text=True,
        env={
            **os.environ,
            "ENV_DIR": str(env_dir),
            "SUDOERS_PATH": str(sudoers_path),
            "RUN_AS_USER": "ubuntu",
        },
    )

    assert result.returncode != 0
    assert "invalid service ID" in result.stderr
    assert not sudoers_path.exists()


def test_strategy_stack_discover_strategy_ids_rejects_invalid_strategy_id(tmp_path: Path) -> None:
    strategies_dir = tmp_path / "strategies"
    strategies_dir.mkdir()
    (strategies_dir / "safe_strategy.toml").write_text("", encoding="utf-8")
    (strategies_dir / "bad;rm-rf.toml").write_text("", encoding="utf-8")

    repo_root = _repo_root()
    script_path = repo_root / "ops/scripts/deploy/shared_strategy_stack.sh"
    result = subprocess.run(  # noqa: S603 - controlled test invocation of a repo shell helper
        [
            "/usr/bin/bash",
            "-lc",
            f'source "{script_path}"\nstrategy_stack_discover_strategy_ids "{strategies_dir}"\n',
        ],
        check=False,
        capture_output=True,
        text=True,
        cwd=repo_root,
        env=os.environ,
    )

    assert result.returncode != 0
    assert "invalid strategy ID" in result.stderr


def test_strategy_stack_require_immutable_release_root_rejects_git_checkout(tmp_path: Path) -> None:
    release_root = tmp_path / "release"
    release_root.mkdir()
    subprocess.run(  # noqa: S603 - controlled test setup
        ["git", "init", str(release_root)],
        check=True,
        capture_output=True,
        text=True,
    )

    repo_root = _repo_root()
    script_path = repo_root / "ops/scripts/deploy/shared_strategy_stack.sh"
    result = subprocess.run(  # noqa: S603 - controlled test invocation of repo shell helper
        [
            "/usr/bin/bash",
            "-lc",
            f'source "{script_path}"\nstrategy_stack_require_immutable_release_root "{release_root}"\n',
        ],
        check=False,
        capture_output=True,
        text=True,
        cwd=repo_root,
        env=os.environ,
    )

    assert result.returncode != 0
    assert "must not be a git checkout" in result.stderr


def test_strategy_stack_require_immutable_release_root_accepts_metadata_backed_dir(
    tmp_path: Path,
) -> None:
    release_root = tmp_path / "release"
    release_root.mkdir()
    metadata_dir = release_root / ".flux-release"
    metadata_dir.mkdir()
    (metadata_dir / "release.env").write_text(
        "DEPLOY_LANE=prod\nSTACK_NAME=equities\nRELEASE_ID=test123\n",
        encoding="utf-8",
    )

    repo_root = _repo_root()
    script_path = repo_root / "ops/scripts/deploy/shared_strategy_stack.sh"
    result = subprocess.run(  # noqa: S603 - controlled test invocation of repo shell helper
        [
            "/usr/bin/bash",
            "-lc",
            f'source "{script_path}"\nstrategy_stack_require_immutable_release_root "{release_root}"\n',
        ],
        check=False,
        capture_output=True,
        text=True,
        cwd=repo_root,
        env=os.environ,
    )

    assert result.returncode == 0
