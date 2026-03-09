from __future__ import annotations

import subprocess
from pathlib import Path

from lp.hedgers import list_hedgers


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _active_lp_hedger_ids() -> list[str]:
    hedgers_dir = _repo_root() / "deploy/lp/hedgers"
    if not hedgers_dir.is_dir():
        return []
    return sorted(
        path.stem
        for path in hedgers_dir.glob("*.ini")
        if path.name != "lp_hedger.template.ini"
    )


def test_lp_registry_default_paths_use_deploy_root() -> None:
    paths = {meta.id: meta.config_default_path for meta in list_hedgers()}

    assert paths == {
        "eth_plume_lp": "deploy/lp/hedgers/eth_plume_lp_hedger.ini",
        "eth_plume_lp_band2": "deploy/lp/hedgers/eth_plume_lp_hedger_band2.ini",
        "hype_usdt_lp": "deploy/lp/hedgers/hype_usdt_lp_hedger.ini",
        "plume_weth_lp": "deploy/lp/hedgers/plume_weth_lp_hedger.ini",
        "third_lp": "deploy/lp/hedgers/third_lp_hedger.ini.disabled",
    }


def test_lp_common_env_mentions_hidden_backend_proxy() -> None:
    content = _read(_repo_root() / "deploy/lp/systemd/common.env.example")

    assert "LP_API_BACKEND_URL=http://127.0.0.1:5025" in content
    assert "LP_SYSTEM_CONFIG=/etc/flux/lp-system.ini" in content


def test_staged_extra_hedgers_ship_as_checked_in_configs() -> None:
    assert _active_lp_hedger_ids() == [
        "eth_plume_lp_hedger",
        "eth_plume_lp_hedger_band2",
        "hype_usdt_lp_hedger",
        "plume_weth_lp_hedger",
    ]


def test_lp_contract_doc_mentions_same_redis_key_family() -> None:
    doc = _read(_repo_root() / "fluxboard/docs/lp_contract.md")

    assert ":state" in doc
    assert ":snapshot" in doc
    assert ":events" in doc
    assert ":geometry_overrides" in doc
    assert ":threshold_overrides" in doc
    assert "hype_usdt_lp" in doc
    assert "plume_weth_lp" in doc
    assert "third_lp" in doc
    assert "config_ready" in doc
    assert "config_readiness_errors" in doc


def test_lp_stack_script_starts_hidden_backend_and_public_proxy() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/lp_stack.sh")

    assert 'DEFAULT_ENV_PATH="${ROOT_DIR}/deploy/lp/lp_stack.env"' in script
    assert 'LP_API_HOST="${LP_API_HOST:-127.0.0.1}"' in script
    assert 'LP_API_PORT="${LP_API_PORT:-5025}"' in script
    assert 'PUBLIC_API_HOST="${LP_PUBLIC_HOST:-127.0.0.1}"' in script
    assert 'PUBLIC_API_PORT="${LP_PUBLIC_PORT:-5022}"' in script
    assert "python3 -m lp.runners.run_api" in script
    assert "--serve-fluxboard" in script
    assert "nautilus_trader.flux.runners.tokenmm.run_api" in script
    assert 'LP_API_BACKEND_URL="http://${LP_API_HOST}:${LP_API_PORT}"' in script
    assert "--serve-fluxboard --serve-pulse" in script


def test_lp_stack_env_file_does_not_override_explicit_port_env_vars(tmp_path: Path) -> None:
    script_path = _repo_root() / "ops/scripts/deploy/lp_stack.sh"
    sourceable_script = tmp_path / "lp_stack.sourceable.sh"
    sourceable_script.write_text(
        _read(script_path).replace('\nmain "$@"\n', "\n:\n"),
        encoding="utf-8",
    )
    env_path = tmp_path / "lp_stack.env"
    env_path.write_text(
        "LP_API_PORT=5025\nLP_PUBLIC_PORT=5022\n",
        encoding="utf-8",
    )

    result = subprocess.run(  # noqa: S603
        [
            "/bin/bash",
            "-lc",
            (
                f"export LP_ENV_PATH='{env_path}'; "
                "export LP_API_PORT=15025; "
                "export LP_PUBLIC_PORT=15022; "
                f"source '{sourceable_script}'; "
                "load_env_file; "
                "printf '%s %s\\n' \"$LP_API_PORT\" \"$LP_PUBLIC_PORT\""
            ),
        ],
        check=True,
        capture_output=True,
        text=True,
    )

    assert result.stdout.strip() == "15025 15022"


