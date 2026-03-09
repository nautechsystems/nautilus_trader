from __future__ import annotations

from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def test_install_tg_bots_systemd_seeds_readable_local_config() -> None:
    script = (_repo_root() / "ops/scripts/deploy/install_tg_bots_systemd.sh").read_text(
        encoding="utf-8"
    )

    assert 'install -m 0644 "${ROOT_DIR}/deploy/tg_bots/lan_rogue_trader_alert.ini" "${LOCAL_CONFIG_PATH}"' in script
    assert 'chown root:ubuntu "${SERVICE_ENV_PATH}"' in script
    assert 'chmod 0640 "${SERVICE_ENV_PATH}"' in script
