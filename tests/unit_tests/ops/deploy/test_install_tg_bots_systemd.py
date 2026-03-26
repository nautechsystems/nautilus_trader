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
    _write(root / "deploy/tg_bots/lan_rogue_trader_alert.ini", "[bot]\n")
    _write(root / "deploy/tg_bots/systemd/common.env.example", "WORKDIR=/tmp/ignored\n")
    _write(root / "deploy/tg_bots/systemd/flux-tg-bots.target", "[Unit]\nDescription=Flux TG Bots\n")
    _write(root / ".flux-release/release.env", "DEPLOY_LANE=prod\nSTACK_NAME=tg_bots\nRELEASE_ID=test\n")
    python_bin = root / ".venv/bin/python"
    _write(python_bin, "#!/usr/bin/env bash\nexit 0\n")
    python_bin.chmod(python_bin.stat().st_mode | stat.S_IXUSR)


def test_install_tg_bots_systemd_seeds_readable_local_config() -> None:
    script = (_repo_root() / "ops/scripts/deploy/install_tg_bots_systemd.sh").read_text(
        encoding="utf-8"
    )

    assert 'install -m 0644 "${DEPLOY_ROOT}/deploy/tg_bots/lan_rogue_trader_alert.ini" "${LOCAL_CONFIG_PATH}"' in script
    assert 'SERVICE_ENV_OWNER="${SERVICE_ENV_OWNER-root:ubuntu}"' in script
    assert 'chown "${SERVICE_ENV_OWNER}" "${SERVICE_ENV_PATH}"' in script
    assert 'chmod "${SERVICE_ENV_MODE}" "${SERVICE_ENV_PATH}"' in script


def test_render_service_env_preserves_existing_live_secrets_and_custom_env(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    service_env_path = env_dir / "tg-bot-lan-rogue-trader-alert.env"
    local_config_path = env_dir / "tg-bot-lan-rogue-trader-alert.ini"
    deploy_root = tmp_path / "releases/prod/tg_bots/current"
    _make_release_root(deploy_root)
    service_env_path.write_text(
        """PULSE_ENABLED=1
PULSE_DESCRIPTION=Old description
PULSE_GROUP_KEY=tg-bots
PULSE_GROUP_LABEL=TG Bots
PULSE_GROUP_ORDER=60
CMD="python3 -m old.runner"
LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY=live-key
LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET=live-secret
LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN=live-token
LAN_ROGUE_TRADER_BOT_BINANCE_SECRET_ID=/existing/binance
LAN_ROGUE_TRADER_BOT_TELEGRAM_SECRET_ID=/existing/telegram
FLUX_TG_BOTS_LOG_LEVEL=DEBUG
""",
        encoding="utf-8",
    )

    env = dict(os.environ)
    env.update(
        {
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(env_dir / "common.env"),
            "SERVICE_ENV_PATH": str(service_env_path),
            "LOCAL_CONFIG_PATH": str(local_config_path),
            "TG_BOTS_DEPLOY_ROOT": str(deploy_root),
            "SERVICE_ENV_OWNER": "",
        }
    )
    subprocess.run(  # noqa: S603 - controlled test invocation of the repo installer helper
            [
                "/usr/bin/bash",
                "-lc",
                f'source "{repo_root / "ops/scripts/deploy/install_tg_bots_systemd.sh"}"\ninitialize_stack_context\nrender_service_env\n',
            ],
            check=True,
            env=env,
        cwd=repo_root,
    )

    rendered_env = service_env_path.read_text(encoding="utf-8")

    assert "PULSE_DESCRIPTION=Lan Rogue Trader Telegram alert bot" in rendered_env
    assert (
        f'CMD="{deploy_root}/.venv/bin/python -m nautilus_trader.flux.runners.tg_bots.run_lan_rogue_trader_alert '
        f'--config {local_config_path}"'
    ) in rendered_env
    assert "LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY=live-key" in rendered_env
    assert "LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET=live-secret" in rendered_env
    assert "LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN=live-token" in rendered_env
    assert "LAN_ROGUE_TRADER_BOT_BINANCE_SECRET_ID=/existing/binance" in rendered_env
    assert "LAN_ROGUE_TRADER_BOT_TELEGRAM_SECRET_ID=/existing/telegram" in rendered_env
    assert "FLUX_TG_BOTS_LOG_LEVEL=DEBUG" in rendered_env
    assert f"WORKDIR={deploy_root}" in rendered_env
    assert f"PYTHONPATH={deploy_root}" in rendered_env
    assert "LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY=\n" not in rendered_env


def test_render_service_env_uses_release_root_python_and_env_overrides(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    service_env_path = env_dir / "tg-bot-lan-rogue-trader-alert.env"
    local_config_path = env_dir / "tg-bot-lan-rogue-trader-alert.ini"
    deploy_root = tmp_path / "releases/prod/tg_bots/current"
    env_dir.mkdir(parents=True)
    _make_release_root(deploy_root)

    env = dict(os.environ)
    env.update(
        {
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "SERVICE_ENV_PATH": str(service_env_path),
            "LOCAL_CONFIG_PATH": str(local_config_path),
            "TG_BOTS_DEPLOY_ROOT": str(deploy_root),
            "SERVICE_ENV_OWNER": "",
        }
    )
    result = subprocess.run(  # noqa: S603 - controlled test invocation of the repo installer helper
        [
            "/usr/bin/bash",
            "-lc",
            "\n".join(
                [
                    f'source "{repo_root / "ops/scripts/deploy/install_tg_bots_systemd.sh"}"',
                    "initialize_stack_context",
                    "render_service_env",
                ],
            ),
        ],
        check=False,
        capture_output=True,
        text=True,
        env=env,
        cwd=repo_root,
    )

    assert result.returncode == 0
    rendered_env = service_env_path.read_text(encoding="utf-8")
    assert (
        f'CMD="{deploy_root}/.venv/bin/python -m nautilus_trader.flux.runners.tg_bots.run_lan_rogue_trader_alert '
        f'--config {local_config_path}"'
    ) in rendered_env
    assert f"WORKDIR={deploy_root}" in rendered_env
    assert f"PYTHONPATH={deploy_root}" in rendered_env