def test_lp_systemd_assets_use_chainsaw_job_ids() -> None:
    target = _read(_repo_root() / "deploy/lp/systemd/flux-lp.target")
    sudoers = _read(_repo_root() / "deploy/lp/systemd/flux-pulse.sudoers")
    install_script = _read(_repo_root() / "ops/scripts/deploy/install_lp_systemd.sh")

    assert "Wants=flux@lp-api.service" in target
    assert "Wants=flux@service-eth-plume-lp-hedger.service" in target
    assert "Wants=flux@service-eth-plume-lp-hedger-band2.service" in target
    assert "service-hedger3" not in target
    assert "service-hedger4" not in target
    assert "/usr/bin/systemctl restart flux@service-eth-plume-lp-hedger.service" in sudoers
    assert "/usr/bin/systemctl restart flux@service-eth-plume-lp-hedger-band2.service" in sudoers
    assert "/usr/bin/systemctl restart flux@lp-api.service" in sudoers
    assert "/usr/bin/systemctl restart flux@service-hedger3.service" in sudoers
    assert "/usr/bin/systemctl restart flux@service-hedger4.service" in sudoers
    assert "deploy/lp/systemd/common.env.example" in install_script
    assert "flux-lp.target" in install_script
    assert "strategy_stack_render_merged_sudoers" in install_script
    assert "lp-api.env" in install_script
    assert "service-eth-plume-lp-hedger.env" in install_script
    assert "service-eth-plume-lp-hedger-band2.env" in install_script
    assert "service-hedger3.env" in install_script
    assert "service-hedger4.env" in install_script


def test_lp_readme_documents_hidden_backend_and_staged_configs() -> None:
    readme = _read(_repo_root() / "deploy/lp/README.md")

    assert "LP_API_BACKEND_URL=http://127.0.0.1:5025" in readme
    assert "/lp" in readme
    assert "/api/v1/hedgers/*" in readme
    assert "hype_usdt_lp_hedger.ini" in readme
    assert "plume_weth_lp_hedger.ini" in readme
    assert "third_lp_hedger.ini.disabled" in readme
    assert "staged" in readme.lower()
    assert "Band1 and Band2" in readme


def test_install_lp_systemd_enrolls_staged_services_without_auto_start_and_keeps_shared_host_restart_order() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/install_lp_systemd.sh")

    assert "lp-api.env" in script
    assert "service-eth-plume-lp-hedger.env" in script
    assert "service-eth-plume-lp-hedger-band2.env" in script
    assert "service-hedger3.env" in script
    assert "service-hedger4.env" in script
    assert "flux@tokenmm-api.service" in script
    assert "LP_API_BACKEND_URL" in script


def test_install_lp_systemd_writes_literal_lp_system_config_path_for_hedger_units() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/install_lp_systemd.sh")

    assert "--system-config /etc/flux/lp-system.ini" in script
    assert "${LP_SYSTEM_CONFIG:-/etc/flux/lp-system.ini}" not in script


def test_shared_sudoers_renderer_merges_existing_flux_pulse_service_ids(tmp_path: Path) -> None:
    shared_script = _repo_root() / "ops/scripts/deploy/shared_strategy_stack.sh"
    existing = tmp_path / "flux-pulse.existing"
    existing.write_text(
        _read(_repo_root() / "deploy/tokenmm/systemd/flux-pulse.sudoers"),
        encoding="utf-8",
    )
    rendered = tmp_path / "flux-pulse.rendered"

    result = subprocess.run(  # noqa: S603
        [
            "/bin/bash",
            "-lc",
            (
                f"source '{shared_script}'; "
                f"strategy_stack_render_merged_sudoers ubuntu '{rendered}' '{existing}' "
                "lp-api service-eth-plume-lp-hedger service-eth-plume-lp-hedger-band2; "
                f"cat '{rendered}'"
            ),
        ],
        check=True,
        capture_output=True,
        text=True,
    )

    sudoers = result.stdout
    assert "/usr/bin/systemctl restart flux@tokenmm-api.service" in sudoers
    assert "/usr/bin/systemctl restart flux@lp-api.service" in sudoers
    assert "/usr/bin/systemctl restart flux@service-eth-plume-lp-hedger.service" in sudoers
    assert "/usr/bin/systemctl restart flux@service-eth-plume-lp-hedger-band2.service" in sudoers
    assert sudoers.count("flux@tokenmm-api.service") == 4
    assert sudoers.count("flux@lp-api.service") == 4
