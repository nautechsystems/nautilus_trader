from __future__ import annotations

import re
import tomllib
from pathlib import Path

ACTIVE_STRATEGY_CLASS = "maker_v3"
ACTIVE_PARAM_SET = "makerv3"
ROLLBACK_STRATEGY_ID = "aapl_tradexyz_makerv4"
ACTIVE_STRATEGIES = (
    {
        "symbol": "AAPL",
        "strategy_id": "aapl_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "AAPL.NASDAQ",
    },
    {
        "symbol": "AMD",
        "strategy_id": "amd_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:AMD-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "AMD.NASDAQ",
    },
    {
        "symbol": "AMZN",
        "strategy_id": "amzn_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:AMZN-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "AMZN.NASDAQ",
    },
    {
        "symbol": "BABA",
        "strategy_id": "baba_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:BABA-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "BABA.NYSE",
    },
    {
        "symbol": "COIN",
        "strategy_id": "coin_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:COIN-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "COIN.NASDAQ",
    },
    {
        "symbol": "CRCL",
        "strategy_id": "crcl_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:CRCL-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "CRCL.NYSE",
    },
    {
        "symbol": "CRWV",
        "strategy_id": "crwv_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:CRWV-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "CRWV.NASDAQ",
    },
    {
        "symbol": "GOOGL",
        "strategy_id": "googl_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:GOOGL-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "GOOGL.NASDAQ",
    },
    {
        "symbol": "HOOD",
        "strategy_id": "hood_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:HOOD-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "HOOD.NASDAQ",
    },
    {
        "symbol": "HYUNDAI",
        "strategy_id": "hyundai_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:HYUNDAI-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "005380.KRX",
    },
    {
        "symbol": "INTC",
        "strategy_id": "intc_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:INTC-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "INTC.NASDAQ",
    },
    {
        "symbol": "META",
        "strategy_id": "meta_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:META-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "META.NASDAQ",
    },
    {
        "symbol": "MSTR",
        "strategy_id": "mstr_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:MSTR-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "MSTR.NASDAQ",
    },
    {
        "symbol": "MSFT",
        "strategy_id": "msft_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:MSFT-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "MSFT.NASDAQ",
    },
    {
        "symbol": "MU",
        "strategy_id": "mu_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:MU-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "MU.NASDAQ",
    },
    {
        "symbol": "NFLX",
        "strategy_id": "nflx_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:NFLX-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "NFLX.NASDAQ",
    },
    {
        "symbol": "NVDA",
        "strategy_id": "nvda_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:NVDA-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "NVDA.NASDAQ",
    },
    {
        "symbol": "ORCL",
        "strategy_id": "orcl_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:ORCL-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "ORCL.NYSE",
    },
    {
        "symbol": "PLTR",
        "strategy_id": "pltr_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:PLTR-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "PLTR.NASDAQ",
    },
    {
        "symbol": "RIVN",
        "strategy_id": "rivn_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:RIVN-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "RIVN.NASDAQ",
    },
    {
        "symbol": "SNDK",
        "strategy_id": "sndk_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:SNDK-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "SNDK.NASDAQ",
    },
    {
        "symbol": "TSM",
        "strategy_id": "tsm_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:TSM-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "TSM.NYSE",
    },
    {
        "symbol": "TSLA",
        "strategy_id": "tsla_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:TSLA-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "TSLA.NASDAQ",
    },
    {
        "symbol": "USAR",
        "strategy_id": "usar_tradexyz_makerv3",
        "hyperliquid_instrument_id": "xyz:USAR-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "USAR.NASDAQ",
    },
)
ACTIVE_STRATEGY_IDS = [entry["strategy_id"] for entry in ACTIVE_STRATEGIES]
ACTIVE_HYPERLIQUID_INSTRUMENT_IDS = {
    entry["hyperliquid_instrument_id"]
    for entry in ACTIVE_STRATEGIES
}
ACTIVE_IBKR_INSTRUMENT_IDS = {
    entry["ibkr_instrument_id"]
    for entry in ACTIVE_STRATEGIES
}


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _load_toml(path: Path) -> dict:
    return tomllib.load(path.open("rb"))


