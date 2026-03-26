from __future__ import annotations

from argparse import Namespace
from pathlib import Path
import tomllib

import pytest
from flask import Flask

from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.runners.equities.run_api import _attach_fluxboard_equities_routes
from flux.runners.equities.run_api import _build_strategy_alerts_resolver
from flux.runners.equities.run_api import _build_contract_catalog
from flux.runners.equities.run_api import _build_contract_catalog_by_strategy
from flux.runners.equities.run_api import _build_profile_strategy_maps
from flux.runners.equities.run_api import _build_strategy_running_resolver
from flux.runners.equities.run_api import _equities_profile_summary
from flux.runners.equities.run_api import _load_config
from flux.runners.equities.run_api import _parse_args
from flux.runners.equities.run_api import _resolve_runtime_params_payloads
from flux.runners.equities.run_api import _resolve_strategy_name
from flux.runners.equities.run_api import build_equities_strategy_metadata_map
from flux.runners.equities.run_api import build_strategy_metadata_for_test

def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _load_toml(path: Path) -> dict:
    return tomllib.load(path.open("rb"))


def test_build_profile_strategy_maps_reads_equities_allowlist_and_required_subset() -> None:
    strategy_map, required_map = _build_profile_strategy_maps(
        {
            "equities_strategy_ids": ["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"],
            "equities_required_strategy_ids": ["aapl_tradexyz_makerv4"],
        },
    )

    assert strategy_map == {
        "equities": ["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"],
    }
    assert required_map == {"equities": ["aapl_tradexyz_makerv4"]}


def test_build_profile_strategy_maps_reads_core_prod_allowlist_from_shared_live_config() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    expected_strategy_ids = list(config["api"]["equities_strategy_ids"])
    expected_required_strategy_ids = list(config["api"]["equities_required_strategy_ids"])

    strategy_map, required_map = _build_profile_strategy_maps(config["api"])

    assert strategy_map == {"equities": expected_strategy_ids}
    assert required_map == {"equities": expected_required_strategy_ids}


def test_equities_descriptor_exposes_stable_profile_contract() -> None:
    descriptor = get_strategy_set_descriptor("equities")

    assert descriptor.profile == "equities"
    assert descriptor.aliases == ("equities",)
    assert descriptor.base_path == "/equities"
    assert descriptor.route_aliases == ()
    assert descriptor.strategy_ids_field == "equities_strategy_ids"
    assert descriptor.required_strategy_ids_field == "equities_required_strategy_ids"


def test_build_profile_strategy_maps_requires_non_empty_equities_allowlist() -> None:
    with pytest.raises(ValueError, match="non-empty"):
        _build_profile_strategy_maps({})


def test_build_profile_strategy_maps_rejects_required_ids_outside_allowlist() -> None:
    with pytest.raises(ValueError, match="subset"):
        _build_profile_strategy_maps(
            {
                "equities_strategy_ids": ["aapl_tradexyz_makerv4"],
                "equities_required_strategy_ids": ["msft_tradexyz_makerv4"],
            },
        )


def test_equities_profile_summary_reports_effective_strategy_sets() -> None:
    summary = _equities_profile_summary(
        {"equities": ["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"]},
        {"equities": ["aapl_tradexyz_makerv4"]},
    )

    assert "equities_strategy_count=2" in summary
    assert "equities_strategy_ids=['aapl_tradexyz_makerv4', 'msft_tradexyz_makerv4']" in summary
    assert "equities_required_strategy_ids=['aapl_tradexyz_makerv4']" in summary


def test_equities_run_api_uses_makerv4_metadata_when_strategy_spec_is_makerv4() -> None:
    metadata = build_strategy_metadata_for_test("makerv4")

    assert metadata.strategy_class == "maker_v4"
    assert metadata.param_set == "makerv4"
    assert metadata.strategy_family == "maker_v4"
    assert metadata.strategy_version == "v4"


