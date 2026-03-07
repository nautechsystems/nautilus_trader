from __future__ import annotations

import tomllib
from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _tokenmm_strategy_ids() -> list[str]:
    config = tomllib.load((_repo_root() / "deploy/tokenmm/tokenmm.live.toml").open("rb"))
    raw_ids = config.get("api", {}).get("tokenmm_strategy_ids") or []
    return [str(item).strip() for item in raw_ids if str(item).strip()]


def _strategy_config_path(strategy_id: str) -> Path:
    strategies_dir = _repo_root() / "deploy/tokenmm/strategies"
    active_path = strategies_dir / f"{strategy_id}.toml"
    if active_path.is_file():
        return active_path
    disabled_path = strategies_dir / f"{strategy_id}.toml.disabled"
    return disabled_path


TOKENMM_STRATEGY_IDS = _tokenmm_strategy_ids()


def test_tokenmm_stack_script_defaults_to_safe_non_trading_runtime() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert 'DEFAULT_ENV_PATH="${ROOT_DIR}/deploy/tokenmm/tokenmm_stack.env"' in script
    assert (
        'CONFIG_PATH="${TOKENMM_CONFIG_PATH:-${ROOT_DIR}/deploy/tokenmm/tokenmm.live.toml}"'
        in script
    )
    assert (
        'STRATEGIES_DIR="${TOKENMM_STRATEGIES_DIR:-${ROOT_DIR}/deploy/tokenmm/strategies}"'
        in script
    )
    assert 'MODE="${TOKENMM_MODE:-paper}"' in script
    assert 'CONFIRM_LIVE="${TOKENMM_CONFIRM_LIVE:-0}"' in script
    assert 'ENABLE_EXECUTION="${TOKENMM_ENABLE_EXECUTION:-0}"' in script
    assert 'ALLOW_MISSING_KEYS="${TOKENMM_ALLOW_MISSING_KEYS:-0}"' in script
    assert 'BALANCES_READY_TIMEOUT_SECS="${TOKENMM_BALANCES_READY_TIMEOUT_SECS:-90}"' in script
    assert 'STRICT_BALANCES_READY_CHECK="${TOKENMM_STRICT_BALANCES_READY_CHECK:-1}"' in script
    assert "local smoke only; production service management belongs in Pulse" in script
    assert "live deployments are not supported via tokenmm_stack.sh" in script
    assert (
        "[tokenmm-stack] runtime intent: mode=${MODE} confirm_live=${CONFIRM_LIVE} enable_execution=${ENABLE_EXECUTION}"
        in script
    )


def test_tokenmm_stack_script_manages_portfolio_aggregator_service() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert "flux.runners.tokenmm.run_portfolio" in script
    assert 'start_process "portfolio"' in script
    assert 'stop_process "portfolio"' in script
    assert 'service_status_line "portfolio"' in script


def test_tokenmm_stack_script_builds_and_serves_pulse_ui() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert 'SKIP_PULSE_BUILD="${TOKENMM_SKIP_PULSE_BUILD:-0}"' in script
    assert "building pulse ui" in script
    assert 'pnpm --dir "${ROOT_DIR}/pulse-ui" build' in script
    assert "--serve-pulse" in script
    assert '"PULSE_SERVE_DIST=1"' in script


def test_tokenmm_systemd_installer_wires_pulse_metadata_for_live_services() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/install_tokenmm_systemd.sh")

    assert "strategy_stack_write_env" in script
    assert '"tokenmm"' in script
    assert '"TokenMM"' in script
    assert '"10"' in script
    assert '"tokenmm-api"' in script
    assert '"tokenmm-pulse"' in script
    assert "--serve-pulse" in script


def test_tokenmm_stack_script_requires_explicit_tokenmm_env_and_never_falls_back_to_makerv3() -> (
    None
):
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert "TOKENMM_* | BYBIT_* | BINANCE_* | OKX_*" in script
    assert "MAKERV3_*" not in script
    assert "SHARED_ENV_FALLBACK_PATH" not in script
    assert "[tokenmm-stack] using shared env fallback:" not in script
    assert 'if [[ -n "${TOKENMM_ENV_PATH:-}" && ! -f "${ENV_PATH}" ]]; then' in script
    assert "[tokenmm-stack] env file not found: ${ENV_PATH}" in script
    assert "MAKERV3_MODE" not in script
    assert "MAKERV3_CONFIRM_LIVE" not in script
    assert "MAKERV3_ENABLE_EXECUTION" not in script
    assert "MAKERV3_ALLOW_MISSING_KEYS" not in script