def test_equities_live_config_uses_dedicated_portfolio_and_allowlists() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")

    assert config["portfolio"]["portfolio_id"] == "equities"
    assert config["api"]["strategy_class"] == ACTIVE_STRATEGY_CLASS
    assert config["api"]["strategy_groups"] == "equities"
    assert config["api"]["param_set"] == ACTIVE_PARAM_SET
    assert config["api"]["equities_strategy_ids"] == ACTIVE_STRATEGY_IDS
    assert config["api"]["equities_required_strategy_ids"] == ACTIVE_STRATEGY_IDS


def test_equities_strategy_template_uses_hyperliquid_xyz_and_equities_group() -> None:
    template_path = _repo_root() / "deploy/equities/strategies/equities.strategy.template.toml"
    template = _read(template_path)
    config = _load_toml(template_path)
    hyperliquid = config["node"]["venues"]["HYPERLIQUID"]
    ibkr = config["node"]["venues"]["IBKR"]
    strategy = config["strategy"]
    identity = config["identity"]

    assert 'execution_venue = "HYPERLIQUID"' in template
    assert 'reference_venue = "IBKR"' in template
    assert 'reference_symbol = "AAPL/USD"' in template
    assert 'dex = "xyz"' in template
    assert 'instrument_id = "xyz:AAPL-USD-PERP.HYPERLIQUID"' in template
    assert '[node.venues.IBKR]' in template
    assert 'instrument_id = "AAPL.NASDAQ"' in template
    assert '[node.venues.IBKR.dockerized_gateway]' in template
    assert 'trading_mode = "live"' in template
    assert 'read_only_api = true' in template
    assert 'auto_restart_time = "11:45 PM"' in template
    assert 'time_zone = "America/New_York"' in template
    assert "relogin_after_twofa_timeout = true" in template
    assert 'twofa_timeout_action = "restart"' in template
    assert identity["strategy_id"] == "symbol_tradexyz_makerv3"
    assert identity["strategy_instance_id"] == "symbol_tradexyz_makerv3"
    assert identity["external_strategy_id"] == "symbol_tradexyz_makerv3"
    assert hyperliquid["private_key_env"] == "TRADE_XYZ_AGENT_PK"
    assert hyperliquid["account_address_env"] == "TRADE_XYZ_ACCOUNT_ADDRESS"
    assert hyperliquid["vault_address_env"] == "TRADE_XYZ_VAULT_ADDRESS"
    assert ibkr["instrument_id"] == "AAPL.NASDAQ"
    assert ibkr["use_regular_trading_hours"] is False
    assert strategy["strategy_groups"] == "equities"
    assert strategy["order_qty"] == "1"
    assert strategy["qty"] == "1"
    assert strategy.get("param_set") in {None, ACTIVE_PARAM_SET}
    assert strategy["max_qty_global"] == 100.0
    assert strategy["max_skew_bps_global"] == 10.0
    assert strategy["bid_edge1"] == 5.0
    assert strategy["ask_edge1"] == 5.0
    assert strategy["place_edge1"] == 1.0
    assert strategy["distance1"] == 2.0
    assert strategy["n_orders1"] == 3


def test_equities_live_config_only_keeps_shared_contract_values() -> None:
    live_config_path = _repo_root() / "deploy/equities/equities.live.toml"
    live_config = _read(live_config_path)
    config = _load_toml(live_config_path)
    contracts = {(entry["exchange"], entry["instrument_id"]) for entry in config["contracts"]}

    assert set(config) == {
        "flux",
        "identity",
        "redis",
        "venues",
        "bridge",
        "api",
        "portfolio",
        "contracts",
    }
    assert "[node]" not in live_config
    assert "[strategy]" not in live_config
    assert 'exchange = "hyperliquid"' in live_config
    assert 'exchange = "ibkr"' in live_config
    assert contracts == {
        *{
            ("hyperliquid", instrument_id)
            for instrument_id in ACTIVE_HYPERLIQUID_INSTRUMENT_IDS
        },
        *{
            ("ibkr", instrument_id)
            for instrument_id in ACTIVE_IBKR_INSTRUMENT_IDS
        },
    }


