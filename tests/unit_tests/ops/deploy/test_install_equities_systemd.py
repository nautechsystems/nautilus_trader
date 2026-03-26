from __future__ import annotations

import os
import stat
import subprocess
from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def _make_release_root(root: Path, *, strategy_id: str = "aapl_tradexyz_makerv4") -> None:
    _write(root / "deploy/equities/equities.live.toml", "[api]\nstrategy_class = 'maker_v4'\n")
    _write(root / f"deploy/equities/strategies/{strategy_id}.toml", "[node]\n")
    _write(root / ".flux-release/release.env", "DEPLOY_LANE=prod\nSTACK_NAME=equities\nRELEASE_ID=test\n")
    python_bin = root / ".venv/bin/python"
    _write(python_bin, "#!/usr/bin/env bash\nexit 0\n")
    python_bin.chmod(python_bin.stat().st_mode | stat.S_IXUSR)


def _run_installer_snippet(snippet: str, *, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    repo_root = _repo_root()
    script_path = repo_root / "ops/scripts/deploy/install_equities_systemd.sh"
    return subprocess.run(  # noqa: S603 - controlled test invocation of repo shell helper
        [
            "/usr/bin/bash",
            "-lc",
            f'source "{script_path}"\n{snippet}\n',
        ],
        check=False,
        capture_output=True,
        text=True,
        cwd=repo_root,
        env=env,
    )


def test_resolve_deploy_root_honors_explicit_release_root(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/prod/equities/current"
    _make_release_root(deploy_root)
    _write(common_env_path, "WORKDIR=/tmp/old-common-root\n")

    result = _run_installer_snippet(
        "resolve_deploy_root\n",
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(deploy_root),
        },
    )

    assert result.returncode == 0
    assert result.stdout.strip() == str(deploy_root)


def test_resolve_deploy_root_preserves_existing_service_root_on_rerun(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    stable_root = tmp_path / "releases/prod/equities/current"
    _make_release_root(stable_root)
    _write(common_env_path, "WORKDIR=/tmp/dev-checkout\n")
    _write(env_dir / "equities-api.env", f"WORKDIR={stable_root}\n")

    result = _run_installer_snippet(
        "resolve_deploy_root\n",
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
        },
    )

    assert result.returncode == 0
    assert result.stdout.strip() == str(stable_root)


def test_require_deploy_root_rejects_git_checkout(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    checkout_root = tmp_path / "checkout"
    checkout_root.mkdir(parents=True)
    subprocess.run(  # noqa: S603 - controlled test setup
        ["git", "init", str(checkout_root)],
        check=True,
        capture_output=True,
        text=True,
    )

    result = _run_installer_snippet(
        "initialize_stack_context\n",
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(checkout_root),
        },
    )

    assert result.returncode != 0
    assert "must not be a git checkout" in result.stderr


def test_render_pilot_envs_use_lane_aware_service_ids(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/pilot/equities/current"
    strategy_id = "aapl_tradexyz_makerv4"
    env_dir.mkdir(parents=True)
    _make_release_root(deploy_root, strategy_id=strategy_id)

    result = _run_installer_snippet(
        "\n".join(
            [
                "initialize_stack_context",
                "discover_node_strategies",
                "render_api_env",
                "render_portfolio_env",
                "render_bridge_env",
                "render_node_envs",
            ],
        ),
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(deploy_root),
            "EQUITIES_DEPLOY_LANE": "pilot",
        },
    )

    assert result.returncode == 0

    api_env = (env_dir / "equities-pilot-api.env").read_text(encoding="utf-8")
    portfolio_env = (env_dir / "equities-pilot-portfolio.env").read_text(encoding="utf-8")
    bridge_env = (env_dir / "equities-pilot-bridge.env").read_text(encoding="utf-8")
    node_env = (env_dir / f"equities-pilot-node-{strategy_id}.env").read_text(encoding="utf-8")

    assert "PULSE_GROUP_KEY=equities-pilot" in api_env
    assert "PULSE_GROUP_LABEL=Equities Pilot" in api_env
    assert "PULSE_SELF_SERVICE_ID=equities-pilot-api" in api_env
    assert "PORT=5124" in api_env
    assert f"WORKDIR={deploy_root}" in api_env
    assert f"PYTHONPATH={deploy_root}" in api_env

    assert "PULSE_GROUP_KEY=equities-pilot" in portfolio_env
    assert "PULSE_GROUP_KEY=equities-pilot" in bridge_env
    assert "PULSE_GROUP_KEY=equities-pilot" in node_env
    assert f"{deploy_root}/.venv/bin/python" in node_env
