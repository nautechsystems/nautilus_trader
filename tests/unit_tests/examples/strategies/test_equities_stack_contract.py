from __future__ import annotations

import tomllib
from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _load_toml(path: Path) -> dict:
    return tomllib.load(path.open("rb"))


def test_equities_live_config_uses_dedicated_portfolio_and_allowlists() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")

    assert config["portfolio"]["portfolio_id"] == "equities"
    assert config["api"]["strategy_class"] == "maker_v4"
    assert config["api"]["strategy_groups"] == "equities"
    assert config["api"]["equities_strategy_ids"] == ["aapl_tradexyz_makerv4"]
    assert config["api"]["equities_required_strategy_ids"] == ["aapl_tradexyz_makerv4"]


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
    assert identity["strategy_id"] == "aapl_tradexyz_makerv4"
    assert identity["strategy_instance_id"] == "aapl_tradexyz_makerv4"
    assert identity["external_strategy_id"] == "aapl_tradexyz_makerv4"
    assert hyperliquid["private_key_env"] == "TRADE_XYZ_AGENT_PK"
    assert hyperliquid["account_address_env"] == "TRADE_XYZ_ACCOUNT_ADDRESS"
    assert hyperliquid["vault_address_env"] == "TRADE_XYZ_VAULT_ADDRESS"
    assert ibkr["instrument_id"] == "AAPL.NASDAQ"
    assert ibkr["use_regular_trading_hours"] is False
    assert strategy["strategy_groups"] == "equities"
    assert strategy["param_set"] == "makerv4"
    assert strategy["order_qty"] == "1"
    assert strategy["qty"] == "1"
    assert strategy["outside_rth_hedge_enabled"] is True
    assert strategy["ibkr_primary_exchange"] == "NASDAQ"
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
    assert 'instrument_id = "xyz:AAPL-USD-PERP.HYPERLIQUID"' in live_config
    assert 'exchange = "ibkr"' in live_config
    assert 'instrument_id = "AAPL.NASDAQ"' in live_config
    assert contracts == {
        ("hyperliquid", "xyz:AAPL-USD-PERP.HYPERLIQUID"),
        ("ibkr", "AAPL.NASDAQ"),
    }


def test_equities_active_strategy_contract_is_makerv4_only() -> None:
    repo_root = _repo_root()
    active_path = repo_root / "deploy/equities/strategies/aapl_tradexyz_makerv4.toml"
    disabled_rollback_path = repo_root / "deploy/equities/strategies/aapl_tradexyz_makerv3.toml.disabled"

    assert active_path.exists()
    assert not (repo_root / "deploy/equities/strategies/aapl_tradexyz_makerv3.toml").exists()
    assert disabled_rollback_path.exists()

    config = _load_toml(active_path)
    assert config["identity"]["strategy_id"] == "aapl_tradexyz_makerv4"
    assert config["identity"]["strategy_instance_id"] == "aapl_tradexyz_makerv4"
    assert config["identity"]["external_strategy_id"] == "aapl_tradexyz_makerv4"
    assert config["strategy"]["strategy_id"] == "aapl_tradexyz_makerv4"
    assert config["strategy"]["param_set"] == "makerv4"
    assert config["strategy"]["outside_rth_hedge_enabled"] is True
    assert config["strategy"]["ibkr_primary_exchange"] == "NASDAQ"
    assert config["node"]["venues"]["IBKR"]["instrument_id"] == "AAPL.NASDAQ"
    assert config["node"]["venues"]["IBKR"]["use_regular_trading_hours"] is False


def test_equities_shared_contract_catalog_matches_active_canary_route() -> None:
    repo_root = _repo_root()
    shared_config = _load_toml(repo_root / "deploy/equities/equities.live.toml")
    active_config = _load_toml(repo_root / "deploy/equities/strategies/aapl_tradexyz_makerv4.toml")

    shared_ibkr_contract = next(
        entry
        for entry in shared_config["contracts"]
        if entry["exchange"] == "ibkr"
    )
    active_ibkr_instrument_id = active_config["node"]["venues"]["IBKR"]["instrument_id"]
    active_ibkr_primary_exchange = active_config["strategy"]["ibkr_primary_exchange"]

    assert shared_ibkr_contract["instrument_id"] == active_ibkr_instrument_id
    assert active_ibkr_instrument_id.endswith(f".{active_ibkr_primary_exchange}")