def test_equities_active_strategy_contracts_are_makerv3_only() -> None:
    repo_root = _repo_root()
    disabled_rollback_path = repo_root / f"deploy/equities/strategies/{ROLLBACK_STRATEGY_ID}.toml.disabled"

    assert not (repo_root / f"deploy/equities/strategies/{ROLLBACK_STRATEGY_ID}.toml").exists()
    assert disabled_rollback_path.exists()
    for entry in ACTIVE_STRATEGIES:
        active_path = repo_root / f"deploy/equities/strategies/{entry['strategy_id']}.toml"
        assert active_path.exists()
        config = _load_toml(active_path)
        assert config["identity"]["strategy_id"] == entry["strategy_id"]
        assert config["identity"]["strategy_instance_id"] == entry["strategy_id"]
        assert config["identity"]["external_strategy_id"] == entry["strategy_id"]
        assert config["strategy"]["strategy_id"] == entry["strategy_id"]
        assert config["strategy"].get("param_set") in {None, ACTIVE_PARAM_SET}
        assert config["strategy"]["reference_use_quote_ticks"] is True
        assert config["node"]["enable_execution"] is False
        assert (
            config["node"]["venues"]["HYPERLIQUID"]["instrument_id"]
            == entry["hyperliquid_instrument_id"]
        )
        assert config["node"]["venues"]["IBKR"]["instrument_id"] == entry["ibkr_instrument_id"]
        assert config["node"]["venues"]["IBKR"]["use_regular_trading_hours"] is False
        assert (
            config["node"]["venues"]["HYPERLIQUID"]["vault_address_env"]
            == "TRADE_XYZ_VAULT_ADDRESS"
        )
        assert config["node"]["venues"]["IBKR"]["dockerized_gateway"]["twofa_timeout_action"] == "restart"


def test_equities_node_execution_contract_is_safe_in_toml_and_opt_in_in_stack() -> None:
    repo_root = _repo_root()
    active_path = repo_root / f"deploy/equities/strategies/{ACTIVE_STRATEGIES[0]['strategy_id']}.toml"
    template_path = repo_root / "deploy/equities/strategies/equities.strategy.template.toml"
    active_config = _load_toml(active_path)
    template_config = _load_toml(template_path)
    active_text = _read(active_path)
    template_text = _read(template_path)
    stack_script = _read(repo_root / "ops/scripts/deploy/equities_stack.sh")
    install_script = _read(repo_root / "ops/scripts/deploy/install_equities_systemd.sh")

    assert active_config["node"]["enable_execution"] is False
    assert template_config["node"]["enable_execution"] is False
    assert (
        "# Checked-in strategy configs stay safe-off; explicit runtime --enable-execution"
        in active_text
    )
    assert (
        "# Checked-in strategy configs stay safe-off; explicit runtime --enable-execution"
        in template_text
    )
    assert 'ENABLE_EXECUTION="${EQUITIES_ENABLE_EXECUTION:-0}"' in stack_script
    assert (
        'runtime override: EQUITIES_ENABLE_EXECUTION=1 passes --enable-execution even when'
        in stack_script
    )
    assert 'exec_flag+=(--enable-execution)' in stack_script
    assert (
        '${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_node --config'
        in install_script
    )
    assert "--enable-execution" in install_script


def test_equities_shared_contract_catalog_matches_active_strategy_routes() -> None:
    repo_root = _repo_root()
    shared_config = _load_toml(repo_root / "deploy/equities/equities.live.toml")
    shared_contracts = {
        (entry["exchange"], entry["instrument_id"])
        for entry in shared_config["contracts"]
    }
    for entry in ACTIVE_STRATEGIES:
        active_config = _load_toml(
            repo_root / f"deploy/equities/strategies/{entry['strategy_id']}.toml",
        )
        assert (
            "hyperliquid",
            active_config["node"]["venues"]["HYPERLIQUID"]["instrument_id"],
        ) in shared_contracts
        assert (
            "ibkr",
            active_config["node"]["venues"]["IBKR"]["instrument_id"],
        ) in shared_contracts