def test_tokenmm_stack_script_loads_aws_secrets_before_key_validation() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert 'LOAD_AWS_SECRETS="${TOKENMM_LOAD_AWS_SECRETS:-0}"' in script
    assert 'AWS_REGION="${TOKENMM_AWS_REGION:-ap-southeast-1}"' in script
    assert 'BYBIT_SECRET_ID="${TOKENMM_BYBIT_SECRET_ID:-/nautilus/tokenmm/bybit}"' in script
    assert 'BINANCE_SECRET_ID="${TOKENMM_BINANCE_SECRET_ID:-/nautilus/tokenmm/binance}"' in script
    assert 'OKX_SECRET_ID="${TOKENMM_OKX_SECRET_ID:-/nautilus/tokenmm/okx}"' in script
    assert "load_aws_secrets_if_enabled()" in script

    start_stack_idx = script.index("start_stack() {")
    call_idx = script.index("  load_aws_secrets_if_enabled", start_stack_idx)
    validate_idx = script.index("  validate_config_and_keys", start_stack_idx)
    assert call_idx < validate_idx


def test_tokenmm_stack_script_does_not_inherit_execution_toggle_from_makerv3_env() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert "MAKERV3_ENABLE_EXECUTION" not in script
    assert "MAKERV3_MODE" not in script
    assert "MAKERV3_CONFIRM_LIVE" not in script
    assert "MAKERV3_ALLOW_MISSING_KEYS" not in script


def test_tokenmm_stack_script_supports_public_api_bind_targets() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    start_stack_idx = script.index("start_stack() {")
    validate_mode_idx = script.index("  validate_mode", start_stack_idx)
    log_intent_idx = script.index("  log_runtime_intent", start_stack_idx)
    assert validate_mode_idx < log_intent_idx
    assert 'API_HOST="${TOKENMM_API_HOST:-}"' in script
    assert 'resolve_api_bind_from_config "${pybin}"' in script
    assert 'EXPECTED_NODES="${TOKENMM_EXPECTED_NODES:-0}"' in script
    assert "TOKENMM_API_HOST must be loopback-only" not in script


def test_tokenmm_stack_script_limits_secret_imports_to_exchange_credentials() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    load_secret_idx = script.index("load_secret_into_env() {")
    assert "BYBIT_*|BINANCE_*|OKX_*)" in script[load_secret_idx:]
    assert "warning: skipping unsupported secret key" in script[load_secret_idx:]


def test_tokenmm_stack_env_example_defaults_to_safe_paper_without_execution() -> None:
    env_example = _read(
        _repo_root() / "deploy/tokenmm/tokenmm_stack.env.example",
    )

    assert "TOKENMM_MODE=paper" in env_example
    assert "TOKENMM_CONFIRM_LIVE=0" in env_example
    assert "TOKENMM_ENABLE_EXECUTION=0" in env_example
    assert "TOKENMM_ALLOW_MISSING_KEYS=0" in env_example
    assert "TOKENMM_LOAD_AWS_SECRETS=0" in env_example
    assert "TOKENMM_AWS_REGION=ap-southeast-1" in env_example
    assert "TOKENMM_OKX_SECRET_ID=/nautilus/tokenmm/okx" in env_example
    assert "TOKENMM_BALANCES_READY_TIMEOUT_SECS=90" in env_example
    assert "TOKENMM_STRICT_BALANCES_READY_CHECK=1" in env_example
    assert "TOKENMM_API_HOST=127.0.0.1" in env_example
    assert "TOKENMM_EXPECTED_NODES=0" in env_example
    assert "deploy/tokenmm/tokenmm.live.toml" in env_example
    assert "Production/live service management is unsupported through this env file." in env_example
    assert "/etc/flux/common.env" in env_example
    assert "/pulse" in env_example