def test_equities_stack_env_example_defaults_to_safe_paper_without_execution() -> None:
    env_example = _read(_repo_root() / "deploy/equities/equities_stack.env.example")

    assert "EQUITIES_MODE=paper" in env_example
    assert "EQUITIES_CONFIRM_LIVE=0" in env_example
    assert "EQUITIES_ENABLE_EXECUTION=0" in env_example
    assert "EQUITIES_ALLOW_MISSING_KEYS=0" in env_example
    assert "deploy/equities/equities.live.toml" in env_example
    assert "TRADE_XYZ_AGENT_PK=" in env_example
    assert "TRADE_XYZ_ACCOUNT_ADDRESS=" in env_example


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
    assert 'Wants=flux@equities-node-aapl_tradexyz_makerv4.service' in target
    assert 'deploy/equities/equities.live.toml' in install_script
    assert 'flux-equities.target' in install_script
    assert 'deploy/equities/systemd/common.env.example' in install_script
    assert '/etc/sudoers.d/flux-pulse' in install_script
    assert 'strategy_stack_render_sudoers' in install_script
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
    assert "/usr/bin/systemctl restart flux@equities-node-aapl_tradexyz_makerv4.service" in sudoers
    assert "flux@*" not in sudoers


def test_equities_and_tokenmm_installers_use_shared_strategy_stack_conventions() -> None:
    repo_root = _repo_root()
    shared = _read(repo_root / "ops/scripts/deploy/shared_strategy_stack.sh")
    tokenmm_install = _read(repo_root / "ops/scripts/deploy/install_tokenmm_systemd.sh")
    equities_install = _read(repo_root / "ops/scripts/deploy/install_equities_systemd.sh")
    tokenmm_stack = _read(repo_root / "ops/scripts/deploy/tokenmm_stack.sh")
    equities_stack = _read(repo_root / "ops/scripts/deploy/equities_stack.sh")

    assert 'source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"' in tokenmm_install
    assert 'source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"' in equities_install
    assert 'source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"' in tokenmm_stack
    assert 'source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"' in equities_stack
    assert '/../../.." && pwd)' in tokenmm_install
    assert '/../../.." && pwd)' in equities_install
    assert '/../../.." && pwd)' in tokenmm_stack
    assert '/../../.." && pwd)' in equities_stack

    assert "strategy_stack_render_target()" in shared
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
    assert "warning: skipping unsupported secret key" in script


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
    assert "shared config merge only imports `redis` and `portfolio`" in readme
    assert "active node settings live in `deploy/equities/strategies/*.toml`" in readme
    assert "AAPL.NASDAQ" in readme
    assert "`/equities` API contract catalog is built from the shared `[[contracts]]` entries" in readme
    assert "shared IBKR contract entry must mirror the active canary route" in readme
    assert "outside-RTH fills are actually available" in readme
    assert "assumed_hedge_fee_bps" in readme

    assert "<stock>_tradexyz_makerv4.toml" in strategies_readme
    assert "aapl_tradexyz_makerv3.toml.disabled" in strategies_readme
    assert "AAPL.NASDAQ" in strategies_readme
    assert "use_regular_trading_hours = false" in strategies_readme
    assert "outside_rth_hedge_enabled = true" in strategies_readme
    assert "ibkr_primary_exchange" in strategies_readme
    assert "assumed_hedge_fee_bps" in strategies_readme
    assert "Keep the shared `[[contracts]]` IBKR entry aligned with the active canary reference instrument" in strategies_readme

    assert "EQUITIES_REDIS_HOST=" in common_env
    assert "EQUITIES_REDIS_PASSWORD=" in common_env
    assert "EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024" in common_env
    assert "TRADE_XYZ_AGENT_PK=" in common_env
    assert "TRADE_XYZ_ACCOUNT_ADDRESS=" in common_env
    assert "TRADE_XYZ_VAULT_ADDRESS=" in common_env

    assert 'portfolio_id = "equities"' in live_config
    assert "equities_strategy_ids" in live_config
    assert 'strategy_class = "maker_v4"' in live_config
    assert "aapl_tradexyz_makerv4" in live_config

    assert "/equities" in contract
    assert "/api/v1/signals?profile=equities" in contract
    assert "/api/v1/params?profile=equities" in contract
    assert "trade[XYZ]" in contract
    assert "AAPL.NASDAQ" in contract