def test_equities_stack_env_example_defaults_to_safe_paper_without_execution() -> None:
    env_example = _read(_repo_root() / "deploy/equities/equities_stack.env.example")

    assert "EQUITIES_MODE=paper" in env_example
    assert "EQUITIES_CONFIRM_LIVE=0" in env_example
    assert "EQUITIES_ENABLE_EXECUTION=0" in env_example
    assert "EQUITIES_ALLOW_MISSING_KEYS=0" in env_example
    assert "deploy/equities/equities.live.toml" in env_example
    assert "TRADE_XYZ_AGENT_PK=" in env_example
    assert "TRADE_XYZ_ACCOUNT_ADDRESS=" in env_example
    assert "TRADE_XYZ_VAULT_ADDRESS=" in env_example
    assert "TWS_USERNAME=" in env_example
    assert "TWS_PASSWORD=" in env_example


def test_equities_stack_honors_enable_execution_flag_for_nodes() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/equities_stack.sh")

    assert 'exec_flag=()' in script
    assert 'if [[ "${ENABLE_EXECUTION}" == "1" ]]; then' in script
    assert 'exec_flag+=(--enable-execution)' in script


def test_equities_stack_script_is_scoped_to_equities_services_and_paths() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/equities_stack.sh")

    assert 'DEFAULT_ENV_PATH="${ROOT_DIR}/deploy/equities/equities_stack.env"' in script
    assert 'CONFIG_PATH="${EQUITIES_CONFIG_PATH:-${ROOT_DIR}/deploy/equities/equities.live.toml}"' in script
    assert 'STRATEGIES_DIR="${EQUITIES_STRATEGIES_DIR:-${ROOT_DIR}/deploy/equities/strategies}"' in script
    assert 'MODE="${EQUITIES_MODE:-paper}"' in script
    assert 'ENABLE_EXECUTION="${EQUITIES_ENABLE_EXECUTION:-0}"' in script
    assert 'nautilus_trader.flux.runners.equities.run_portfolio' in script
    assert 'nautilus_trader.flux.runners.equities.run_bridge' in script
    assert 'nautilus_trader.flux.runners.equities.run_api' in script
    assert 'nautilus_trader.flux.runners.equities.run_node' in script
    assert "--all-strategies" not in script
    assert '/equities' in script
    assert '/api/v1/params?profile=equities' in script
    assert '/api/v1/balances?profile=equities' in script
    assert '/api/v1/trades?profile=equities' in script
    assert 'logs <svc>' in script
    assert 'TOKENMM_' not in script


def test_equities_systemd_assets_use_equities_service_names() -> None:
    target = _read(_repo_root() / "deploy/equities/systemd/flux-equities.target")
    install_script = _read(_repo_root() / "ops/scripts/deploy/install_equities_systemd.sh")
    common_env = _read(_repo_root() / "deploy/equities/systemd/common.env.example")
    sudoers = _read(_repo_root() / "deploy/equities/systemd/flux-pulse.sudoers")

    assert "[Install]" in target
    assert "WantedBy=multi-user.target" in target
    assert 'Wants=flux@equities-api.service' in target
    assert 'Wants=flux@equities-portfolio.service' in target
    assert 'Wants=flux@equities-bridge.service' in target
    for strategy_id in ACTIVE_STRATEGY_IDS:
        assert f'Wants=flux@equities-node-{strategy_id}.service' in target
    assert 'deploy/equities/equities.live.toml' in install_script
    assert 'flux-equities.target' in install_script
    assert 'deploy/equities/systemd/common.env.example' in install_script
    assert '/etc/sudoers.d/flux-pulse' in install_script
    assert 'rebuild_flux_pulse_sudoers.sh' in install_script
    assert 'strategy_stack_write_env' in install_script
    assert 'equities-api.env' in install_script
    assert 'equities-portfolio.env' in install_script
    assert 'equities-bridge.env' in install_script
    assert '"equities"' in install_script
    assert '"Equities"' in install_script
    assert '"20"' in install_script
    assert '--host 127.0.0.1 --port 5024 --serve-fluxboard' in install_script
    assert '"0"' in install_script
    assert 'strategy_stack_discover_strategy_ids' in install_script
    assert "--all-strategies" not in install_script
    assert 'EQUITIES_REDIS_HOST=' in common_env
    assert 'EQUITIES_REDIS_PORT=6379' in common_env
    assert 'EQUITIES_REDIS_USERNAME=default' in common_env
    assert 'EQUITIES_REDIS_PASSWORD=' in common_env
    assert 'EQUITIES_REDIS_SSL=1' in common_env
    assert 'EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024' in common_env
    assert 'TRADE_XYZ_AGENT_PK=' in common_env
    assert 'TRADE_XYZ_ACCOUNT_ADDRESS=' in common_env
    assert 'TRADE_XYZ_VAULT_ADDRESS=' in common_env
    assert "/usr/bin/systemctl start flux@equities-api.service" not in sudoers
    assert "/usr/bin/systemctl restart flux@equities-portfolio.service" in sudoers
    for strategy_id in ACTIVE_STRATEGY_IDS:
        assert f"/usr/bin/systemctl restart flux@equities-node-{strategy_id}.service" in sudoers
    assert "flux@*" not in sudoers