def test_equities_run_api_can_publish_per_strategy_family_metadata() -> None:
    metadata = build_equities_strategy_metadata_map(
        {
            "strategy_groups": "equities",
            "quote_asset": "USD",
            "strategy_contracts": [
                {
                    "strategy_id": "aapl_tradexyz_makerv3",
                    "portfolio_asset_id": "AAPL",
                    "maker_venue": "HYPERLIQUID",
                    "maker_symbol": "AAPL",
                    "market_type": "perp",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
                {
                    "strategy_id": "aapl_tradexyz_makerv4",
                    "portfolio_asset_id": "AAPL",
                    "maker_venue": "HYPERLIQUID",
                    "maker_symbol": "AAPL",
                    "market_type": "perp",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
            ],
        },
        strategy_ids=["aapl_tradexyz_makerv3", "aapl_tradexyz_makerv4"],
    )

    assert metadata["aapl_tradexyz_makerv3"].base_asset == "AAPL"
    assert metadata["aapl_tradexyz_makerv3"].strategy_family == "maker_v3"
    assert metadata["aapl_tradexyz_makerv4"].base_asset == "AAPL"
    assert metadata["aapl_tradexyz_makerv4"].strategy_family == "maker_v4"


def test_equities_run_api_keeps_same_stock_multivenue_routes_distinct_in_metadata() -> None:
    metadata = build_equities_strategy_metadata_map(
        {
            "strategy_groups": "equities",
            "quote_asset": "USD",
            "strategy_contracts": [
                {
                    "strategy_id": "pltr_tradexyz_makerv4",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "HYPERLIQUID",
                    "maker_symbol": "PLTR",
                    "market_type": "perp",
                    "maker_instrument_id": "xyz:PLTR-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
                {
                    "strategy_id": "pltr_binance_perp_makerv4",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "BINANCE_PERP",
                    "maker_symbol": "PLTRUSDT",
                    "market_type": "perp",
                    "maker_instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "binance.futures.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
            ],
        },
        strategy_ids=["pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"],
    )

    assert set(metadata) == {"pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"}
    assert metadata["pltr_tradexyz_makerv4"].base_asset == "PLTR"
    assert metadata["pltr_binance_perp_makerv4"].base_asset == "PLTR"
    assert metadata["pltr_tradexyz_makerv4"].strategy_family == "maker_v4"
    assert metadata["pltr_binance_perp_makerv4"].strategy_family == "maker_v4"


def test_equities_run_api_builds_per_strategy_contract_catalog() -> None:
    config = {
        "contracts": [
            {
                "exchange": "hyperliquid",
                "symbol": "AAPL/USD",
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
            },
            {
                "exchange": "ibkr",
                "symbol": "AAPL/USD",
                "instrument_id": "AAPL.NASDAQ",
            },
            {
                "exchange": "hyperliquid",
                "symbol": "AMD/USD",
                "instrument_id": "xyz:AMD-USD-PERP.HYPERLIQUID",
            },
            {
                "exchange": "ibkr",
                "symbol": "AMD/USD",
                "instrument_id": "AMD.NASDAQ",
            },
        ],
    }
    api_cfg = {
        "strategy_contracts": [
            {
                "strategy_id": "aapl_tradexyz_makerv4",
                "portfolio_asset_id": "AAPL",
                "maker_venue": "HYPERLIQUID",
                "maker_symbol": "AAPL",
                "market_type": "perp",
                "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": "AAPL.NASDAQ",
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
            },
            {
                "strategy_id": "amd_tradexyz_makerv4",
                "portfolio_asset_id": "AMD",
                "maker_venue": "HYPERLIQUID",
                "maker_symbol": "AMD",
                "market_type": "perp",
                "maker_instrument_id": "xyz:AMD-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": "AMD.NASDAQ",
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
            },
        ],
    }

    contracts = _build_contract_catalog(config)
    contracts_by_strategy = _build_contract_catalog_by_strategy(
        api_cfg,
        contract_catalog=contracts,
    )

    assert [entry.instrument_id for entry in contracts_by_strategy["aapl_tradexyz_makerv4"]] == [
        "XYZ:AAPL-USD-PERP.HYPERLIQUID",
        "AAPL.NASDAQ",
    ]
    assert [entry.instrument_id for entry in contracts_by_strategy["amd_tradexyz_makerv4"]] == [
        "XYZ:AMD-USD-PERP.HYPERLIQUID",
        "AMD.NASDAQ",
    ]


def test_equities_run_api_keeps_multivenue_routes_separate_in_contract_catalog() -> None:
    config = {
        "contracts": [
            {
                "exchange": "hyperliquid",
                "symbol": "PLTR/USD",
                "instrument_id": "xyz:PLTR-USD-PERP.HYPERLIQUID",
            },
            {
                "exchange": "binance_perp",
                "symbol": "PLTR/USDT",
                "instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
            },
            {
                "exchange": "ibkr",
                "symbol": "PLTR/USD",
                "instrument_id": "PLTR.NASDAQ",
            },
        ],
    }
    api_cfg = {
        "strategy_contracts": [
            {
                "strategy_id": "pltr_tradexyz_makerv4",
                "portfolio_asset_id": "PLTR",
                "maker_venue": "HYPERLIQUID",
                "maker_symbol": "PLTR",
                "market_type": "perp",
                "maker_instrument_id": "xyz:PLTR-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": "PLTR.NASDAQ",
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
            },
            {
                "strategy_id": "pltr_binance_perp_makerv4",
                "portfolio_asset_id": "PLTR",
                "maker_venue": "BINANCE_PERP",
                "maker_symbol": "PLTRUSDT",
                "market_type": "perp",
                "maker_instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                "reference_instrument_id": "PLTR.NASDAQ",
                "execution_account_scope_id": "binance.futures.main",
                "reference_account_scope_id": "ibkr.reference.main",
            },
        ],
    }

    contracts = _build_contract_catalog(config)
    contracts_by_strategy = _build_contract_catalog_by_strategy(
        api_cfg,
        contract_catalog=contracts,
    )

    assert [entry.instrument_id for entry in contracts_by_strategy["pltr_tradexyz_makerv4"]] == [
        "XYZ:PLTR-USD-PERP.HYPERLIQUID",
        "PLTR.NASDAQ",
    ]
    assert [
        entry.instrument_id for entry in contracts_by_strategy["pltr_binance_perp_makerv4"]
    ] == [
        "PLTRUSDT-PERP.BINANCE_PERP",
        "PLTR.NASDAQ",
    ]


def test_equities_run_api_defaults_makerv3_qty_to_one() -> None:
    schema, defaults = _resolve_runtime_params_payloads("makerv3")

    assert schema["qty"]["type"] == "number"
    assert defaults["qty"] == pytest.approx(1.0)


def test_build_strategy_running_resolver_maps_pulse_status_to_equities_strategy_ids() -> None:
    class _FakePulse:
        def __init__(self) -> None:
            self.calls: list[str] = []

        def get_job_status(self, job_id: str) -> str:
            self.calls.append(job_id)
            return {
                "equities-node-aapl_tradexyz_makerv4": "active",
                "equities-node-meta_tradexyz_makerv4": "failed",
            }.get(job_id, "unknown")

    pulse = _FakePulse()
    resolver = _build_strategy_running_resolver(pulse_control=pulse, cache_ttl_s=60.0)

    running = resolver(["aapl_tradexyz_makerv4", "meta_tradexyz_makerv4"])

    assert running == {
        "aapl_tradexyz_makerv4": True,
        "meta_tradexyz_makerv4": False,
    }
    assert pulse.calls == [
        "equities-node-aapl_tradexyz_makerv4",
        "equities-node-meta_tradexyz_makerv4",
    ]


def test_build_strategy_alerts_resolver_surfaces_pulse_failures_but_ignores_active_error_logs() -> None:
    class _FakePulse:
        def __init__(self) -> None:
            self.calls: list[str] = []

        def get_job_snapshot(self, job_id: str) -> dict[str, object] | None:
            self.calls.append(job_id)
            return {
                "equities-node-aapl_tradexyz_makerv4": {
                    "id": job_id,
                    "status": "failed",
                    "errors": {
                        "count": 2,
                        "last_seen": "2026-03-19T11:43:02Z",
                        "preview": "Invalid API-key, IP, or permissions for action",
                    },
                },
                "equities-node-meta_tradexyz_makerv4": {
                    "id": job_id,
                    "status": "active",
                    "errors": {
                        "count": 1,
                        "last_seen": "2026-03-19T11:43:10Z",
                        "preview": "Binance API key does not have trading permissions",
                    },
                },
            }.get(job_id)

    pulse = _FakePulse()
    resolver = _build_strategy_alerts_resolver(
        pulse_control=pulse,
        cache_ttl_s=60.0,
        now_ms_fn=lambda: 1_763_289_790_000,
    )

    rows_by_strategy = resolver(
        [
            "aapl_tradexyz_makerv4",
            "meta_tradexyz_makerv4",
            "missing_tradexyz_makerv4",
        ],
    )

    assert pulse.calls == [
        "equities-node-aapl_tradexyz_makerv4",
        "equities-node-meta_tradexyz_makerv4",
        "equities-node-missing_tradexyz_makerv4",
    ]
    assert rows_by_strategy["aapl_tradexyz_makerv4"] == [
        {
            "strategy_id": "aapl_tradexyz_makerv4",
            "row_id": "pulse:aapl_tradexyz_makerv4:pulse_job_failed:1773920582000",
            "id": "pulse:aapl_tradexyz_makerv4:pulse_job_failed:1773920582000",
            "level": "CRITICAL",
            "message": "Pulse runner failed: Invalid API-key, IP, or permissions for action",
            "alert_key": "pulse_job_failed",
            "ts_ms": 1_773_920_582_000,
            "source": "pulse",
            "job_id": "equities-node-aapl_tradexyz_makerv4",
            "status": "failed",
            "error_preview": "Invalid API-key, IP, or permissions for action",
            "error_count": 2,
            "last_seen": "2026-03-19T11:43:02Z",
        },
    ]
    assert rows_by_strategy["meta_tradexyz_makerv4"] == []
    assert rows_by_strategy["missing_tradexyz_makerv4"] == [
        {
            "strategy_id": "missing_tradexyz_makerv4",
            "row_id": "pulse:missing_tradexyz_makerv4:pulse_job_unknown:1763289790000",
            "id": "pulse:missing_tradexyz_makerv4:pulse_job_unknown:1763289790000",
            "level": "ERROR",
            "message": "Pulse runner is not registered",
            "alert_key": "pulse_job_unknown",
            "ts_ms": 1_763_289_790_000,
            "source": "pulse",
            "job_id": "equities-node-missing_tradexyz_makerv4",
            "status": "unknown",
            "error_preview": None,
            "error_count": 0,
            "last_seen": None,
        },
    ]


def test_parse_args_requires_explicit_config(monkeypatch) -> None:
    monkeypatch.setattr("sys.argv", ["run_api.py"])

    with pytest.raises(SystemExit, match="2"):
        _parse_args()


def test_parse_args_describes_shared_fluxboard_static_contract(monkeypatch, capsys) -> None:
    monkeypatch.setattr("sys.argv", ["run_api.py", "--help"])

    with pytest.raises(SystemExit, match="0"):
        _parse_args()

    help_text = " ".join(capsys.readouterr().out.split())

    assert "Serve built Fluxboard static assets at /static/fluxboard/*" in help_text
    assert "/equities with SPA fallback" in help_text
    assert "/equities/*" not in help_text


def test_attach_fluxboard_equities_routes_serves_shared_static_assets_and_spa_fallback(
    tmp_path: Path,
) -> None:
    dist_dir = tmp_path / "dist"
    assets_dir = dist_dir / "assets"
    assets_dir.mkdir(parents=True)
    (dist_dir / "index.html").write_text("<html>equities</html>", encoding="utf-8")
    (assets_dir / "app.js").write_text("console.log(equities)", encoding="utf-8")

    app = Flask(__name__)
    _attach_fluxboard_equities_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    response = client.get("/equities")
    assert response.status_code == 200
    assert "equities" in response.get_data(as_text=True)

    response = client.get("/static/fluxboard/assets/app.js")
    assert response.status_code == 200
    assert "console.log(equities)" in response.get_data(as_text=True)

    response = client.get("/equities/assets/app.js")
    assert response.status_code == 404

    response = client.get("/equities/signals/aapl")
    assert response.status_code == 200
    assert "equities" in response.get_data(as_text=True)


def test_attach_fluxboard_equities_routes_keeps_dist_root_files_off_spa_path(
    tmp_path: Path,
) -> None:
    dist_dir = tmp_path / "dist"
    dist_dir.mkdir(parents=True)
    (dist_dir / "index.html").write_text("<html>equities</html>", encoding="utf-8")
    (dist_dir / "favicon.svg").write_text("<svg>shared-icon</svg>", encoding="utf-8")

    app = Flask(__name__)
    _attach_fluxboard_equities_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    response = client.get("/static/fluxboard/favicon.svg")
    assert response.status_code == 200
    assert response.get_data(as_text=True) == "<svg>shared-icon</svg>"

    response = client.get("/equities/favicon.svg")
    assert response.status_code == 200
    assert response.get_data(as_text=True) == "<html>equities</html>"


def test_attach_fluxboard_equities_routes_does_not_answer_tokenm_alias(tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    dist_dir.mkdir(parents=True)
    (dist_dir / "index.html").write_text("<html>equities</html>", encoding="utf-8")

    app = Flask(__name__)
    _attach_fluxboard_equities_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    response = client.get("/tokenm")

    assert response.status_code == 404


def test_resolve_strategy_name_requires_explicit_supported_strategy_class_for_equities() -> None:
    with pytest.raises(ValueError, match="api.strategy_class"):
        _resolve_strategy_name({})

    with pytest.raises(ValueError, match="api.strategy_class"):
        _resolve_strategy_name({"strategy_class": "maker_v9"})


def test_resolve_strategy_name_rejects_param_set_drift() -> None:
    with pytest.raises(ValueError, match="api.param_set"):
        _resolve_strategy_name(
            {
                "strategy_class": "maker_v4",
                "param_set": "makerv3",
            }
        )


def test_load_config_applies_redis_env_overrides(monkeypatch, tmp_path: Path) -> None:
    config_path = tmp_path / "equities.toml"
    config_path.write_text(
        """
[redis]
host = "127.0.0.1"
port = 6380
db = 0
ssl = false
""".strip(),
        encoding="utf-8",
    )
    monkeypatch.setenv("EQUITIES_REDIS_HOST", "redis.internal")
    monkeypatch.setenv("EQUITIES_REDIS_PORT", "6379")
    monkeypatch.setenv("EQUITIES_REDIS_USERNAME", "default")
    monkeypatch.setenv("EQUITIES_REDIS_PASSWORD", "secret")
    monkeypatch.setenv("EQUITIES_REDIS_SSL", "true")

    config = _load_config(config_path)

    assert config["redis"]["host"] == "redis.internal"
    assert config["redis"]["port"] == 6379
    assert config["redis"]["username"] == "default"
    assert config["redis"]["password"] == "secret"
    assert config["redis"]["ssl"] is True
