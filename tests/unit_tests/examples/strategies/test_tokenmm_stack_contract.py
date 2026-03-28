from __future__ import annotations

from copy import deepcopy
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


def _tokenmm_required_strategy_ids() -> list[str]:
    config = tomllib.load((_repo_root() / "deploy/tokenmm/tokenmm.live.toml").open("rb"))
    raw_ids = config.get("api", {}).get("tokenmm_required_strategy_ids") or []
    return [str(item).strip() for item in raw_ids if str(item).strip()]


def _tokenmm_account_scopes() -> list[dict]:
    config = tomllib.load((_repo_root() / "deploy/tokenmm/tokenmm.live.toml").open("rb"))
    raw_rows = config.get("account_scopes") or []
    return [dict(row) for row in raw_rows if isinstance(row, dict)]


def _tokenmm_strategy_contracts() -> list[dict]:
    config = tomllib.load((_repo_root() / "deploy/tokenmm/tokenmm.live.toml").open("rb"))
    raw_rows = config.get("strategy_contracts") or []
    return [dict(row) for row in raw_rows if isinstance(row, dict)]


def _strategy_config_path(strategy_id: str) -> Path:
    strategies_dir = _repo_root() / "deploy/tokenmm/strategies"
    active_path = strategies_dir / f"{strategy_id}.toml"
    if active_path.is_file():
        return active_path
    disabled_path = strategies_dir / f"{strategy_id}.toml.disabled"
    return disabled_path


TOKENMM_STRATEGY_IDS = _tokenmm_strategy_ids()
TOKENMM_REQUIRED_STRATEGY_IDS = _tokenmm_required_strategy_ids()
TOKENMM_ACCOUNT_SCOPES = _tokenmm_account_scopes()
TOKENMM_STRATEGY_CONTRACTS = _tokenmm_strategy_contracts()
TOKENMM_SUPPORTED_CORE_STRATEGY_IDS = [
    "plumeusdt_bybit_perp_makerv3",
    "plumeusdt_bybit_spot_makerv3",
    "plumeusdt_okx_perp_makerv3",
    "plumeusdt_binance_perp_makerv3",
    "plumeusdt_binance_spot_makerv3",
    "plumeusdt_bitget_perp_makerv3",
    "plumeusdt_bitget_spot_makerv3",
]
DEPLOY_ROOT_PLACEHOLDER = "/absolute/path/to/deploy-root"


def _assert_tokenmm_binance_spot_strategy_identity_contract(strategy_config: dict) -> None:
    assert strategy_config["identity"]["strategy_id"] == "plumeusdt_binance_spot_makerv3"
    assert strategy_config["identity"]["strategy_instance_id"] == "plumeusdt_binance_spot_makerv3"
    assert strategy_config["identity"]["external_strategy_id"] == "plumeusdt_binance_spot_makerv3"
    assert strategy_config["strategy"]["strategy_id"] == "plumeusdt_binance_spot_makerv3"
    assert strategy_config["node"]["venues"]["BINANCE_SPOT"]["api_key_env"] == "BINANCE_API_KEY"
    assert strategy_config["node"]["venues"]["BINANCE_SPOT"]["api_secret_env"] == "BINANCE_API_SECRET"


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


def test_tokenmm_binance_spot_strategy_uses_supported_margin_family_account_type() -> None:
    shared_config = tomllib.load((_repo_root() / "deploy/tokenmm/tokenmm.live.toml").open("rb"))
    strategy_config = tomllib.load(
        (_repo_root() / "deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml").open(
            "rb",
        ),
    )

    assert shared_config["node"]["venues"]["BINANCE_SPOT"]["account_type"] == "MARGIN"
    assert strategy_config["node"]["venues"]["BINANCE_SPOT"]["account_type"] == "PORTFOLIO_MARGIN"
    _assert_tokenmm_binance_spot_strategy_identity_contract(strategy_config)


def test_tokenmm_binance_spot_portfolio_margin_variant_preserves_identity_contract() -> None:
    strategy_config = tomllib.load(
        (_repo_root() / "deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml").open(
            "rb",
        ),
    )
    pm_variant = deepcopy(strategy_config)
    pm_variant["node"]["venues"]["BINANCE_SPOT"]["account_type"] = "PORTFOLIO_MARGIN"

    assert pm_variant["node"]["venues"]["BINANCE_SPOT"]["account_type"] == "PORTFOLIO_MARGIN"
    _assert_tokenmm_binance_spot_strategy_identity_contract(pm_variant)


def test_tokenmm_binance_spot_strategy_declares_cash_borrowing_contract() -> None:
    strategy_config = tomllib.load(
        (_repo_root() / "deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml").open(
            "rb",
        ),
    )

    assert strategy_config["node"]["venues"]["BINANCE_SPOT"]["allow_cash_borrowing"] is True
    assert strategy_config["strategy"]["spot_cash_borrowing_policy"] == "both_sides"


def test_tokenmm_binance_spot_strategy_pins_supported_margin_contract() -> None:
    strategy_config = tomllib.load(
        (_repo_root() / "deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml").open(
            "rb",
        ),
    )

    assert strategy_config["node"]["venues"]["BINANCE_SPOT"]["account_type"] == "PORTFOLIO_MARGIN"
    assert strategy_config["node"]["venues"]["BINANCE_SPOT"]["allow_cash_borrowing"] is True
    assert strategy_config["strategy"]["spot_cash_borrowing_policy"] == "both_sides"
    assert strategy_config["strategy"]["force_bot_off_on_start"] is True
    assert strategy_config["strategy"]["bot_on"] is False


