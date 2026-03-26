from __future__ import annotations

from argparse import Namespace
from pathlib import Path
import tomllib

import pytest
from flask import Flask

import flux.runners.equities.run_api as run_api
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.runners.equities.run_api import _attach_fluxboard_equities_routes
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

LIVE_ENROLLED_ROUTE_IDS = (
    "aapl_tradexyz",
    "amd_tradexyz",
    "amzn_tradexyz",
    "googl_tradexyz",
    "meta_tradexyz",
    "msft_tradexyz",
    "nvda_tradexyz",
    "orcl_tradexyz",
    "pltr_tradexyz",
    "tsla_tradexyz",
)
LIVE_ENROLLED_STRATEGY_IDS = tuple(
    f"{route_id}_{variant}"
    for route_id in LIVE_ENROLLED_ROUTE_IDS
    for variant in ("maker", "taker")
)


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _load_toml(path: Path) -> dict:
    return tomllib.load(path.open("rb"))


def test_build_profile_strategy_maps_reads_equities_allowlist_and_required_subset() -> None:
    strategy_map, required_map = _build_profile_strategy_maps(
        {
            "equities_strategy_ids": ["aapl_tradexyz_maker", "aapl_tradexyz_taker"],
            "equities_required_strategy_ids": ["aapl_tradexyz_maker"],
        },
    )

    assert strategy_map == {
        "equities": ["aapl_tradexyz_maker", "aapl_tradexyz_taker"],
    }
    assert required_map == {"equities": ["aapl_tradexyz_maker"]}


def test_build_profile_strategy_maps_reads_core_prod_allowlist_from_shared_live_config() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")

    strategy_map, required_map = _build_profile_strategy_maps(config["api"])

    assert strategy_map == {"equities": list(LIVE_ENROLLED_STRATEGY_IDS)}
    assert required_map == {"equities": list(LIVE_ENROLLED_STRATEGY_IDS)}


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
                "equities_strategy_ids": ["aapl_tradexyz_maker"],
                "equities_required_strategy_ids": ["aapl_tradexyz_taker"],
            },
        )


def test_equities_profile_summary_reports_effective_strategy_sets() -> None:
    summary = _equities_profile_summary(
        {"equities": ["aapl_tradexyz_maker", "aapl_tradexyz_taker"]},
        {"equities": ["aapl_tradexyz_maker"]},
    )

    assert "equities_strategy_count=2" in summary
    assert "equities_strategy_ids=['aapl_tradexyz_maker', 'aapl_tradexyz_taker']" in summary
    assert "equities_required_strategy_ids=['aapl_tradexyz_maker']" in summary


def test_equities_run_api_uses_makerv4_metadata_when_strategy_spec_is_makerv4() -> None:
    metadata = build_strategy_metadata_for_test("makerv4")

    assert metadata.strategy_class == "maker_v4"
    assert metadata.param_set == "makerv4"
    assert metadata.strategy_family == "maker_v4"
    assert metadata.strategy_version == "v4"
    assert metadata.as_payload(strategy_id="aapl_tradexyz_makerv4")["deprecated"] is True


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
    assert metadata["aapl_tradexyz_makerv4"].as_payload(
        strategy_id="aapl_tradexyz_makerv4",
    )["replacement"] == "equities_maker/equities_taker"


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


def test_resolve_strategy_name_accepts_split_equities_defaults() -> None:
    assert _resolve_strategy_name(
        {
            "strategy_class": "equities_maker",
            "param_set": "equities_maker",
        }
    ) == "equities_maker"


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