def test_tokenmm_stack_script_rolls_back_partial_startup_on_failure() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert "STARTUP_CLEANUP_ON_EXIT=0" in script
    assert "cleanup_partial_startup_on_exit() {" in script
    assert "[tokenmm-stack] startup failed; stopping partial stack" in script
    assert "trap cleanup_partial_startup_on_exit EXIT" in script

    main_idx = script.index("main() {")
    start_case_idx = script.index("    start)", main_idx)
    start_arm_idx = script.index("      STARTUP_CLEANUP_ON_EXIT=1", start_case_idx)
    start_call_idx = script.index("      start_stack", start_case_idx)
    start_disarm_idx = script.index("      STARTUP_CLEANUP_ON_EXIT=0", start_call_idx)
    assert start_arm_idx < start_call_idx < start_disarm_idx

    restart_case_idx = script.index("    restart)", main_idx)
    restart_stop_idx = script.index("      stop_stack", restart_case_idx)
    restart_arm_idx = script.index("      STARTUP_CLEANUP_ON_EXIT=1", restart_case_idx)
    restart_call_idx = script.index("      start_stack", restart_case_idx)
    restart_disarm_idx = script.index("      STARTUP_CLEANUP_ON_EXIT=0", restart_call_idx)
    assert restart_stop_idx < restart_arm_idx < restart_call_idx < restart_disarm_idx


def test_tokenmm_stack_script_waits_for_balances_readiness_after_profile_alignment() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    start_stack_idx = script.index("start_stack() {")
    params_check_idx = script.index("  assert_tokenmm_profile_params_alignment", start_stack_idx)
    balances_check_idx = script.index(
        "  assert_tokenmm_profile_balances_readiness",
        start_stack_idx,
    )
    assert params_check_idx < balances_check_idx
    assert "missing_required" in script
    assert "required_stale" in script
    assert "required_stale_without_ts" not in script


def test_tokenmm_stack_script_requires_nonempty_tokenmm_registry_allowlist() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert "[api].tokenmm_strategy_ids must be non-empty" in script
    assert "skipping TokenMM profile assertions: [api].tokenmm_strategy_ids is empty" not in script


def test_tokenmm_stack_script_uses_loopback_for_internal_health_checks_when_publicly_bound() -> (
    None
):
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert "api_request_host() {" in script
    assert 'if [[ "${API_HOST}" == "0.0.0.0" ]]; then' in script
    assert 'echo "127.0.0.1"' in script
    assert 'request_base_url="$(api_request_base_url)"' in script
    assert 'curl -fsS "${request_base_url}/api/v1/healthz"' in script
    assert 'curl -fsS "${request_base_url}/socket.io/?EIO=4&transport=polling"' in script


def test_tokenmm_stack_script_only_imports_exchange_secret_keys() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert 'case "${key}" in' in script
    assert "BYBIT_*|BINANCE_*|OKX_*)" in script
    assert "skipping unsupported secret key" in script


def test_tokenmm_stack_script_validates_flux_and_nautilus_strategy_id_uniqueness() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert "duplicate [identity].strategy_id in strategy configs" in script
    assert "duplicate [strategy].strategy_id in strategy configs" in script


def test_tokenmm_stack_script_prefers_setsid_detach_and_keeps_nohup_fallback() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")
    start_process_idx = script.index("start_process() {")

    assert "if command -v setsid > /dev/null 2>&1; then" in script[start_process_idx:]
    assert 'setsid nohup "$@" >> "${log}" 2>&1 < /dev/null &' in script[start_process_idx:]
    assert 'setsid "$@" >> "${log}" 2>&1 < /dev/null &' in script[start_process_idx:]
    assert 'nohup "$@" >> "${log}" 2>&1 < /dev/null &' in script[start_process_idx:]
    assert '"$@" >> "${log}" 2>&1 < /dev/null &' in script[start_process_idx:]


def test_tokenmm_stack_script_exports_resolved_redis_runtime_to_all_services() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    for token in (
        '"TOKENMM_REDIS_HOST=${REDIS_HOST}"',
        '"TOKENMM_REDIS_PORT=${REDIS_PORT}"',
        '"TOKENMM_REDIS_DB=${REDIS_DB}"',
        '"TOKENMM_REDIS_USERNAME=${REDIS_USERNAME}"',
        '"TOKENMM_REDIS_PASSWORD=${REDIS_PASSWORD}"',
        '"TOKENMM_REDIS_SSL=${REDIS_SSL}"',
    ):
        assert script.count(token) >= 3