def test_tokenmm_binance_perp_strategy_uses_portfolio_margin_private_api_family() -> None:
    strategy_config = tomllib.load(
        (_repo_root() / "deploy/tokenmm/strategies/plumeusdt_binance_perp_makerv3.toml").open(
            "rb",
        ),
    )

    perp_venue = strategy_config["node"]["venues"]["BINANCE_PERP"]
    spot_venue = strategy_config["node"]["venues"]["BINANCE_SPOT"]

    assert perp_venue["api_key_env"] == "BINANCE_API_KEY"
    assert perp_venue["api_secret_env"] == "BINANCE_API_SECRET"
    assert perp_venue["account_type"] == "USDT_FUTURES"
    assert perp_venue["private_api_family"] == "PORTFOLIO_MARGIN"
    assert spot_venue["api_key_env"] == "BINANCE_API_KEY"
    assert spot_venue["api_secret_env"] == "BINANCE_API_SECRET"


def test_tokenmm_bitget_spot_strategy_declares_uta_borrowing_contract() -> None:
    strategy_config = tomllib.load(
        (_repo_root() / "deploy/tokenmm/strategies/plumeusdt_bitget_spot_makerv3.toml").open(
            "rb",
        ),
    )

    assert strategy_config["node"]["venues"]["BITGET"]["account_mode"] == "UTA"
    assert strategy_config["node"]["venues"]["BITGET"]["allow_cash_borrowing"] is True
    assert strategy_config["node"]["venues"]["BITGET"]["margin_mode"] == "cross"
    assert strategy_config["node"]["venues"]["BITGET"]["position_mode"] == "one_way"
    assert strategy_config["strategy"]["spot_cash_borrowing_policy"] == "sell_only"
    assert strategy_config["strategy"]["force_bot_off_on_start"] is True
    assert strategy_config["strategy"]["bot_on"] is False


def test_tokenmm_bitget_perp_strategy_declares_uta_one_way_contract() -> None:
    strategy_config = tomllib.load(
        (_repo_root() / "deploy/tokenmm/strategies/plumeusdt_bitget_perp_makerv3.toml").open(
            "rb",
        ),
    )

    assert strategy_config["node"]["venues"]["BITGET"]["account_mode"] == "UTA"
    assert strategy_config["node"]["venues"]["BITGET"]["margin_mode"] == "cross"
    assert strategy_config["node"]["venues"]["BITGET"]["position_mode"] == "one_way"
    assert strategy_config["strategy"]["force_bot_off_on_start"] is True
    assert strategy_config["strategy"]["bot_on"] is False


def test_tokenmm_active_strategy_ids_have_active_toml_files() -> None:
    strategies_dir = _repo_root() / "deploy/tokenmm/strategies"

    for strategy_id in TOKENMM_STRATEGY_IDS:
        assert (strategies_dir / f"{strategy_id}.toml").is_file()
        assert not (strategies_dir / f"{strategy_id}.toml.disabled").exists()


def test_tokenmm_registry_requires_all_supported_live_core_strategies() -> None:
    assert TOKENMM_STRATEGY_IDS == [
        "plumeusdt_bybit_perp_makerv3",
        "plumeusdt_bybit_spot_makerv3",
        "plumeusdt_okx_perp_makerv3",
        "plumeusdt_binance_perp_makerv3",
        "plumeusdt_binance_spot_makerv3",
        "plumeusdt_bitget_perp_makerv3",
        "plumeusdt_bitget_spot_makerv3",
    ]
    assert len(TOKENMM_STRATEGY_IDS) == 7
    assert TOKENMM_REQUIRED_STRATEGY_IDS == TOKENMM_SUPPORTED_CORE_STRATEGY_IDS
    assert len(TOKENMM_REQUIRED_STRATEGY_IDS) == 7
    assert set(TOKENMM_REQUIRED_STRATEGY_IDS).issubset(TOKENMM_STRATEGY_IDS)
    assert "plumeusdt_binance_perp_makerv3" in TOKENMM_REQUIRED_STRATEGY_IDS
    assert "plumeusdt_binance_spot_makerv3" in TOKENMM_REQUIRED_STRATEGY_IDS


def test_tokenmm_live_config_declares_shared_account_scope_for_binance() -> None:
    scopes_by_id = {
        str(row["scope_id"]).strip(): row
        for row in TOKENMM_ACCOUNT_SCOPES
        if str(row.get("scope_id") or "").strip()
    }

    assert "binance.pm.main" in scopes_by_id
    assert scopes_by_id["binance.pm.main"]["provider"] == "binance"
    assert scopes_by_id["binance.pm.main"]["venue"] == "BINANCE"
    assert scopes_by_id["binance.pm.main"]["api_key_env"] == "BINANCE_API_KEY"
    assert scopes_by_id["binance.pm.main"]["api_secret_env"] == "BINANCE_API_SECRET"
    assert scopes_by_id["binance.pm.main"]["private_api_family"] == "PORTFOLIO_MARGIN"


def test_tokenmm_live_config_declares_strategy_contracts_for_tokenmm_allowlist() -> None:
    contracts_by_strategy = {
        str(row["strategy_id"]).strip(): row
        for row in TOKENMM_STRATEGY_CONTRACTS
        if str(row.get("strategy_id") or "").strip()
    }

    assert set(TOKENMM_STRATEGY_IDS).issubset(contracts_by_strategy)
    assert len(contracts_by_strategy) == len(TOKENMM_STRATEGY_IDS)

    for strategy_id in TOKENMM_STRATEGY_IDS:
        contract = contracts_by_strategy[strategy_id]
        assert contract["portfolio_asset_id"] == "PLUME"
        assert str(contract["maker_instrument_id"]).strip()
        assert str(contract["reference_instrument_id"]).strip()
        assert str(contract["execution_account_scope_id"]).strip()
        assert str(contract["reference_account_scope_id"]).strip()

    binance_perp = contracts_by_strategy["plumeusdt_binance_perp_makerv3"]
    binance_spot = contracts_by_strategy["plumeusdt_binance_spot_makerv3"]
    assert binance_perp["execution_account_scope_id"] == "binance.pm.main"
    assert binance_spot["execution_account_scope_id"] == "binance.pm.main"
    assert binance_perp["execution_account_scope_id"] == binance_spot["execution_account_scope_id"]


