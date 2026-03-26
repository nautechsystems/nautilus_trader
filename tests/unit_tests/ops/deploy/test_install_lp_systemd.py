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


def _make_release_root(root: Path) -> None:
    _write(root / "deploy/lp/lp.live.toml", "[api]\n")
    _write(root / "deploy/lp/hedgers/eth_plume_lp_hedger.ini", "[hedger]\n")
    _write(root / "deploy/lp/hedgers/eth_plume_lp_hedger_band2.ini", "[hedger]\n")
    _write(root / "deploy/lp/hedgers/hype_usdt_lp_hedger.ini", "[hedger]\n")
    _write(root / "deploy/lp/hedgers/plume_weth_lp_hedger.ini", "[hedger]\n")
    _write(root / "deploy/lp/systemd/common.env.example", "WORKDIR=/tmp/ignored\n")
    _write(root / ".flux-release/release.env", "DEPLOY_LANE=prod\nSTACK_NAME=lp\nRELEASE_ID=test\n")
    python_bin = root / ".venv/bin/python"
    _write(python_bin, "#!/usr/bin/env bash\nexit 0\n")
    python_bin.chmod(python_bin.stat().st_mode | stat.S_IXUSR)


def _run_installer_snippet(snippet: str, *, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    repo_root = _repo_root()
    script_path = repo_root / "ops/scripts/deploy/install_lp_systemd.sh"
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
    deploy_root = tmp_path / "releases/prod/lp/current"
    _make_release_root(deploy_root)
    _write(common_env_path, "WORKDIR=/tmp/old-common-root\n")

    result = _run_installer_snippet(
        "resolve_deploy_root\n",
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "LP_DEPLOY_ROOT": str(deploy_root),
        },
    )

    assert result.returncode == 0
    assert result.stdout.strip() == str(deploy_root)


def test_require_deploy_root_rejects_git_checkout(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    checkout_root = tmp_path / "checkout"
    checkout_root.mkdir(parents=True)
    subprocess.run(
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
            "LP_DEPLOY_ROOT": str(checkout_root),
        },
    )

    assert result.returncode != 0
    assert "must not be a git checkout" in result.stderr


def test_render_lp_envs_use_release_root_python_and_env_overrides(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    systemd_dir = tmp_path / "etc" / "systemd" / "system"
    sudoers_dir = tmp_path / "etc" / "sudoers.d"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/prod/lp/current"
    env_dir.mkdir(parents=True)
    systemd_dir.mkdir(parents=True)
    sudoers_dir.mkdir(parents=True)
    _make_release_root(deploy_root)

    result = _run_installer_snippet(
        "\n".join(
            [
                "initialize_stack_context",
                "render_api_env",
                "render_band1_env",
                "render_band2_env",
                "render_hype_env",
                "render_plume_weth_env",
            ],
        ),
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "SYSTEMD_DIR": str(systemd_dir),
            "ENV_DIR": str(env_dir),
            "SUDOERS_DIR": str(sudoers_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "LP_DEPLOY_ROOT": str(deploy_root),
            "FLUX_DEPLOY_TEST_MODE": "1",
        },
    )

    assert result.returncode == 0
    api_env = (env_dir / "lp-api.env").read_text(encoding="utf-8")
    band1_env = (env_dir / "service-eth-plume-lp-hedger.env").read_text(encoding="utf-8")

    assert f'{deploy_root}/.venv/bin/python -m lp.runners.run_api --config {deploy_root}/deploy/lp/lp.live.toml' in api_env
    assert f"WORKDIR={deploy_root}" in api_env
    assert f"PYTHONPATH={deploy_root}" in api_env
    assert f'{deploy_root}/.venv/bin/python -m lp.runners.run_hedger --config {deploy_root}/deploy/lp/hedgers/eth_plume_lp_hedger.ini' in band1_env
    assert f"WORKDIR={deploy_root}" in band1_env
    assert f"PYTHONPATH={deploy_root}" in band1_env