def test_main_binds_per_strategy_metadata_from_root_strategy_contracts(monkeypatch) -> None:
    captured: dict[str, object] = {}
    config = {
        "flux": {"mode": "paper", "namespace": "flux", "schema_version": "v1"},
        "identity": {
            "strategy_id": "equities_api",
            "strategy_instance_id": "equities_api",
            "trader_id": "EQUITIES-API-001",
            "external_strategy_id": "equities_api",
        },
        "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
        "venues": {
            "execution_venue": "HYPERLIQUID",
            "reference_venue": "IBKR",
            "execution_symbol": "STOCKS",
            "reference_symbol": "STOCKS/USD",
        },
        "api": {
            "host": "127.0.0.1",
            "port": 5022,
            "strategy_class": "equities_maker",
            "param_set": "equities_maker",
            "strategy_groups": "equities",
            "base_asset": "STOCKS",
            "quote_asset": "USD",
            "equities_strategy_ids": [
                "aapl_tradexyz_maker",
                "aapl_tradexyz_taker",
                "amzn_binance_perp_maker",
                "amzn_binance_perp_taker",
            ],
        },
        "strategy_contracts": [
            {
                "strategy_id": "aapl_tradexyz_maker",
                "portfolio_asset_id": "AAPL",
                "maker_venue": "HYPERLIQUID",
                "maker_symbol": "AAPL",
                "market_type": "perp",
                "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": "AAPL.NASDAQ",
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
            {
                "strategy_id": "aapl_tradexyz_taker",
                "portfolio_asset_id": "AAPL",
                "maker_venue": "HYPERLIQUID",
                "maker_symbol": "AAPL",
                "market_type": "perp",
                "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": "AAPL.NASDAQ",
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
            {
                "strategy_id": "amzn_binance_perp_maker",
                "portfolio_asset_id": "AMZN",
                "maker_venue": "BINANCE_PERP",
                "maker_symbol": "AMZNUSDT",
                "market_type": "perp",
                "maker_instrument_id": "AMZNUSDT-PERP.BINANCE_PERP",
                "reference_instrument_id": "AMZN.NASDAQ",
                "execution_account_scope_id": "binance.futures.main",
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
            {
                "strategy_id": "amzn_binance_perp_taker",
                "portfolio_asset_id": "AMZN",
                "maker_venue": "BINANCE_PERP",
                "maker_symbol": "AMZNUSDT",
                "market_type": "perp",
                "maker_instrument_id": "AMZNUSDT-PERP.BINANCE_PERP",
                "reference_instrument_id": "AMZN.NASDAQ",
                "execution_account_scope_id": "binance.futures.main",
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
        ],
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
                "exchange": "binance_perp",
                "symbol": "AMZN/USDT",
                "instrument_id": "AMZNUSDT-PERP.BINANCE_PERP",
            },
            {
                "exchange": "ibkr",
                "symbol": "AMZN/USD",
                "instrument_id": "AMZN.NASDAQ",
            },
        ],
    }
    args = Namespace(
        config=Path("equities.toml"),
        mode=None,
        confirm_live=False,
        log_level=None,
        host=None,
        port=None,
        serve_fluxboard=False,
        fluxboard_dist=None,
        serve_pulse=False,
        pulse_dist=None,
    )

    monkeypatch.setattr(run_api, "_parse_args", lambda: args)
    monkeypatch.setattr(run_api, "_load_config", lambda path: config)
    monkeypatch.setattr(run_api, "_resolve_mode", lambda cfg, parsed: "paper")
    monkeypatch.setattr(run_api, "configure_python_logging", lambda **kwargs: None)
    monkeypatch.setattr(run_api, "emit_startup_banner", lambda **kwargs: None)
    monkeypatch.setattr(run_api, "_build_strategy_running_resolver", lambda: (lambda ids: {}))
    monkeypatch.setattr(run_api.redis, "Redis", lambda **kwargs: object())

    class _FakePulse:
        def register_routes(self, app) -> None:
            captured["pulse_app"] = app

    monkeypatch.setattr(run_api, "PulseControlPlane", lambda: _FakePulse())
    monkeypatch.setattr(
        run_api,
        "_run_with_socketio_if_available",
        lambda app, *, host, port: captured.update({"host": host, "port": port}),
    )

    def _fake_create_flux_api_app(*args, **kwargs):
        captured["strategy_metadata"] = kwargs["strategy_metadata"]
        captured["strategy_metadata_resolver"] = kwargs["strategy_metadata_resolver"]
        captured["strategy_contracts"] = kwargs["strategy_contracts"]
        return Flask(__name__)

    monkeypatch.setattr(run_api, "create_flux_api_app", _fake_create_flux_api_app)

    run_api.main()

    resolver = captured["strategy_metadata_resolver"]
    maker_metadata = resolver("aapl_tradexyz_maker")
    taker_metadata = resolver("aapl_tradexyz_taker")
    amzn_binance_maker = resolver("amzn_binance_perp_maker")
    amzn_binance_taker = resolver("amzn_binance_perp_taker")

    assert maker_metadata.base_asset == "AAPL"
    assert maker_metadata.param_set == "equities_maker"
    assert maker_metadata.strategy_family == "equities_maker"
    assert taker_metadata.base_asset == "AAPL"
    assert taker_metadata.param_set == "equities_taker"
    assert taker_metadata.strategy_family == "equities_taker"
    assert amzn_binance_maker.base_asset == "AMZN"
    assert amzn_binance_maker.param_set == "equities_maker"
    assert amzn_binance_maker.strategy_family == "equities_maker"
    assert amzn_binance_taker.base_asset == "AMZN"
    assert amzn_binance_taker.param_set == "equities_taker"
    assert amzn_binance_taker.strategy_family == "equities_taker"
    assert captured["strategy_contracts"] == config["strategy_contracts"]