def test_tokenmm_stack_script_builds_and_serves_pulse_ui() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert 'SKIP_PULSE_BUILD="${TOKENMM_SKIP_PULSE_BUILD:-0}"' in script
    assert "building pulse ui" in script
    assert 'pnpm --dir "${ROOT_DIR}/pulse-ui" build' in script
    assert "--serve-pulse" in script
    assert '"PULSE_SERVE_DIST=1"' in script


def test_tokenmm_systemd_installer_wires_pulse_metadata_for_live_services() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/install_tokenmm_systemd.sh")
    readme = _read(_repo_root() / "deploy/tokenmm/README.md")
    runbook = _read(_repo_root() / "docs/fluxboard/tokenmm_runbook.md")

    assert "rebuild_flux_pulse_sudoers.sh" in script
    assert "strategy_stack_write_env" in script
    assert 'ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"' in script
    assert 'DEPLOY_ROOT_OVERRIDE="${TOKENMM_DEPLOY_ROOT:-}"' in script
    assert "resolve_deploy_root() {" in script
    assert "path_is_git_worktree() {" in script
    assert 'DEPLOY_ROOT="$(resolve_deploy_root)"' in script
    assert (
        'echo "[tokenmm-systemd] deploy root missing or not a directory: ${DEPLOY_ROOT}" >&2'
        in script
    )
    assert (
        'echo "[tokenmm-systemd] deploy root must not be a git worktree: ${DEPLOY_ROOT}" >&2'
        in script
    )
    assert script.index('DEPLOY_ROOT="$(resolve_deploy_root)"') < script.index(
        'source "${DEPLOY_ROOT}/ops/scripts/deploy/shared_strategy_stack.sh"',
    )
    assert 'source "${DEPLOY_ROOT}/ops/scripts/deploy/shared_strategy_stack.sh"' in script
    assert 'TOKENMM_PYTHON_BIN="${DEPLOY_ROOT}/.venv/bin/python"' in script
    assert 'SHARED_CONFIG="${DEPLOY_ROOT}/deploy/tokenmm/tokenmm.live.toml"' in script
    assert 'STRATEGIES_DIR="${DEPLOY_ROOT}/deploy/tokenmm/strategies"' in script
    assert "CANONICAL_DEPLOY_ROOT" not in script
    assert "append_checkout_env_overrides" not in script
    assert ('printf \'WORKDIR=%s\\nPYTHONPATH=%s\\n\' "${DEPLOY_ROOT}" "${DEPLOY_ROOT}"') in script
    assert '"tokenmm"' in script
    assert '"TokenMM"' in script
    assert '"10"' in script
    assert '"tokenmm-api"' in script
    assert "--serve-pulse" in script
    assert "--serve-fluxboard" in script
    assert "${TOKENMM_PYTHON_BIN} -m nautilus_trader.flux.runners.tokenmm.run_api" in script
    assert "--port 5022 --serve-fluxboard --serve-pulse" in script
    assert "http://127.0.0.1:5022/pulse" in _read(
        _repo_root() / "deploy/tokenmm/tokenmm_stack.env.example",
    )
    assert "`TOKENMM_DEPLOY_ROOT`" in readme
    assert "`/etc/flux/common.env`" in readme
    assert "non-worktree deploy root" in readme
    assert "pins each TokenMM env file to the checkout used during install" not in readme
    assert "pins each TokenMM env file to the resolved deploy root" in readme
    assert "`TOKENMM_DEPLOY_ROOT`" in runbook
    assert "`/etc/flux/common.env`" in runbook
    assert "non-worktree deploy root" in runbook
    assert "pins `WORKDIR`, `PYTHONPATH`, and the checkout `.venv/bin/python`" not in runbook


def test_tokenmm_jupyter_service_assets_are_localhost_only_and_documented() -> None:
    repo_root = _repo_root()
    pyproject = _read(repo_root / "pyproject.toml")
    install_script = _read(repo_root / "ops/scripts/deploy/install_tokenmm_systemd.sh")
    preflight_wrapper = _read(repo_root / "ops/scripts/deploy/tokenmm_rollout_preflight.py")
    preflight_module = _read(repo_root / "systems/flux/flux/runners/tokenmm/rollout_preflight.py")
    common_env = _read(repo_root / "deploy/tokenmm/systemd/common.env.example")
    jupyter_env = _read(repo_root / "deploy/tokenmm/systemd/tokenmm-jupyter.env.example")
    readme = _read(repo_root / "deploy/tokenmm/README.md")
    research_readme = _read(repo_root / "research/tokenmm/README.md")
    notebook = _read(repo_root / "research/tokenmm/notebooks/tokenmm_trade_data.ipynb")

    assert "notebook = [" in pyproject
    assert "jupyterlab>=" in pyproject
    assert "ipykernel>=" in pyproject

    assert "TOKENMM_TELEMETRY_DIR=/var/lib/nautilus/telemetry/tokenmm" in common_env

    assert "PULSE_ENABLED=0" in jupyter_env
    assert "PORT=8888" in jupyter_env
    assert 'CMD="uv run --group notebook jupyter lab' in jupyter_env
    assert "bash -lc" not in jupyter_env
    assert "--ip=127.0.0.1" in jupyter_env
    assert "--ServerApp.allow_remote_access=False" in jupyter_env
    assert "--ServerApp.root_dir=research/tokenmm" in jupyter_env

    assert "render_jupyter_env()" in install_script
    assert "tokenmm-jupyter.env" in install_script
    assert "tokenmm-jupyter.env.example" in install_script
    assert "tokenmm_rollout_preflight.py" in install_script
    assert (
        '"${TOKENMM_PYTHON_BIN}" "${DEPLOY_ROOT}/ops/scripts/deploy/tokenmm_rollout_preflight.py"'
    ) in install_script
    assert (
        'printf \'WORKDIR=%s\\nPYTHONPATH=%s\\n\' "${DEPLOY_ROOT}" "${DEPLOY_ROOT}"'
    ) in install_script
    assert "python3 -m nautilus_trader.flux.runners.tokenmm" not in install_script

    assert "rollout_preflight" in preflight_wrapper
    assert "collect_rollout_preflight_errors" in preflight_module
    assert "fluxboard/dist/index.html" in preflight_module
    assert "pulse-ui/dist/index.html" in preflight_module
    assert "BitgetEnvironment" in preflight_module

    assert "tokenmm-jupyter.env.example" in readme
    assert "flux@tokenmm-jupyter.service" in readme
    assert "tokenmm_trade_data.ipynb" in readme
    assert "http://127.0.0.1:8888/lab" in readme

    assert "execution_fill" in research_readme
    assert "order_action" in research_readme
    assert "quote_cycle" in research_readme

    assert '"nbformat": 4' in notebook
    assert "execution_fill" in notebook
    assert "order_action" in notebook
    assert "quote_cycle" in notebook