def test_equities_installer_embeds_checkout_specific_runtime_paths() -> None:
    install_script = _read(_repo_root() / "ops/scripts/deploy/install_equities_systemd.sh")

    assert 'EQUITIES_PYTHON_BIN="${ROOT_DIR}/.venv/bin/python"' in install_script
    assert "require_project_python()" in install_script
    assert 'if [[ ! -x "${EQUITIES_PYTHON_BIN}" ]]; then' in install_script
    assert "uv sync --all-groups --all-extras" in install_script
    assert "--active --all-groups --all-extras" not in install_script
    assert re.search(
        r"main\(\)\s*\{\n(?:[ \t]*(?:#.*)?\n)*[ \t]*require_sudo\n(?:[ \t]*(?:#.*)?\n)*[ \t]*require_project_python\n",
        install_script,
    )
    assert "find \"${ENV_DIR}\" -maxdepth 1 -type f -name 'equities-node-*.env' -delete" in install_script
    assert "append_checkout_env_overrides()" in install_script
    assert "printf 'WORKDIR=%s\\nPYTHONPATH=%s\\n' \"${ROOT_DIR}\" \"${ROOT_DIR}\" >> \"${env_path}\"" in install_script
    assert '${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_api' in install_script
    assert '${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_portfolio' in install_script
    assert '${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_bridge' in install_script
    assert '${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_node' in install_script
    assert 'append_checkout_env_overrides "${ENV_DIR}/equities-api.env"' in install_script
    assert 'append_checkout_env_overrides "${ENV_DIR}/equities-portfolio.env"' in install_script
    assert 'append_checkout_env_overrides "${ENV_DIR}/equities-bridge.env"' in install_script
    assert 'append_checkout_env_overrides "${ENV_DIR}/${service_id}.env"' in install_script


def test_equities_shared_fluxboard_contract_uses_neutral_static_prefix() -> None:
    repo_root = _repo_root()
    install_script = _read(repo_root / "ops/scripts/deploy/install_equities_systemd.sh")
    lp_contract = _read(repo_root / "fluxboard/docs/lp_contract.md")

    assert "FLUXBOARD_BASE_PATH=/tokenmm/" not in install_script
    assert "/static/fluxboard/*" in lp_contract
    assert "`/equities`, `/lp`, and `/tokenmm` stay SPA entry routes, not asset owners." in lp_contract


def test_equities_contract_doc_keeps_equities_routes_spa_only() -> None:
    contract = _read(_repo_root() / "fluxboard/docs/equities_contract.md")

    assert "/equities/assets/*" not in contract
    assert "/static/fluxboard/assets/*" in contract
    assert "`/equities` stays a SPA route, not the asset prefix." in contract