def test_tokenmm_live_configs_enable_shared_account_reconciliation_guardrails() -> None:
    root = _repo_root()
    config_paths = [
        root / "deploy/tokenmm/tokenmm.live.toml",
        root / "deploy/tokenmm/strategies/tokenmm.strategy.template.toml",
    ]
    config_paths.extend(_strategy_config_path(strategy_id) for strategy_id in TOKENMM_STRATEGY_IDS)

    for path in config_paths:
        text = _read(path)
        assert "exec_reconciliation_lookback_mins = 15" in text
        assert "filter_unclaimed_external_orders = true" in text
        assert "filter_position_reports = false" in text


def test_tokenmm_strategy_configs_inherit_redis_from_shared_top_level_config() -> None:
    repo_root = _repo_root()
    strategy_paths = [
        repo_root / "deploy/tokenmm/strategies/tokenmm.strategy.template.toml",
        *(_strategy_config_path(strategy_id) for strategy_id in TOKENMM_STRATEGY_IDS),
    ]

    for path in strategy_paths:
        config = _read(path)
        assert "[redis]" not in config

    shared_config = _read(repo_root / "deploy/tokenmm/tokenmm.live.toml")
    assert "[redis]" in shared_config


def test_tokenmm_live_configs_use_generic_node_venues_contract() -> None:
    repo_root = _repo_root()
    config_paths = [
        repo_root / "deploy/tokenmm/tokenmm.live.toml",
        repo_root / "deploy/tokenmm/strategies/tokenmm.strategy.template.toml",
        repo_root / "examples/live/makerv3/config/makerv3.toml",
    ]
    config_paths.extend(_strategy_config_path(strategy_id) for strategy_id in TOKENMM_STRATEGY_IDS)

    for path in config_paths:
        config = _read(path)
        assert "[node.venues." in config
        assert "instrument_id =" in config
        assert "[node.bybit]" not in config
        assert "[node.binance]" not in config


def test_tokenmm_systemd_artifacts_define_env_driven_flux_units() -> None:
    repo_root = _repo_root()
    service_template = _read(repo_root / "deploy/systemd/flux@.service")
    target_unit = _read(repo_root / "deploy/tokenmm/systemd/flux-tokenmm.target")
    install_script = _read(repo_root / "ops/scripts/deploy/install_tokenmm_systemd.sh")
    common_env = _read(repo_root / "deploy/tokenmm/systemd/common.env.example")
    sudoers = _read(repo_root / "deploy/tokenmm/systemd/flux-pulse.sudoers")

    assert "EnvironmentFile=-/etc/flux/common.env" in service_template
    assert "EnvironmentFile=/etc/flux/%i.env" in service_template
    assert "ExecStart=/bin/bash -lc 'cd \"${WORKDIR:?}\" && exec ${CMD}'" in service_template
    assert 'if [ -n "${PORT:-}" ]; then' in service_template
    assert "SyslogIdentifier=flux-%i" in service_template
    assert "NoNewPrivileges=true" not in service_template

    assert "[Install]" in target_unit
    assert "WantedBy=multi-user.target" in target_unit
    assert "Wants=flux@tokenmm-api.service" in target_unit
    assert "Wants=flux@tokenmm-pulse.service" in target_unit
    assert "Wants=flux@tokenmm-portfolio.service" in target_unit
    assert "Wants=flux@tokenmm-bridge.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_bybit_perp_makerv3.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_bybit_spot_makerv3.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_okx_perp_makerv3.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_binance_spot_makerv3.service" in target_unit

    assert "deploy/tokenmm/tokenmm.live.toml" in install_script
    assert "/etc/flux" in install_script
    assert "/etc/sudoers.d/flux-pulse" in install_script
    assert "strategy_stack_discover_strategy_ids" in install_script
    assert "plumeusdt_bybit_perp_makerv3" not in install_script
    assert (
        'env FLUXBOARD_SERVE_DIST=1 python3 -m nautilus_trader.flux.runners.tokenmm.run_api'
        in install_script
    )
    assert "--serve-fluxboard" in install_script
    assert "--host 0.0.0.0" in install_script
    assert 'env PULSE_SERVE_DIST=1 python3 -m nautilus_trader.flux.runners.tokenmm.run_api --config ${SHARED_CONFIG} --mode live --confirm-live --host 127.0.0.1 --port 5023 --serve-pulse' in install_script
    assert "tokenmm-portfolio" in install_script
    assert "tokenmm-pulse" in install_script
    assert 'service_id="tokenmm-node-${strategy_id}"' in install_script
    assert "tokenmm-api" in install_script
    assert "--strategy-id ${strategy_id}" in install_script
    assert "--all-strategies" not in install_script

    assert "TOKENMM_REDIS_PASSWORD=" in common_env
    assert "BYBIT_API_KEY=" in common_env
    assert "BINANCE_API_KEY=" in common_env
    assert "OKX_API_KEY=" in common_env

    assert "/usr/bin/systemctl start flux@tokenmm-api.service" in sudoers
    assert "/usr/bin/systemctl start flux@tokenmm-pulse.service" in sudoers
    assert "/usr/bin/systemctl start flux@tokenmm-bridge.service" in sudoers
    assert "/usr/bin/systemctl restart flux@tokenmm-portfolio.service" in sudoers
    assert "/usr/bin/systemctl restart flux@tokenmm-node-plumeusdt_bybit_perp_makerv3.service" in sudoers
    assert "/usr/bin/journalctl -u flux@tokenmm-api.service" in sudoers
    assert "flux@*" not in sudoers