def test_tokenmm_live_runtime_dependencies_are_declared_for_checkout_venv() -> None:
    pyproject = tomllib.loads(_read(_repo_root() / "pyproject.toml"))
    dependencies = {
        str(item).split(">=", 1)[0].split("==", 1)[0].split("[", 1)[0].strip().lower()
        for item in pyproject["project"]["dependencies"]
    }

    assert "flask" in dependencies
    assert "flask-socketio" in dependencies
    assert "psycopg" in dependencies
    assert "redis" in dependencies


def test_tokenmm_docs_cover_telemetry_cutover_and_optional_jupyter_ops() -> None:
    repo_root = _repo_root()
    api_doc = _read(repo_root / "docs/flux/api.md")
    runbook = _read(repo_root / "docs/fluxboard/tokenmm_runbook.md")
    deploy_readme = _read(repo_root / "deploy/tokenmm/README.md")
    telemetry_runbook = _read(repo_root / "deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md")
    design_doc = _read(
        repo_root / "docs/plans/2026-03-09-tokenmm-telemetry-jupyter-go-prod-design.md",
    )

    assert "binds to `127.0.0.1` by default" in api_doc
    assert "localhost/internal deployments" in api_doc

    assert "flux@tokenmm-jupyter.service" in runbook
    assert "fills.sqlite" in runbook
    assert "orders.sqlite" in runbook
    assert "quote_cycles.sqlite" in runbook
    assert "POST http://127.0.0.1:5022/api/pulse/jobs/group/tokenmm/restart" in runbook
    assert ".venv/bin/python ops/scripts/deploy/tokenmm_rollout_preflight.py" in runbook
    assert "http://127.0.0.1:8888/lab" in runbook

    assert "TOKENMM_TELEMETRY_DIR" in deploy_readme
    assert "tokenmm-jupyter.env.example" in deploy_readme
    assert "bootstrap_tokenmm_telemetry_rds.sh" in deploy_readme
    assert "run_tokenmm_telemetry_shipper.sh" in deploy_readme
    assert "tokenmm_telemetry_cutover.py" in deploy_readme
    assert "NAUTILUS_TELEMETRY_PG_SECRET_ID" in deploy_readme
    assert "pnpm --dir fluxboard install --frozen-lockfile" in deploy_readme
    assert ".venv/bin/python ops/scripts/deploy/tokenmm_rollout_preflight.py" in deploy_readme
    assert "pnpm --dir fluxboard build" in deploy_readme
    assert "pnpm --dir pulse-ui install --frozen-lockfile" in deploy_readme
    assert "pnpm --dir pulse-ui build" in deploy_readme
    assert "make build" in deploy_readme
    assert "`TOKENMM_DEPLOY_ROOT`" in deploy_readme
    assert "`TOKENMM_DEPLOY_ROOT`" in runbook
    assert DEPLOY_ROOT_PLACEHOLDER in _read(
        _repo_root() / "deploy/tokenmm/systemd/common.env.example",
    )

    assert "orders.sqlite" in telemetry_runbook
    assert "fills.sqlite" in telemetry_runbook
    assert "quote_cycles.sqlite" in telemetry_runbook
    assert "telemetry" in telemetry_runbook
    assert "psql" in telemetry_runbook
    assert 'export POSTGRES_URL="postgresql://' in telemetry_runbook
    assert "bootstrap_tokenmm_telemetry_rds.sh" in telemetry_runbook
    assert "NAUTILUS_TELEMETRY_PG_SECRET_ID" in telemetry_runbook
    assert "tokenmm_telemetry_cutover.py" in telemetry_runbook

    assert "cutover" in design_doc
    assert "localhost-only JupyterLab" in design_doc
    assert "telemetry shipper" in design_doc


def test_tokenmm_live_config_enables_local_telemetry_persistence_paths() -> None:
    shared_config = _read(_repo_root() / "deploy/tokenmm/tokenmm.live.toml")

    assert "[telemetry_shipper]" in shared_config
    assert "enable_local_persistence = true" in shared_config
    assert "prune_retention_hours = 48" in shared_config
    assert 'fills_db_path = "/var/lib/nautilus/telemetry/tokenmm/fills.sqlite"' in shared_config
    assert 'orders_db_path = "/var/lib/nautilus/telemetry/tokenmm/orders.sqlite"' in shared_config
    assert (
        'quote_cycles_db_path = "/var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite"'
        in shared_config
    )
    assert (
        'portfolio_inventory_db_path = "/var/lib/nautilus/telemetry/tokenmm/portfolio_inventory.sqlite"'
        in shared_config
    )


