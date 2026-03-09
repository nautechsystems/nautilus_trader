from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from typing import Any


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(relative_path: str) -> str:
    return (_repo_root() / relative_path).read_text(encoding="utf-8")


def _run_preflight(
    *,
    common_env: Path,
    system_ini: Path,
    band1_config: Path,
    band2_config: Path,
) -> dict[str, Any]:
    script = _repo_root() / "ops/scripts/lp_hedger_preflight.py"
    result = subprocess.run(  # noqa: S603
        [
            sys.executable,
            str(script),
            "--json",
            "--common-env",
            str(common_env),
            "--system-ini",
            str(system_ini),
            "--band1-config",
            str(band1_config),
            "--band2-config",
            str(band2_config),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode not in {0, 1}:
        raise RuntimeError(result.stderr or result.stdout)
    return json.loads(result.stdout)


def test_lp_prod_runbook_documents_shared_host_topology() -> None:
    text = _read("docs/runbooks/lp-hedger-production-rollout.md")

    assert "/lp" in text
    assert "/api/v1/hedgers/*" in text
    assert "LP_API_BACKEND_URL=http://127.0.0.1:5025" in text
    assert "flux-lp.target" in text
    assert "service-eth-plume-lp-hedger" in text
    assert "service-eth-plume-lp-hedger-band2" in text
    assert "rollback" in text.lower()


def test_lp_prod_docs_keep_band1_band2_as_only_active_instances() -> None:
    text = _read("deploy/lp/README.md")

    assert "Band1 and Band2" in text
    assert ".ini.disabled" in text


def test_lp_preflight_requires_loopback_backend_url_and_system_ini_sections(tmp_path: Path) -> None:
    common_env = tmp_path / "common.env"
    common_env.write_text("LP_API_BACKEND_URL=http://127.0.0.1:5025\n", encoding="utf-8")
    system_ini = tmp_path / "lp-system.ini"
    system_ini.write_text("[redis]\nurl=redis://example\n[plume]\nrpc_url=http://rpc\n", encoding="utf-8")
    band1_config = tmp_path / "band1.ini"
    band1_config.write_text("[identity]\nid=band1\n", encoding="utf-8")
    band2_config = tmp_path / "band2.ini"
    band2_config.write_text("[identity]\nid=band2\n", encoding="utf-8")

    result = _run_preflight(
        common_env=common_env,
        system_ini=system_ini,
        band1_config=band1_config,
        band2_config=band2_config,
    )

    assert result["ok"] is False
    errors = result["errors"]
    assert any("bybit_hedger" in error for error in errors)
    assert any("bybit_hedger_band2" in error for error in errors)


def test_lp_preflight_accepts_band1_band2_config_contract(tmp_path: Path) -> None:
    common_env = tmp_path / "common.env"
    common_env.write_text("LP_API_BACKEND_URL=http://127.0.0.1:5025\n", encoding="utf-8")
    system_ini = tmp_path / "lp-system.ini"
    system_ini.write_text(
        "\n".join(
            [
                "[redis]",
                "url=redis://example",
                "[plume]",
                "rpc_url=http://rpc",
                "[bybit]",
                "api_domain=example",
                "[bybit_hedger]",
                "enabled=true",
                "[bybit_hedger_band2]",
                "enabled=true",
            ],
        )
        + "\n",
        encoding="utf-8",
    )
    band1_config = tmp_path / "band1.ini"
    band1_config.write_text("[identity]\nid=band1\n", encoding="utf-8")
    band2_config = tmp_path / "band2.ini"
    band2_config.write_text("[identity]\nid=band2\n", encoding="utf-8")

    result = _run_preflight(
        common_env=common_env,
        system_ini=system_ini,
        band1_config=band1_config,
        band2_config=band2_config,
    )

    assert result["ok"] is True
    assert result["errors"] == []
