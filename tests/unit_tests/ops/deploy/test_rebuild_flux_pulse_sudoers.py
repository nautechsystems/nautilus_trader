from __future__ import annotations

import re
import subprocess
from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _write_env(path: Path, lines: list[str]) -> None:
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def test_rebuild_flux_pulse_sudoers_discovers_all_pulse_enabled_jobs(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    sudoers_path = tmp_path / "sudoers.d" / "flux-pulse"

    _write_env(env_dir / "common.env", ["WORKDIR=/tmp/repo"])
    _write_env(
        env_dir / "tokenmm-api.env",
        [
            "PULSE_ENABLED=1",
            "PULSE_DESCRIPTION=TokenMM API",
        ],
    )
    _write_env(
        env_dir / "equities-api.env",
        [
            "PULSE_ENABLED=0",
            "PULSE_DESCRIPTION=Equities API",
        ],
    )
    _write_env(
        env_dir / "lp-api.env",
        [
            "PULSE_ENABLED=1",
            "PULSE_DESCRIPTION=LP API",
        ],
    )
    _write_env(
        env_dir / "tg-bot-lan-rogue-trader-alert.env",
        [
            "PULSE_ENABLED=1",
            "PULSE_DESCRIPTION=Lan Rogue Trader Alert",
        ],
    )

    script_path = _repo_root() / "ops/scripts/deploy/rebuild_flux_pulse_sudoers.sh"
    subprocess.run(  # noqa: S603 - test executes a repo-controlled helper script path
        ["/usr/bin/bash", str(script_path)],
        check=True,
        env={
            "ENV_DIR": str(env_dir),
            "SUDOERS_PATH": str(sudoers_path),
            "RUN_AS_USER": "ubuntu",
        },
    )

    sudoers_text = sudoers_path.read_text(encoding="utf-8")
    discovered_ids = re.findall(r"systemctl start flux@([^.]+)\.service", sudoers_text)

    assert discovered_ids == [
        "lp-api",
        "tg-bot-lan-rogue-trader-alert",
        "tokenmm-api",
    ]
    assert "equities-api" not in sudoers_text
    assert "/usr/bin/systemctl restart flux@lp-api.service" in sudoers_text
    assert "/usr/bin/systemctl restart flux@tg-bot-lan-rogue-trader-alert.service" in sudoers_text
    assert "/usr/bin/journalctl -u flux@tokenmm-api.service" in sudoers_text