def test_tokenmm_stack_script_requires_explicit_tokenmm_env_and_never_falls_back_to_makerv3() -> (
    None
):
    script = _read(_repo_root() / "ops/scripts/deploy/tokenmm_stack.sh")

    assert "TOKENMM_* | BYBIT_* | BINANCE_* | OKX_* | BITGET_*" in script
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
    assert 'BITGET_SECRET_ID="${TOKENMM_BITGET_SECRET_ID:-/nautilus/tokenmm/bitget}"' in script
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
    assert "BYBIT_*|BINANCE_*|BITGET_*|OKX_*)" in script[load_secret_idx:]
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
    assert "TOKENMM_BITGET_SECRET_ID=/nautilus/tokenmm/bitget" in env_example
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
    assert "BYBIT_*|BINANCE_*|BITGET_*|OKX_*)" in script
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
    shared_config_paths = [
        root / "deploy/tokenmm/tokenmm.live.toml",
        root / "deploy/tokenmm/strategies/tokenmm.strategy.template.toml",
    ]
    strategy_lookbacks = {
        "plumeusdt_bybit_perp_makerv3": 1440,
        "plumeusdt_bitget_perp_makerv3": 1440,
        "plumeusdt_okx_perp_makerv3": 1440,
    }

    for path in shared_config_paths:
        text = _read(path)
        assert "exec_reconciliation_lookback_mins = 15" in text
        assert "filter_unclaimed_external_orders = true" in text
        assert "filter_position_reports = false" in text

    for strategy_id in TOKENMM_STRATEGY_IDS:
        text = _read(_strategy_config_path(strategy_id))
        expected_lookback = strategy_lookbacks.get(strategy_id, 15)
        assert f"exec_reconciliation_lookback_mins = {expected_lookback}" in text
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
    deploy_readme = _read(repo_root / "deploy/tokenmm/README.md")

    assert "EnvironmentFile=-/etc/flux/common.env" in service_template
    assert "EnvironmentFile=/etc/flux/%i.env" in service_template
    assert "ExecStart=/bin/bash -lc 'cd \"${WORKDIR:?}\" && exec ${CMD}'" in service_template
    assert 'if [ -n "${PORT:-}" ]; then' in service_template
    assert "SyslogIdentifier=flux-%i" in service_template
    assert "RestartPreventExitStatus=78" in service_template
    assert "NoNewPrivileges=true" not in service_template

    assert "[Install]" in target_unit
    assert "WantedBy=multi-user.target" in target_unit
    assert "Wants=flux@tokenmm-api.service" in target_unit
    assert "Wants=flux@tokenmm-portfolio.service" in target_unit
    assert "Wants=flux@tokenmm-bridge.service" in target_unit
    assert "Wants=flux@tokenmm-prometheus.service" in target_unit
    assert "Wants=flux@tokenmm-grafana.service" in target_unit
    assert "Wants=flux@tokenmm-liquidity-exporter.service" in target_unit
    assert "Wants=flux@tokenmm-markouts-exporter.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_bybit_perp_makerv3.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_bybit_spot_makerv3.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_okx_perp_makerv3.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_binance_perp_makerv3.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_binance_spot_makerv3.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_bitget_perp_makerv3.service" in target_unit
    assert "Wants=flux@tokenmm-node-plumeusdt_bitget_spot_makerv3.service" in target_unit

    assert "deploy/tokenmm/tokenmm.live.toml" in install_script
    assert "/etc/flux" in install_script
    assert "/etc/sudoers.d/flux-pulse" in install_script
    assert "strategy_stack_discover_strategy_ids" in install_script
    assert "plumeusdt_bybit_perp_makerv3" not in install_script
    assert (
        "env FLUXBOARD_SERVE_DIST=1 PULSE_SERVE_DIST=1 ${TOKENMM_PYTHON_BIN} -m "
        "nautilus_trader.flux.runners.tokenmm.run_api" in install_script
    )
    assert "--serve-fluxboard" in install_script
    assert "--serve-pulse" in install_script
    assert 'TOKENMM_API_HOST="${TOKENMM_API_HOST:-}"' in install_script
    assert 'read_existing_api_host() {' in install_script
    assert 'api_host="${TOKENMM_API_HOST:-$(read_existing_api_host)}"' in install_script
    assert 'api_host="${api_host:-0.0.0.0}"' in install_script
    assert "--host ${api_host}" in install_script
    assert "tokenmm-portfolio" in install_script
    assert "tokenmm-pulse" not in install_script
    assert 'service_id="tokenmm-node-${strategy_id}"' in install_script
    assert "tokenmm-api" in install_script
    assert "--strategy-id ${strategy_id}" in install_script
    assert "--all-strategies" not in install_script
    assert "run_tokenmm_telemetry_shipper.sh" in install_script
    assert "render_prometheus_env()" in install_script
    assert "render_grafana_env()" in install_script
    assert "render_liquidity_exporter_env()" in install_script
    assert "render_markouts_exporter_env()" in install_script
    assert "install_monitoring_assets()" in install_script
    assert "/etc/tokenmm-monitoring" in install_script
    assert "/usr/local/bin/tokenmm-grafana-run.sh" in install_script
    assert "/usr/local/bin/tokenmm-prometheus-run.sh" in install_script
    assert "tokenmm-telemetry-rds.env.example" in install_script
    assert "flux-tokenmm-telemetry-health.service" in install_script
    assert "flux-tokenmm-telemetry-health.timer" in install_script

    assert "TOKENMM_AWS_REGION=ap-southeast-1" in common_env
    assert "NAUTILUS_TELEMETRY_PG_SECRET_ID=" in common_env
    assert "TOKENMM_REDIS_PASSWORD=" in common_env
    assert "BYBIT_API_KEY=" in common_env
    assert "BINANCE_API_KEY=" in common_env
    assert "BITGET_API_KEY=" in common_env
    assert "BITGET_API_PASSPHRASE=" in common_env
    assert "OKX_API_KEY=" in common_env

    assert "/usr/bin/systemctl start flux@tokenmm-api.service" in sudoers
    assert "/usr/bin/systemctl start flux@tokenmm-bridge.service" in sudoers
    assert "/usr/bin/systemctl start flux@tokenmm-prometheus.service" in sudoers
    assert "/usr/bin/systemctl restart flux@tokenmm-grafana.service" in sudoers
    assert "/usr/bin/systemctl restart flux@tokenmm-liquidity-exporter.service" in sudoers
    assert "/usr/bin/systemctl restart flux@tokenmm-markouts-exporter.service" in sudoers
    assert "/usr/bin/systemctl restart flux@tokenmm-portfolio.service" in sudoers
    assert (
        "/usr/bin/systemctl restart flux@tokenmm-node-plumeusdt_bybit_perp_makerv3.service"
        in sudoers
    )
    assert (
        "/usr/bin/systemctl restart flux@tokenmm-node-plumeusdt_binance_perp_makerv3.service"
        in sudoers
    )
    assert (
        "/usr/bin/systemctl restart flux@tokenmm-node-plumeusdt_bitget_perp_makerv3.service"
        in sudoers
    )
    assert (
        "/usr/bin/systemctl restart flux@tokenmm-node-plumeusdt_bitget_spot_makerv3.service"
        in sudoers
    )
    assert "/usr/bin/journalctl -u flux@tokenmm-api.service" in sudoers
    assert "/usr/bin/journalctl -u flux@tokenmm-grafana.service" in sudoers
    assert "/usr/bin/journalctl -u flux@tokenmm-markouts-exporter.service" in sudoers
    assert "tokenmm-pulse.service" not in sudoers
    assert "flux@*" not in sudoers

    assert "tokenmm-prometheus" in deploy_readme
    assert "tokenmm-grafana" in deploy_readme
    assert "tokenmm-liquidity-exporter" in deploy_readme
    assert "tokenmm-markouts-exporter" in deploy_readme