def test_tokenmm_shared_account_live_configs_enable_reconciliation_filters_with_bounded_lookback() -> (
    None
):
    repo_root = _repo_root()
    config_paths = [
        repo_root / "deploy/tokenmm/tokenmm.live.toml",
        repo_root / "deploy/tokenmm/strategies/tokenmm.strategy.template.toml",
        *(_strategy_config_path(strategy_id) for strategy_id in TOKENMM_STRATEGY_IDS),
    ]

    for path in config_paths:
        config = _read(path)
        assert "exec_reconciliation_lookback_mins = 15" in config
        assert "filter_unclaimed_external_orders = true" in config
        assert "filter_position_reports = false" in config


def test_tokenmm_strategy_configs_explicitly_set_manage_stop_false() -> None:
    repo_root = _repo_root()
    config_paths = [
        repo_root / "deploy/tokenmm/strategies/tokenmm.strategy.template.toml",
        *(_strategy_config_path(strategy_id) for strategy_id in TOKENMM_STRATEGY_IDS),
    ]

    for path in config_paths:
        config = _read(path)
        assert "manage_stop = false" in config


def test_tokenmm_live_configs_explicitly_disable_generate_missing_orders() -> None:
    repo_root = _repo_root()
    config_paths = [
        repo_root / "deploy/tokenmm/tokenmm.live.toml",
        repo_root / "deploy/tokenmm/strategies/tokenmm.strategy.template.toml",
        *(_strategy_config_path(strategy_id) for strategy_id in TOKENMM_STRATEGY_IDS),
    ]

    for path in config_paths:
        config = _read(path)
        assert "exec_generate_missing_orders = false" in config


def test_tokenmm_production_strategy_configs_use_descriptive_strategy_ids() -> None:
    strategy_paths = [_strategy_config_path(strategy_id) for strategy_id in TOKENMM_STRATEGY_IDS]

    assert len(strategy_paths) == len(TOKENMM_STRATEGY_IDS)
    for path in strategy_paths:
        config = _read(path)
        strategy_id = path.stem
        assert f'strategy_id = "{strategy_id}"' in config
        assert "MAKERV3-TMM-" not in config
        assert "tokenmm_plume_makerv3" not in config
        assert "_01" not in strategy_id
        assert "_02" not in strategy_id
        assert "_03" not in strategy_id
        assert "_04" not in strategy_id
        assert "_05" not in strategy_id