def test_equities_deploy_docs_keep_equities_routes_spa_only() -> None:
    repo_root = _repo_root()
    readme = _read(repo_root / "deploy/equities/README.md")
    strategies_readme = _read(repo_root / "deploy/equities/strategies/README.md")

    assert "currently exposes `/equities/assets/*` route capability" not in readme
    assert "currently exposes `/equities/assets/*` route capability" not in strategies_readme
    assert (
        "The standalone equities runner keeps `/equities` as the SPA route while shared Fluxboard assets load from `/static/fluxboard/*`."
        in readme
    )
    assert (
        "The standalone equities runner keeps `/equities` as the SPA route while shared Fluxboard assets load from `/static/fluxboard/*`."
        in strategies_readme
    )


def test_equities_deploy_docs_require_post_install_env_verification() -> None:
    readme = _read(_repo_root() / "deploy/equities/README.md")
    common_env = _read(_repo_root() / "deploy/equities/systemd/common.env.example")

    assert "`sed -n '1,120p' /etc/flux/equities-api.env`" in readme
    assert "`sed -n '1,120p' /etc/flux/equities-portfolio.env`" in readme
    assert "`sed -n '1,120p' /etc/flux/equities-bridge.env`" in readme
    assert (
        f"`sed -n '1,120p' /etc/flux/equities-node-{ACTIVE_STRATEGY_IDS[0]}.env`" in readme
    )
    assert "`find /etc/flux -maxdepth 1 -type f -name 'equities-node-*.env' -print | sort`" in readme
    assert "`for env_path in /etc/flux/equities-node-*.env; do sed -n '1,120p' \"$env_path\"; done`" in readme
    assert "`uv sync --all-groups --all-extras`" in readme
    assert "the generated envs append `WORKDIR=` / `PYTHONPATH=` for the selected checkout" in readme
    assert "the generated env commands use the checkout-local `.venv/bin/python`" in readme
    assert "every generated `equities-node-*.env` is rewritten from the intended checkout" in readme
    assert "print and review every rendered `equities-node-*.env` contents" in readme
    assert "Do not restart services until those env files match the intended checkout and live flags." in readme
    assert "verify every matching `/etc/flux/equities-node-*.env`" in common_env
    assert "`uv sync --all-groups --all-extras` in the selected checkout" in common_env


def test_equities_and_tokenmm_installers_use_shared_strategy_stack_conventions() -> None:
    repo_root = _repo_root()
    shared = _read(repo_root / "ops/scripts/deploy/shared_strategy_stack.sh")
    tokenmm_install = _read(repo_root / "ops/scripts/deploy/install_tokenmm_systemd.sh")
    equities_install = _read(repo_root / "ops/scripts/deploy/install_equities_systemd.sh")
    tokenmm_stack = _read(repo_root / "ops/scripts/deploy/tokenmm_stack.sh")
    equities_stack = _read(repo_root / "ops/scripts/deploy/equities_stack.sh")

    assert (
        'source "${DEPLOY_ROOT}/ops/scripts/deploy/shared_strategy_stack.sh"'
        in tokenmm_install
    )
    assert 'source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"' in equities_install
    assert 'source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"' in tokenmm_stack
    assert 'source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"' in equities_stack
    assert '/../../.." && pwd)' in tokenmm_install
    assert '/../../.." && pwd)' in equities_install
    assert '/../../.." && pwd)' in tokenmm_stack
    assert '/../../.." && pwd)' in equities_stack

    assert "strategy_stack_render_target()" in shared
    assert "strategy_stack_render_merged_sudoers()" in shared
    assert "strategy_stack_write_env()" in shared
    assert "strategy_stack_render_sudoers()" in shared
    assert "strategy_stack_load_strategy_configs()" in shared
    assert "strategy_stack_print_install_hint()" in shared

    assert "strategy_stack_write_env" in tokenmm_install
    assert "strategy_stack_write_env" in equities_install
    assert '"tokenmm"' in tokenmm_install
    assert '"equities"' in equities_install
    assert 'ENV_PATH="${TOKENMM_ENV_PATH:-${DEFAULT_ENV_PATH}}"' in tokenmm_stack
    assert 'ENV_PATH="${EQUITIES_ENV_PATH:-${DEFAULT_ENV_PATH}}"' in equities_stack