def test_tokenmm_monitoring_assets_are_repo_managed_and_host_network_compatible() -> None:
    repo_root = _repo_root()
    install_script = _read(repo_root / "ops/scripts/deploy/install_tokenmm_systemd.sh")
    grafana_wrapper = _read(repo_root / "deploy/tokenmm/systemd/tokenmm-grafana-run.sh")
    prometheus_wrapper = _read(repo_root / "deploy/tokenmm/systemd/tokenmm-prometheus-run.sh")
    prometheus_config = _read(repo_root / "deploy/tokenmm/systemd/prometheus.yml")
    grafana_datasources = _read(
        repo_root / "monitoring/grafana/provisioning/datasources/datasources.yml",
    )
    dashboards_doc = _read(repo_root / "monitoring/DASHBOARDS.md")

    assert "install -d /etc/tokenmm-monitoring" not in install_script
    assert "tokenmm-monitoring" in install_script
    assert 'MONITORING_RUNTIME_ROOT="/opt/tokenmm-monitoring"' in install_script
    assert 'GRAFANA_VERSION="10.2.3"' in install_script
    assert 'PROMETHEUS_VERSION="2.48.0"' in install_script
    assert "monitoring/grafana/dashboards" in install_script
    assert "monitoring/grafana/provisioning/dashboards/dashboards.yml" in install_script
    assert "monitoring/grafana/provisioning/datasources/datasources.yml" in install_script
    assert "deploy/tokenmm/systemd/prometheus.yml" in install_script
    assert "install_monitoring_binaries()" in install_script
    assert "curl -fsSL" in install_script
    assert "trap 'rm -rf -- \"${tmpdir}\"' EXIT" in install_script
    assert "dl.grafana.com/oss/release/grafana-" in install_script
    assert "github.com/prometheus/prometheus/releases/download/v" in install_script

    assert "docker run" not in grafana_wrapper
    assert "docker rm -f tokenmm-grafana" not in grafana_wrapper
    assert 'TOKENMM_GRAFANA_INSTALL_DIR:=/opt/tokenmm-monitoring/grafana/current' in grafana_wrapper
    assert 'GF_PATHS_DATA="${TOKENMM_GRAFANA_DATA_DIR}"' in grafana_wrapper
    assert 'GF_PATHS_PROVISIONING="${TOKENMM_GRAFANA_CONFIG_DIR}/provisioning"' in grafana_wrapper
    assert '"${TOKENMM_GRAFANA_INSTALL_DIR}/bin/grafana-server"' in grafana_wrapper
    assert '--homepath "${TOKENMM_GRAFANA_INSTALL_DIR}"' in grafana_wrapper

    assert "docker run" not in prometheus_wrapper
    assert "docker rm -f tokenmm-prometheus" not in prometheus_wrapper
    assert 'TOKENMM_PROMETHEUS_INSTALL_DIR:=/opt/tokenmm-monitoring/prometheus/current' in prometheus_wrapper
    assert '"${TOKENMM_PROMETHEUS_INSTALL_DIR}/prometheus"' in prometheus_wrapper
    assert '--config.file="${TOKENMM_PROMETHEUS_CONFIG_DIR}/prometheus.yml"' in prometheus_wrapper
    assert '--storage.tsdb.path="${TOKENMM_PROMETHEUS_DATA_DIR}"' in prometheus_wrapper
    assert "--web.enable-lifecycle" in prometheus_wrapper

    assert 'targets: ["127.0.0.1:9108"]' in prometheus_config
    assert 'targets: ["127.0.0.1:9109"]' in prometheus_config
    assert 'targets: ["127.0.0.1:9090"]' in prometheus_config

    assert "url: http://127.0.0.1:9090" in grafana_datasources
    assert "url: http://prometheus:9090" not in grafana_datasources

    assert "/etc/tokenmm-monitoring/grafana/dashboards" in dashboards_doc
    assert "/etc/tokenmm-monitoring/prometheus/prometheus.yml" in dashboards_doc
    assert "/opt/tokenmm-monitoring/grafana/current" in dashboards_doc
    assert "/opt/tokenmm-monitoring/prometheus/current" in dashboards_doc
    assert "/var/lib/grafana/dashboards" not in dashboards_doc