def test_tokenmm_registry_uses_execution_scoped_ids_not_tokenmm_clone_ids() -> None:
    config = _read(_repo_root() / "deploy/tokenmm/tokenmm.live.toml")

    for strategy_id in TOKENMM_STRATEGY_IDS:
        assert f'"{strategy_id}"' in config
    assert "tokenmm_plume_makerv3" not in config
    assert 'host = "0.0.0.0"' in config


def test_tokenmm_api_contract_catalog_lists_distinct_plume_spot_and_perp_instruments() -> None:
    config = _read(_repo_root() / "deploy/tokenmm/tokenmm.live.toml")

    assert 'instrument_id = "PLUMEUSDT-LINEAR.BYBIT"' in config
    assert 'instrument_id = "PLUMEUSDT-SPOT.BYBIT"' in config
    assert 'instrument_id = "PLUMEUSDT.BINANCE_SPOT"' in config
    assert 'instrument_id = "PLUME-USDT-SWAP.OKX"' in config


def test_tokenmm_strategy_configs_use_requested_execution_and_reference_markets() -> None:
    expectations = {
        "plumeusdt_bybit_perp_makerv3": (
            'execution_venue = "BYBIT"',
            'reference_venue = "BINANCE_SPOT"',
            'instrument_id = "PLUMEUSDT-LINEAR.BYBIT"',
            'instrument_id = "PLUMEUSDT.BINANCE_SPOT"',
        ),
        "plumeusdt_bybit_spot_makerv3": (
            'execution_venue = "BYBIT"',
            'reference_venue = "BINANCE_SPOT"',
            'instrument_id = "PLUMEUSDT-SPOT.BYBIT"',
            'instrument_id = "PLUMEUSDT.BINANCE_SPOT"',
        ),
        "plumeusdt_okx_perp_makerv3": (
            'execution_venue = "OKX"',
            'reference_venue = "BINANCE_SPOT"',
            'instrument_id = "PLUME-USDT-SWAP.OKX"',
            'instrument_id = "PLUMEUSDT.BINANCE_SPOT"',
        ),
        "plumeusdt_binance_perp_makerv3": (
            'execution_venue = "BINANCE_PERP"',
            'reference_venue = "BINANCE_SPOT"',
            'instrument_id = "PLUMEUSDT-PERP.BINANCE_PERP"',
            'instrument_id = "PLUMEUSDT.BINANCE_SPOT"',
        ),
        "plumeusdt_binance_spot_makerv3": (
            'execution_venue = "BINANCE_SPOT"',
            'reference_venue = "BINANCE_SPOT"',
            'instrument_id = "PLUMEUSDT.BINANCE_SPOT"',
        ),
    }

    for strategy_id, required_lines in expectations.items():
        if strategy_id not in TOKENMM_STRATEGY_IDS:
            continue
        config = _read(_strategy_config_path(strategy_id))
        for line in required_lines:
            assert line in config


def test_tokenmm_okx_perp_configs_default_to_cross_margin() -> None:
    repo_root = _repo_root()
    template = _read(repo_root / "deploy/tokenmm/strategies/tokenmm.strategy.template.toml")
    okx_config = _read(_strategy_config_path("plumeusdt_okx_perp_makerv3"))

    assert 'margin_mode = "CROSS"' in okx_config
    assert 'OKX perp configs should set `margin_mode = "CROSS"`' in template


def test_deploy_env_examples_default_to_safe_paper_profiles_and_direct_prod_rejection() -> None:
    tokenmm_env = _read(_repo_root() / "deploy/tokenmm/tokenmm_stack.env.example")

    assert "TOKENMM_MODE=paper" in tokenmm_env
    assert "TOKENMM_CONFIRM_LIVE=0" in tokenmm_env
    assert "TOKENMM_ENABLE_EXECUTION=0" in tokenmm_env
    assert "Production/live service management is unsupported through this env file." in tokenmm_env
    assert "sudo ops/scripts/deploy/install_tokenmm_systemd.sh" in tokenmm_env
    assert "http://127.0.0.1:5022/pulse" in tokenmm_env


