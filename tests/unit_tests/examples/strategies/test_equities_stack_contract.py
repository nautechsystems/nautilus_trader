from __future__ import annotations

import tomllib
from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_equities_live_config_uses_dedicated_portfolio_and_allowlists() -> None:
    config = tomllib.load((_repo_root() / "deploy/equities/equities.live.toml").open("rb"))

    assert config["portfolio"]["portfolio_id"] == "equities"
    assert config["api"]["strategy_groups"] == "equities"
    assert config["api"]["equities_strategy_ids"] == ["aapl_tradexyz_makerv3"]
    assert config["api"]["equities_required_strategy_ids"] == ["aapl_tradexyz_makerv3"]


def test_equities_strategy_template_uses_hyperliquid_xyz_and_equities_group() -> None:
    template = _read(_repo_root() / "deploy/equities/strategies/equities.strategy.template.toml")

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
    assert 'strategy_groups = "equities"' in template
    assert 'private_key_env = "TRADE_XYZ_AGENT_PK"' in template
    assert 'account_address_env = "TRADE_XYZ_ACCOUNT_ADDRESS"' in template


def test_equities_stack_env_example_defaults_to_safe_paper_without_execution() -> None:
    env_example = _read(_repo_root() / "deploy/equities/equities_stack.env.example")

    assert "EQUITIES_MODE=paper" in env_example
    assert "EQUITIES_CONFIRM_LIVE=0" in env_example
    assert "EQUITIES_ENABLE_EXECUTION=0" in env_example
    assert "EQUITIES_ALLOW_MISSING_KEYS=0" in env_example
    assert "deploy/equities/equities.live.toml" in env_example
    assert "TRADE_XYZ_AGENT_PK=" in env_example
    assert "TRADE_XYZ_ACCOUNT_ADDRESS=" in env_example


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
    assert 'Wants=flux@equities-node-aapl_tradexyz_makerv3.service' in target
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
    assert 'TRADE_XYZ_AGENT_PK=' in common_env
    assert 'TRADE_XYZ_ACCOUNT_ADDRESS=' in common_env
    assert "/usr/bin/systemctl start flux@equities-api.service" not in sudoers
    assert "/usr/bin/systemctl restart flux@equities-portfolio.service" in sudoers
    assert "/usr/bin/systemctl restart flux@equities-node-aapl_tradexyz_makerv3.service" in sudoers
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

    assert "/equities" in readme
    assert "POST /api/pulse/jobs/group/equities/restart" in readme
    assert "tokenmm-api" in readme
    assert "internal-only `equities-api` backend on loopback" in readme
    assert "does not provision a second public API on `:5022`" in readme
    assert "EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024" in readme
    assert "/api/v1/params?profile=equities" in readme
    assert "TRADE_XYZ_AGENT_PK" in readme
    assert "TRADE_XYZ_ACCOUNT_ADDRESS" in readme
    assert "preserve the outer equities surface" in readme

    assert "one stock uses one strategy file and one node process" in strategies_readme.lower()
    assert "Start from `equities.strategy.template.toml`." in strategies_readme

    assert "EQUITIES_REDIS_HOST=" in common_env
    assert "EQUITIES_REDIS_PASSWORD=" in common_env
    assert "TRADE_XYZ_AGENT_PK=" in common_env
    assert "TRADE_XYZ_ACCOUNT_ADDRESS=" in common_env

    assert 'portfolio_id = "equities"' in live_config
    assert "equities_strategy_ids" in live_config

    assert "/equities" in contract
    assert "/api/v1/signals?profile=equities" in contract
    assert "/api/v1/params?profile=equities" in contract
    assert "trade[XYZ]" in contract