def test_tokenmm_shared_account_live_configs_enable_reconciliation_filters_with_bounded_lookback() -> (
    None
):
    repo_root = _repo_root()
    shared_config_paths = [
        repo_root / "deploy/tokenmm/tokenmm.live.toml",
        repo_root / "deploy/tokenmm/strategies/tokenmm.strategy.template.toml",
    ]
    strategy_lookbacks = {
        "plumeusdt_bybit_perp_makerv3": 1440,
        "plumeusdt_bitget_perp_makerv3": 1440,
        "plumeusdt_okx_perp_makerv3": 1440,
    }

    for path in shared_config_paths:
        config = _read(path)
        assert "exec_reconciliation_lookback_mins = 15" in config
        assert "filter_unclaimed_external_orders = true" in config
        assert "filter_position_reports = false" in config

    for strategy_id in TOKENMM_STRATEGY_IDS:
        config = _read(_strategy_config_path(strategy_id))
        expected_lookback = strategy_lookbacks.get(strategy_id, 15)
        assert f"exec_reconciliation_lookback_mins = {expected_lookback}" in config
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


def test_tokenmm_live_configs_explicitly_set_generate_missing_orders_policy() -> None:
    repo_root = _repo_root()
    expected_flags = {
        repo_root / "deploy/tokenmm/tokenmm.live.toml": False,
        repo_root / "deploy/tokenmm/strategies/tokenmm.strategy.template.toml": False,
        _strategy_config_path("plumeusdt_bybit_perp_makerv3"): True,
        _strategy_config_path("plumeusdt_okx_perp_makerv3"): True,
        _strategy_config_path("plumeusdt_bitget_perp_makerv3"): True,
        _strategy_config_path("plumeusdt_bybit_spot_makerv3"): False,
        _strategy_config_path("plumeusdt_bitget_spot_makerv3"): False,
        _strategy_config_path("plumeusdt_binance_spot_makerv3"): False,
        _strategy_config_path("plumeusdt_binance_perp_makerv3"): False,
    }

    for path, expected_enabled in expected_flags.items():
        config = _read(path)
        expected_line = (
            "exec_generate_missing_orders = true"
            if expected_enabled
            else "exec_generate_missing_orders = false"
        )
        assert expected_line in config


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
    assert 'host = "127.0.0.1"' in config


def test_tokenmm_api_contract_catalog_lists_distinct_plume_spot_and_perp_instruments() -> None:
    config = _read(_repo_root() / "deploy/tokenmm/tokenmm.live.toml")

    assert 'instrument_id = "PLUMEUSDT-LINEAR.BYBIT"' in config
    assert 'instrument_id = "PLUMEUSDT-SPOT.BYBIT"' in config
    assert 'instrument_id = "PLUMEUSDT-PERP.BINANCE_PERP"' in config
    assert 'instrument_id = "PLUMEUSDT.BINANCE_SPOT"' in config
    assert 'instrument_id = "PLUMEUSDT-PERP.BITGET"' in config
    assert 'instrument_id = "PLUMEUSDT.BITGET"' in config
    assert 'instrument_id = "PLUME-USDT-SWAP.OKX"' in config


def test_tokenmm_telemetry_shipper_paths_parse_as_top_level_config() -> None:
    config = tomllib.load((_repo_root() / "deploy/tokenmm/tokenmm.live.toml").open("rb"))
    shipper = config["telemetry_shipper"]
    benchmarks = shipper["markout_benchmarks"]

    assert shipper["balance_snapshots_db_path"] == (
        "/var/lib/nautilus/telemetry/tokenmm/balance_snapshots.sqlite"
    )
    assert shipper["portfolio_inventory_db_path"] == (
        "/var/lib/nautilus/telemetry/tokenmm/portfolio_inventory.sqlite"
    )
    assert shipper["state_db_path"] == "/var/lib/nautilus/telemetry/tokenmm/shipper_state.sqlite"
    assert shipper["poll_interval_ms"] == 1000
    assert shipper["max_batch_size"] == 500
    assert shipper["prune_retention_hours"] == 48
    assert benchmarks[0] == {
        "benchmark_name": "fv_market_mid",
        "benchmark_field": "fv",
    }
    assert benchmarks[1] == {
        "benchmark_name": "local_mkt_mid",
        "benchmark_field": "maker_mid",
    }