def test_deploy_stack_scripts_use_package_runner_entrypoints() -> None:
    tokenmm_script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")
    assert "flux.runners.tokenmm.run_node" in tokenmm_script
    assert "--shared-config" in tokenmm_script
    assert '"${CONFIG_PATH}"' in tokenmm_script
    assert "flux.runners.tokenmm.run_bridge" in tokenmm_script
    assert "flux.runners.tokenmm.run_api" in tokenmm_script
    assert "flux.runners.makerv3" not in tokenmm_script
    assert "examples/live/makerv3/run_node.py" not in tokenmm_script
    assert "examples/live/makerv3/run_bridge.py" not in tokenmm_script
    assert "examples/live/makerv3/run_api.py" not in tokenmm_script


def test_legacy_makerv3_production_surface_is_removed() -> None:
    repo_root = _repo_root()

    assert not (repo_root / "scripts/deploy/makerv3_stack.sh").exists()
    assert not (repo_root / "deploy/makerv3").exists()
    assert not (repo_root / "nautilus_trader/flux/runners/makerv3").exists()


def test_deploy_docs_make_runtime_intent_and_reconciliation_guardrails_explicit() -> None:
    readme = _read(_repo_root() / "deploy/tokenmm/README.md")
    strategies_readme = _read(_repo_root() / "deploy/tokenmm/strategies/README.md")
    examples_readme = _read(_repo_root() / "examples/live/makerv3/README.md")

    assert (
        "Supported production lifecycle: install with systemd, then manage jobs from Pulse."
        in readme
    )
    assert "tokenmm_stack.sh` is local smoke only" in readme
    assert "TOKENMM_MODE=paper" in readme
    assert "TOKENMM_CONFIRM_LIVE=0" in readme
    assert "TOKENMM_ENABLE_EXECUTION=0" in readme
    assert "TOKENMM_ALLOW_MISSING_KEYS=1" in readme
    assert "GET /api/v1/params?profile=tokenmm" in readme
    assert "GET /api/v1/balances?profile=tokenmm" in readme
    assert "GET /api/v1/trades?profile=tokenmm" in readme
    assert "POST /api/pulse/jobs/group/tokenmm/restart" in readme
    assert "flux.runners.tokenmm.run_node" in readme
    assert "flux.runners.tokenmm.run_portfolio" in readme
    assert "flux.runners.tokenmm.run_bridge" in readme
    assert "flux.runners.tokenmm.run_api" in readme
    assert "deploy/makerv3" not in readme
    assert "runners.makerv3" not in readme

    assert "Production node lifecycle is managed from Pulse via flux@ units." in strategies_readme
    assert "`exec_reconciliation_lookback_mins`" in strategies_readme
    assert "`15`" in strategies_readme
    assert "filter_unclaimed_external_orders = true" in strategies_readme
    assert "filter_position_reports = false" in strategies_readme
    assert "flux.runners.tokenmm.run_node" in strategies_readme
    assert "/etc/flux/tokenmm-node-" in strategies_readme
    assert "deploy/makerv3" not in strategies_readme
    assert "runners.makerv3" not in strategies_readme

    assert "Production deploy docs default to paper/no-exec smoke first." in examples_readme
    assert "`deploy/tokenmm/README.md`" in examples_readme
    assert "Production management lives behind Pulse at `/pulse`." in examples_readme
    assert "deploy/makerv3" not in examples_readme
    assert "flux.runners.makerv3" not in examples_readme


def test_flux_prod_docs_reference_tokenmm_runner_namespace() -> None:
    api_doc = _read(_repo_root() / "systems/flux/docs/api.md")
    bridge_doc = _read(_repo_root() / "systems/flux/docs/bridge.md")
    runbook = _read(_repo_root() / "apps/fluxboard/docs/tokenmm_runbook.md")

    assert "flux.runners.tokenmm.run_api" in api_doc
    assert "flux.runners.makerv3.run_api" not in api_doc

    assert "flux.runners.tokenmm.run_bridge" in bridge_doc
    assert "flux.runners.makerv3.run_bridge" not in bridge_doc

    assert "flux.runners.tokenmm.run_node" in runbook
    assert "flux.runners.tokenmm.run_portfolio" in runbook
    assert "flux.runners.tokenmm.run_bridge" in runbook
    assert "flux.runners.tokenmm.run_api" in runbook
    assert "supported production lifecycle is Pulse-first" in runbook
    assert "bootstrap or disaster recovery only" in runbook
    assert "flux.runners.makerv3" not in runbook