def test_equities_stack_script_parses_env_safely_and_loads_trade_xyz_secrets() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/equities_stack.sh")

    assert 'source "${ENV_PATH}"' not in script
    assert "aws secretsmanager get-secret-value" in script
    assert "jq -r 'to_entries[] |" in script
    assert "TRADE_XYZ_AGENT_PK" in script
    assert "TRADE_XYZ_ACCOUNT_ADDRESS" in script
    assert "TRADE_XYZ_VAULT_ADDRESS" in script
    assert "warning: skipping unsupported secret key" in script


def test_equities_stack_script_validates_ibkr_docker_gateway_credentials_when_configured() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/equities_stack.sh")

    assert "TWS_USERNAME" in script
    assert "TWS_PASSWORD" in script
    assert "missing required IBKR dockerized gateway credentials" in script
    assert "dockerized_gateway" in script


def test_equities_docs_reference_profile_and_portfolio_contracts() -> None:
    repo_root = _repo_root()
    readme = _read(repo_root / "deploy/equities/README.md")
    strategies_readme = _read(repo_root / "deploy/equities/strategies/README.md")
    common_env = _read(repo_root / "deploy/equities/systemd/common.env.example")
    live_config = _read(repo_root / "deploy/equities/equities.live.toml")
    contract = _read(repo_root / "fluxboard/docs/equities_contract.md")

    assert "EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024" in readme
    assert "TRADE_XYZ_AGENT_PK" in readme
    assert "TRADE_XYZ_ACCOUNT_ADDRESS" in readme
    assert "TRADE_XYZ_VAULT_ADDRESS" in readme
    assert "TWS_USERNAME" in readme
    assert "TWS_PASSWORD" in readme
    assert "shared config merge only imports `redis` and `portfolio`" in readme
    assert "active node settings live in `deploy/equities/strategies/*.toml`" in readme
    assert "AAPL.NASDAQ" in readme
    assert "`/equities` API contract catalog is built from the shared `[[contracts]]` entries" in readme
    assert "shared IBKR contract entry must mirror the active canary route" in readme
    assert "vault_address_env" in readme
    assert 'use_regular_trading_hours = false' in readme
    assert 'twofa_timeout_action = "restart"' in readme

    assert "<stock>_tradexyz_makerv3.toml" in strategies_readme
    assert "aapl_tradexyz_makerv4.toml.disabled" in strategies_readme
    assert "AAPL.NASDAQ" in strategies_readme
    assert "use_regular_trading_hours = false" in strategies_readme
    assert 'twofa_timeout_action = "restart"' in strategies_readme
    assert "TRADE_XYZ_VAULT_ADDRESS" in strategies_readme
    assert "Keep the shared `[[contracts]]` IBKR entry aligned with the active canary reference instrument" in strategies_readme
    assert "TWS_USERNAME" in strategies_readme
    assert "TWS_PASSWORD" in strategies_readme

    assert "EQUITIES_REDIS_HOST=" in common_env
    assert "EQUITIES_REDIS_PASSWORD=" in common_env
    assert "EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024" in common_env
    assert "TRADE_XYZ_AGENT_PK=" in common_env
    assert "TRADE_XYZ_ACCOUNT_ADDRESS=" in common_env
    assert "TRADE_XYZ_VAULT_ADDRESS=" in common_env

    assert 'portfolio_id = "equities"' in live_config
    assert "equities_strategy_ids" in live_config
    assert f'strategy_class = "{ACTIVE_STRATEGY_CLASS}"' in live_config
    for strategy_id in ACTIVE_STRATEGY_IDS:
        assert strategy_id in live_config

    assert "/equities" in contract
    assert "/api/v1/signals?profile=equities" in contract
    assert "/api/v1/params?profile=equities" in contract
    assert "trade[XYZ]" in contract
    assert "AAPL.NASDAQ" in contract
    assert "MakerV3" in contract