def test_tokenmm_deploy_readme_describes_seven_node_topology() -> None:
    readme = _read(_repo_root() / "deploy/tokenmm/README.md")

    assert "production deployment root for the TokenMM stack" in readme
    assert "current 7-node PLUME TokenMM stack" not in readme
    assert "- `plumeusdt_binance_perp_makerv3`" in readme
    assert "- `plumeusdt_bitget_perp_makerv3`" in readme
    assert "- `plumeusdt_bitget_spot_makerv3`" in readme
    assert "All seven allowlisted strategies price off Binance spot" in readme
    assert "and the 7 allowlisted" in readme
    assert "allowlist" in readme
    assert "`params` returns the 7 allowlisted strategy IDs" in readme
    assert "`signal` returns seven per-strategy rows." in readme
    assert "Supported live core for this pass" in readme
    assert "Binance perp and Binance spot stay allowlisted but parked" in readme
    assert "Shared portfolio completeness requires only the supported live core" in readme


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
        "plumeusdt_bitget_perp_makerv3": (
            'execution_venue = "BITGET"',
            'reference_venue = "BINANCE_SPOT"',
            'instrument_id = "PLUMEUSDT-PERP.BITGET"',
            'instrument_id = "PLUMEUSDT.BINANCE_SPOT"',
            'adapter = "bitget"',
            'api_passphrase_env = "BITGET_API_PASSPHRASE"',
        ),
        "plumeusdt_bitget_spot_makerv3": (
            'execution_venue = "BITGET"',
            'reference_venue = "BINANCE_SPOT"',
            'instrument_id = "PLUMEUSDT.BITGET"',
            'instrument_id = "PLUMEUSDT.BINANCE_SPOT"',
            'adapter = "bitget"',
            'api_passphrase_env = "BITGET_API_PASSPHRASE"',
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


def test_supported_tokenmm_perp_configs_pin_bounded_convergence_budgets() -> None:
    expectations = {
        "plumeusdt_bybit_perp_makerv3": (
            "max_cancels_per_side_per_cycle = 1",
            "max_places_per_side_per_cycle = 2",
            "max_total_actions_per_cycle = 4",
            "max_pending_cancels_per_side = 1",
        ),
        "plumeusdt_okx_perp_makerv3": (
            "max_cancels_per_side_per_cycle = 2",
            "max_places_per_side_per_cycle = 2",
            "max_total_actions_per_cycle = 6",
            "max_pending_cancels_per_side = 2",
        ),
        "plumeusdt_bitget_perp_makerv3": (
            "max_cancels_per_side_per_cycle = 1",
            "max_places_per_side_per_cycle = 2",
            "max_total_actions_per_cycle = 4",
            "max_pending_cancels_per_side = 1",
        ),
    }

    for strategy_id, required_lines in expectations.items():
        config = _read(_strategy_config_path(strategy_id))
        for line in required_lines:
            assert line in config


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
    assert "Wider lookbacks are allowed only as explicit per-strategy recovery overrides" in strategies_readme
    assert "filter_unclaimed_external_orders = true" in strategies_readme
    assert "filter_position_reports = false" in strategies_readme
    assert "flux.runners.tokenmm.run_node" in strategies_readme
    assert "/etc/flux/tokenmm-node-" in strategies_readme
    assert "deploy/makerv3" not in strategies_readme
    assert "runners.makerv3" not in strategies_readme

    assert "Production deploy docs default to paper/no-exec smoke first." in examples_readme
    assert "`deploy/tokenmm/README.md`" in examples_readme
    assert "Production management lives behind Pulse at `/pulse`." in examples_readme
    assert "flux.runners.tokenmm.run_portfolio" in examples_readme
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


def test_fluxboard_prod_docs_use_shared_static_build_base() -> None:
    repo_root = _repo_root()
    env_example = _read(repo_root / "fluxboard/.env.example")
    runbook = _read(repo_root / "fluxboard/docs/tokenmm_runbook.md")

    assert "FLUXBOARD_BASE_PATH=" not in env_example
    assert "Production builds pin shared-static assets to `/static/fluxboard/*`" in env_example
    assert "FLUXBOARD_BASE_PATH=" not in runbook
    assert "Production builds pin Fluxboard assets to the shared `/static/fluxboard/*` prefix" in runbook


def test_tokenmm_docs_define_base_first_operator_trade_quantity_contract() -> None:
    repo_root = _repo_root()
    rest_contract = _read(repo_root / "fluxboard/docs/tokenmm_contract.md")
    socket_contract = _read(repo_root / "fluxboard/docs/tokenmm_socket_contract.md")

    assert "For TokenMM-facing REST rows, `qty` is operator-facing base quantity." in rest_contract
    assert "Shared producer bare `qty` remains venue/native size; `qty_base` and `qty_venue` carry the normalized pair." in rest_contract
    assert "Older raw SQLite rows may not have `*_base` / `*_venue` columns." in rest_contract
    assert "Rollout requires a TokenMM trade-stream cutover/reset before enabling base-first `qty` in production." in rest_contract
    assert '"qty_base": 0.015' in rest_contract
    assert '"qty_venue": 0.015' in rest_contract
    assert '"qty_conversion_status": "identity"' in rest_contract
    assert '"qty_conversion_source": "generic:multiplier=1"' in rest_contract

    assert "For TokenMM `trade_update` payloads, `trade.qty` is operator-facing base quantity." in socket_contract
    assert "Legacy Redis trade rows cannot be safely reinterpreted without producer-supplied normalized fields." in socket_contract
    assert "Rollout requires a TokenMM trade-stream cutover/reset before enabling the base-first socket projection." in socket_contract
    assert "GET /api/v1/trades/delta?profile=tokenmm&since_seq=<last_seq>` when a usable `last_seq` was persisted" in socket_contract
    assert "GET /api/v1/trades/delta?profile=tokenmm&after=max(0,last_trade_ts_ms-1)` only when no usable `last_seq` is available" in socket_contract
    assert "Bare `qty` in trade payloads remains venue/native size unless a paired explicit base field is also present." not in socket_contract
