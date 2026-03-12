from __future__ import annotations

import importlib
from decimal import Decimal
from pathlib import Path
from types import SimpleNamespace

import pytest

import nautilus_trader.flux.api.app as app_module
from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.common.config import FluxConfig
from nautilus_trader.flux.common.config import FluxIdentityConfig
from nautilus_trader.flux.common.config import FluxRedisConfig
from nautilus_trader.flux.common.config import FluxVenuesConfig
from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.common.portfolio_inventory import encode_portfolio_inventory
from nautilus_trader.flux.common.strategy_contracts import decode_strategy_contracts
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4Strategy
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4StrategyConfig
from nautilus_trader.model.identifiers import InstrumentId


def _compat_flux_config(flux_config):
    return app_module.FluxConfig(
        mode=flux_config.mode,
        confirm_live=flux_config.confirm_live,
        identity=flux_config.identity,
        redis=flux_config.redis,
        venues=flux_config.venues,
    )


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _seed_required_schema_keys(redis_client, flux_config) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.state(),
        {"bot_on": True, "managed_orders": 2, "ts_ms": 1_700_000_000_000},
    )
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {
            "qty": "1.0",
            "bot_on": "1",
            "max_age_ms": "10000",
        },
    )
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(
        keys.fv_stream(),
        [{"strategy_id": flux_config.identity.strategy_id, "fv": 100.0}],
    )


def _seed_required_schema_keys_for_strategy(redis_client, flux_config, strategy_id: str) -> None:
    keys = FluxRedisKeys(
        strategy_id=strategy_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_json(
        keys.state(),
        {"bot_on": True, "managed_orders": 2, "ts_ms": 1_700_000_000_000},
    )
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {
            "qty": "1.0",
            "bot_on": "1",
            "max_age_ms": "10000",
        },
    )
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(keys.fv_stream(), [{"strategy_id": strategy_id, "fv": 100.0}])


def test_equities_contract_docs_define_shared_account_row_provenance() -> None:
    contract = (_repo_root() / "fluxboard/docs/equities_contract.md").read_text(encoding="utf-8")

    assert "source_scope" in contract
    assert "account_scope_id" in contract
    assert "source_strategy_ids" in contract
    assert "Later balance-model tasks" in contract
    assert 'scope = "shared_account"' in contract


def test_strategy_contract_module_defines_canonical_account_scope_identity() -> None:
    path = _repo_root() / "systems/flux/flux/common/strategy_contracts.py"

    assert path.exists()
    contract_module = path.read_text(encoding="utf-8")
    assert "@dataclass(frozen=True, slots=True)" in contract_module
    assert "class StrategyContractEntry" in contract_module
    assert "portfolio_asset_id: str" in contract_module
    assert "execution_account_scope_id: str" in contract_module
    assert "reference_account_scope_id: str" in contract_module
    assert "hedge_account_scope_id: str | None = None" in contract_module


def test_decode_strategy_contracts_rejects_blank_required_fields() -> None:
    with pytest.raises(ValueError, match="portfolio_asset_id"):
        decode_strategy_contracts(
            [
                {
                    "strategy_id": "aapl_tradexyz_makerv3",
                    "portfolio_asset_id": "   ",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
            ],
        )


def test_decode_strategy_contracts_normalizes_optional_hedge_scope() -> None:
    decoded = decode_strategy_contracts(
        [
            {
                "strategy_id": "aapl_tradexyz_makerv3",
                "portfolio_asset_id": "AAPL",
                "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": "AAPL.NASDAQ",
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": " ",
            },
        ],
    )

    assert len(decoded) == 1
    assert decoded[0].hedge_account_scope_id is None


def test_decode_strategy_contracts_rejects_non_string_required_fields() -> None:
    with pytest.raises(TypeError, match="strategy_id"):
        decode_strategy_contracts(
            [
                {
                    "strategy_id": 123,
                    "portfolio_asset_id": "AAPL",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
            ],
        )


def test_decode_strategy_contracts_rejects_non_mapping_rows() -> None:
    with pytest.raises(TypeError, match="manifest row"):
        decode_strategy_contracts([123])


def test_signals_profile_equities_returns_only_allowlisted_strategies(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_02")
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_03")

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["id"] for row in body["data"]["strategies"]] == [
        flux_config.identity.strategy_id,
        "strategy_02",
    ]


def test_api_strategies_falls_back_to_default_metadata_when_resolver_has_no_entry(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        strategy_metadata_resolver=lambda strategy_id: {
            "strategy_02": app_module.StrategyMetadata(
                strategy_class="maker_v4",
                strategy_groups="equities",
                base_asset="AAPL",
                quote_asset="USD",
                param_set="makerv4",
                strategy_family="maker_v4",
                strategy_version="v4",
            ),
        }[strategy_id],
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/strategies")
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert row["meta"]["strategy_id"] == flux_config.identity.strategy_id
    assert row["meta"]["class"] == "maker_v3"


def test_signals_profile_equities_emits_makerv4_quote_snapshot(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv4"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.state(),
        {
            "bot_on": False,
            "managed_orders": 0,
            "state": "hedge_paused",
            "ts_ms": 1_700_000_000_000,
            "maker_role_map": {
                "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                "ref_leg": "AAPL.NASDAQ",
                "hedge_leg": "AAPL.NASDAQ",
            },
            "maker_v4": {
                "quote_snapshot": {
                    "effective_spread_bps": 6.5,
                    "quoted_spread_bps": 8.0,
                    "expected_maker_fee_bps": 0.25,
                    "assumed_hedge_fee_bps": 1.0,
                    "hedge_ready": False,
                    "hedge_route": "SMART",
                    "effective_account_source": "userRole.master",
                    "hedge_disabled_reason": "stale_quote",
                    "ibkr_quote_age_ms": 1200,
                },
            },
        },
    )
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(keys.fv_stream(), [{"strategy_id": strategy_id, "fv": 255.8}])
    redis_client.set_json(
        keys.market_last(
            "hyperliquid",
            "AAPL",
            "USD",
            instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
        ),
        {
            "exchange": "hyperliquid",
            "symbol": "AAPL/USD",
            "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
            "bid": 255.7,
            "ask": 255.9,
            "ts_ms": 1_700_000_000_000,
        },
    )
    redis_client.set_json(
        keys.market_last("ibkr", "AAPL", "USD", instrument_id="AAPL.NASDAQ"),
        {
            "exchange": "ibkr",
            "symbol": "AAPL/USD",
            "instrument_id": "AAPL.NASDAQ",
            "bid": 255.6,
            "ask": 255.8,
            "ts_ms": 1_700_000_000_001,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert row["strategy_family"] == "maker_v4"
    assert row["maker_v4"]["quote_snapshot"]["maker_leg"]["venue"] == "HYPERLIQUID"
    assert row["maker_v4"]["quote_snapshot"]["hedge_leg"]["venue"] == "IBKR"
    assert row["maker_v4"]["quote_snapshot"]["effective_spread_bps"] == 6.5


def test_signals_profile_equities_reads_ibkr_reference_market_from_listing_venue_alias(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv3"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.state(),
        {
            "bot_on": False,
            "managed_orders": 0,
            "state": "bot_off",
            "ts_ms": 1_700_000_000_000,
            "maker_role_map": {
                "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                "ref_leg": "nasdaq:AAPL.NASDAQ",
                "hedge_leg": "nasdaq:AAPL.NASDAQ",
            },
        },
    )
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "0", "max_age_ms": "10000"},
    )
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(
        keys.fv_stream(),
        [{"strategy_id": strategy_id, "fv": 255.8, "maker_mid": 255.8, "reference_mid": 255.7}],
    )
    redis_client.set_json(
        keys.market_last(
            "hyperliquid",
            "AAPL",
            "USD",
            instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
        ),
        {
            "exchange": "hyperliquid",
            "symbol": "AAPL/USD",
            "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
            "bid": 255.7,
            "ask": 255.9,
            "ts_ms": 1_700_000_000_000,
        },
    )
    redis_client.set_json(
        keys.market_last("nasdaq", "AAPL", "USD", instrument_id="AAPL.NASDAQ"),
        {
            "exchange": "nasdaq",
            "symbol": "AAPL/USD",
            "instrument_id": "AAPL.NASDAQ",
            "bid": 255.6,
            "ask": 255.8,
            "ts_ms": 1_700_000_000_001,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v3",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv3",
            strategy_family="maker_v3",
            strategy_version="v3",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv3",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert row["legs"]["ibkr:AAPL.NASDAQ"]["bid"] == 255.6
    assert row["legs"]["ibkr:AAPL.NASDAQ"]["ask"] == 255.8
    assert row["maker_v3"]["quote_snapshot"]["ref_ts_ms"] == 1_700_000_000_001
    assert row["debug"]["md_health"]["stale_legs"] == []


def test_signals_profile_equities_ignores_listing_venue_alias_when_canonical_ibkr_row_exists(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv3"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.state(),
        {
            "bot_on": False,
            "managed_orders": 0,
            "state": "bot_off",
            "ts_ms": 1_700_000_000_000,
            "maker_role_map": {
                "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                "ref_leg": "nasdaq:AAPL.NASDAQ",
                "hedge_leg": "nasdaq:AAPL.NASDAQ",
            },
        },
    )
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "0", "max_age_ms": "10000"},
    )
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(
        keys.fv_stream(),
        [{"strategy_id": strategy_id, "fv": 255.8, "maker_mid": 255.8}],
    )
    redis_client.set_json(
        keys.market_last(
            "hyperliquid",
            "AAPL",
            "USD",
            instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
        ),
        {
            "exchange": "hyperliquid",
            "symbol": "AAPL/USD",
            "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
            "bid": 255.7,
            "ask": 255.9,
            "ts_ms": 1_700_000_000_000,
        },
    )
    redis_client.set_json(
        keys.market_last("ibkr", "AAPL", "USD", instrument_id="AAPL.NASDAQ"),
        {
            "exchange": "ibkr",
            "symbol": "AAPL/USD",
            "instrument_id": "AAPL.NASDAQ",
            "state": "halted",
        },
    )
    redis_client.set_json(
        keys.market_last("nasdaq", "AAPL", "USD", instrument_id="AAPL.NASDAQ"),
        {
            "exchange": "nasdaq",
            "symbol": "AAPL/USD",
            "instrument_id": "AAPL.NASDAQ",
            "bid": 255.6,
            "ask": 255.8,
            "ts_ms": 1_700_000_000_001,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v3",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv3",
            strategy_family="maker_v3",
            strategy_version="v3",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv3",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert row["legs"]["ibkr:AAPL.NASDAQ"]["bid"] is None
    assert row["legs"]["ibkr:AAPL.NASDAQ"]["ask"] is None
    assert row["legs"]["ibkr:AAPL.NASDAQ"]["state"] == "halted"
    assert row["maker_v3"]["quote_snapshot"]["ref_ts_ms"] is None
    assert row["debug"]["md_health"]["stale_legs"] == ["ibkr:AAPL.NASDAQ"]


def test_balances_profile_equities_marks_ibkr_positions_from_listing_venue_alias(
    monkeypatch,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_001_000)
    strategy_id = "aapl_tradexyz_makerv3"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "0", "max_age_ms": "10000"},
    )
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": strategy_id,
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "USD",
                "free": "1000",
                "total": "1000",
                "ts_ms": 1_700_000_000_100,
            },
            {
                "strategy_id": strategy_id,
                "kind": "position",
                "exchange": "ibkr",
                "account_id": "U1234567",
                "instrument_id": "AAPL.NASDAQ",
                "quantity": "5",
                "signed_qty": "5",
                "side": "LONG",
                "ts_ms": 1_700_000_000_300,
            },
        ],
    )
    redis_client.set_json(
        keys.market_last("nasdaq", "AAPL", "USD", instrument_id="AAPL.NASDAQ"),
        {
            "exchange": "nasdaq",
            "symbol": "AAPL/USD",
            "instrument_id": "AAPL.NASDAQ",
            "bid": 255.6,
            "ask": 255.8,
            "ts_ms": 1_700_000_000_400,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v3",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv3",
            strategy_family="maker_v3",
            strategy_version="v3",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv3",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["degraded"] is False
    position_rows = [row for row in body["data"]["rows"] if row.get("kind") == "position"]
    assert len(position_rows) == 1
    assert position_rows[0]["exchange"] == "ibkr"
    assert position_rows[0]["asset"] == "AAPL"
    assert position_rows[0]["mark_raw"] == 255.7
    assert position_rows[0]["mv_raw"] == 1278.5


def test_signals_profile_equities_projects_makerv4_inventory_fields_from_strategy_state(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv4"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    strategy = MakerV4Strategy(
        config=MakerV4StrategyConfig(
            maker_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
            order_qty=Decimal("1"),
            external_strategy_id=strategy_id,
            strategy_id=strategy_id,
        ),
    )
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._instruments = {
        maker_id: SimpleNamespace(
            raw_symbol="AAPL/USD",
            base_currency=SimpleNamespace(code="AAPL"),
            quote_currency=SimpleNamespace(code="USD"),
            settlement_currency=SimpleNamespace(code="USD"),
            multiplier=Decimal("1"),
            is_inverse=False,
            make_qty=lambda value: Decimal(str(value)),
            make_price=lambda value: Decimal(str(value)),
            calculate_base_exposure_qty=lambda qty, _price=None: Decimal(str(qty)),
        ),
        ref_id: SimpleNamespace(
            raw_symbol="AAPL",
            base_currency=SimpleNamespace(code="AAPL"),
            quote_currency=SimpleNamespace(code="USD"),
            settlement_currency=SimpleNamespace(code="USD"),
            multiplier=Decimal("1"),
            is_inverse=False,
            make_qty=lambda value: Decimal(str(value)),
            make_price=lambda value: Decimal(str(value)),
            calculate_base_exposure_qty=lambda qty, _price=None: Decimal(str(qty)),
        ),
    }
    strategy._latest_quotes = {
        maker_id: {"bid": Decimal("255.70"), "ask": Decimal("255.90"), "ts_ns": 1_700_000_000_000_000_000},
        ref_id: {"bid": Decimal("255.60"), "ask": Decimal("255.80"), "ts_ns": 1_700_000_000_001_000_000},
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda: [SimpleNamespace(instrument_id=maker_id, signed_qty=Decimal("12"))],
        accounts=lambda: [],
    )
    strategy.configure_portfolio_inventory_feed(
        redis_client=redis_client,
        portfolio_id="equities",
        namespace="flux",
        schema_version="v1",
        stale_after_ms=3_000,
        allow_partial_global_risk=True,
    )
    redis_client.strings[
        FluxRedisKeys.portfolio_inventory(
            portfolio_id="equities",
            base_currency="AAPL",
            namespace="flux",
            schema_version="v1",
        )
    ] = encode_portfolio_inventory(
        {
            "portfolio_id": "equities",
            "base_currency": "AAPL",
            "global_qty_base": "37",
            "global_qty": "37",
            "global_qty_base_complete": False,
            "global_qty_complete": False,
            "aggregation_mode": "partial",
            "ts_ms": 1_700_000_000_001,
            "stale_after_ms": 3_000,
        },
    ).encode("utf-8")
    published: list[tuple[str, dict[str, object]]] = []
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_state_snapshot(now_ns=1_700_000_000_002_000_000)

    redis_client.set_json(keys.state(), published[-1][1])
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(keys.fv_stream(), [{"strategy_id": strategy_id, "fv": 255.8}])

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert row["position_qty_venue"] == 12.0
    assert row["position_qty_base"] == 12.0
    assert row["local_qty_base"] == 12.0
    assert row["local_qty"] == 12.0
    assert row["global_qty_base"] == 37.0
    assert row["global_qty"] == 37.0
    assert row["global_qty_base_complete"] is False
    assert row["global_qty_complete"] is False
    assert row["aggregation_mode"] == "partial"
    assert row["qty_conversion_status"] == "identity"
    assert row["qty_conversion_source"] == "generic:multiplier=1"


def test_params_profile_equities_does_not_discover_unallowlisted_strategies(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    tertiary_keys = FluxRedisKeys(
        strategy_id="strategy_03",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_hash_json(secondary_keys.params_hash_key(), {"qty": "2.0"})
    redis_client.set_hash_json(tertiary_keys.params_hash_key(), {"qty": "3.0"})

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/params", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["strategy_id"] for row in body["data"]] == [
        flux_config.identity.strategy_id,
        "strategy_02",
    ]


def test_balances_profile_equities_aggregates_cash_and_positions(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_hash_json(secondary_keys.params_hash_key(), {"qty": "2.0"})
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
                {
                    "strategy_id": flux_config.identity.strategy_id,
                    "exchange": "venue_a",
                    "account": "main",
                    "asset": "USDT",
                    "free": "100",
                    "total": "100",
                    "ts_ms": 1_000,
                },
                {
                    "strategy_id": flux_config.identity.strategy_id,
                    "kind": "position",
                    "exchange": "venue_a",
                    "instrument_id": "ABCUSDT-LINEAR.BYBIT",
                    "quantity": "2",
                    "side": "LONG",
                },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
                {
                    "strategy_id": "strategy_02",
                    "exchange": "venue_a",
                    "account": "main",
                    "asset": "USDT",
                    "free": "140",
                    "total": "140",
                    "ts_ms": 2_000,
                },
                {
                    "strategy_id": "strategy_02",
                    "kind": "position",
                    "exchange": "venue_a",
                    "instrument_id": "ABCUSDT-LINEAR.BYBIT",
                    "quantity": "1",
                    "side": "SHORT",
                },
        ],
    )

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    by_row_id = {row["row_id"]: row for row in rows}
    cash_row = by_row_id["equities:cash:venue_a:main:USDT"]
    assert cash_row["free"] == "140"
    assert cash_row["total"] == "140"


def test_balances_profile_equities_keeps_hyperliquid_usdc_collateral_for_usd_perps(
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "accounts": [
                    {
                        "account_id": "hyperliquid-main",
                        "events": [
                            {
                                "account_id": "hyperliquid-main",
                                "ts_ms": 1_700_000_000_123,
                                "balances": [
                                    {
                                        "currency": "USDC",
                                        "free": "250.5",
                                        "locked": "0",
                                        "total": "250.5",
                                    },
                                ],
                            },
                        ],
                    },
                ],
            },
        ],
    )

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["count"] == 1
    cash_row = body["data"]["rows"][0]
    assert cash_row["asset"] == "USDC"
    assert cash_row["exchange"] == "hyperliquid"
    assert cash_row["strategy_id"] == "equities"


def test_balances_profile_equities_includes_hyperliquid_and_ibkr_rows(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv4"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": strategy_id,
                "exchange": "hyperliquid",
                "account": "hyperliquid-main",
                "asset": "USDC",
                "free": "250.5",
                "total": "250.5",
                "ts_ms": 1_700_000_000_100,
            },
            {
                "strategy_id": strategy_id,
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "USD",
                "free": "1000",
                "total": "1000",
                "ts_ms": 1_700_000_000_200,
            },
            {
                "strategy_id": strategy_id,
                "kind": "position",
                "exchange": "ibkr",
                "instrument_id": "AAPL.NASDAQ",
                "quantity": "5",
                "side": "LONG",
                "ts_ms": 1_700_000_000_300,
            },
        ],
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    venues = {row["exchange"] for row in rows}
    assert venues == {"hyperliquid", "ibkr"}
    assert any(row["asset"] == "USDC" for row in rows)
    assert any(row["asset"] == "USD" for row in rows)
    assert any(row.get("kind") == "position" and row["exchange"] == "ibkr" for row in rows)


def test_balances_profile_equities_marks_ibkr_positions_from_listing_venue_alias_market_data(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv3"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": strategy_id,
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "USD",
                "free": "1000",
                "total": "1000",
                "ts_ms": 1_700_000_000_200,
            },
            {
                "strategy_id": strategy_id,
                "kind": "position",
                "exchange": "ibkr",
                "instrument_id": "AAPL.NASDAQ",
                "quantity": "5",
                "side": "LONG",
                "ts_ms": 1_700_000_000_300,
            },
        ],
    )
    redis_client.set_json(
        keys.market_last("nasdaq", "AAPL", "USD", instrument_id="AAPL.NASDAQ"),
        {
            "exchange": "nasdaq",
            "symbol": "AAPL/USD",
            "instrument_id": "AAPL.NASDAQ",
            "bid": 255.7,
            "ask": 255.9,
            "ts_ms": 1_700_000_000_001,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v3",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv3",
            strategy_family="maker_v3",
            strategy_version="v3",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv3",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    ibkr_position = next(
        row
        for row in rows
        if row["exchange"] == "ibkr" and row.get("kind") == "position" and row["asset"] == "AAPL"
    )
    assert ibkr_position["mark_raw"] == 255.8
    assert ibkr_position["mv_raw"] == 1279.0


def test_balances_profile_equities_flattens_nested_ibkr_account_events_with_explicit_venue(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv4"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": strategy_id,
                "ts_ms": 1_700_000_000_100,
                "accounts": [
                    {
                        "account_id": "HYPERLIQUID-master",
                        "events": [
                            {
                                "account_id": "HYPERLIQUID-master",
                                "balances": [
                                    {
                                        "currency": "USDC",
                                        "free": "250.5",
                                        "locked": "0",
                                        "total": "250.5",
                                    },
                                ],
                            },
                        ],
                    },
                    {
                        "account_id": "U1234567",
                        "venue": "ibkr",
                        "events": [
                            {
                                "account_id": "U1234567",
                                "venue": "ibkr",
                                "balances": [
                                    {
                                        "currency": "USD",
                                        "free": "1000",
                                        "locked": "0",
                                        "total": "1000",
                                    },
                                ],
                            },
                        ],
                    },
                ],
                "positions": [
                    {
                        "kind": "position",
                        "exchange": "ibkr",
                        "account_id": "U1234567",
                        "instrument_id": "AAPL.NASDAQ",
                        "quantity": "5",
                        "signed_qty": "5",
                        "side": "LONG",
                        "ts_ms": 1_700_000_000_200,
                    },
                ],
            },
        ],
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    venues = {row["exchange"] for row in rows}
    assert venues == {"hyperliquid", "ibkr"}
    assert any(
        row["exchange"] == "ibkr" and row["asset"] == "USD" and row.get("kind") != "position"
        for row in rows
    )
    assert any(
        row["exchange"] == "ibkr" and row.get("kind") == "position" and row["asset"] == "AAPL"
        for row in rows
    )


def test_balances_profile_equities_marks_shared_ibkr_cash_rows_as_shared_account(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    primary_strategy_id = "aapl_tradexyz_makerv4"
    secondary_strategy_id = "msft_tradexyz_makerv4"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=primary_strategy_id,
            strategy_instance_id=primary_strategy_id,
            trader_id="trader_01",
            external_strategy_id=primary_strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id=secondary_strategy_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_hash_json(secondary_keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": primary_strategy_id,
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "USD",
                "free": "1000",
                "total": "1000",
                "ts_ms": 1_700_000_000_100,
            },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
            {
                "strategy_id": secondary_strategy_id,
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "USD",
                "free": "1000",
                "total": "1000",
                "ts_ms": 1_700_000_000_200,
            },
        ],
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
        ),
        profile_strategy_map={"equities": [primary_strategy_id, secondary_strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    ibkr_cash_rows = [
        row
        for row in body["data"]["rows"]
        if row["exchange"] == "ibkr" and row["asset"] == "USD" and row.get("kind") != "position"
    ]
    assert len(ibkr_cash_rows) == 1
    assert ibkr_cash_rows[0]["scope"] == "shared_account"


def test_balances_profile_equities_preserves_shared_account_provenance_fields(
    monkeypatch,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_000_200)
    primary_strategy_id = "aapl_tradexyz_makerv3"
    secondary_strategy_id = "msft_tradexyz_makerv3"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=primary_strategy_id,
            strategy_instance_id=primary_strategy_id,
            trader_id="trader_01",
            external_strategy_id=primary_strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id=secondary_strategy_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_hash_json(secondary_keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="equities",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "equities",
            "inventory_by_asset": {
                "AAPL": {
                    "portfolio_id": "equities",
                    "base_currency": "AAPL",
                    "global_qty_base": "5",
                    "global_qty": "5",
                    "aggregation_mode": "partial",
                    "global_qty_base_complete": False,
                    "global_qty_complete": False,
                    "ts_ms": 1_700_000_000_000,
                    "stale_after_ms": 3_000,
                    "components": [],
                    "missing_required": [],
                    "stale_required": [],
                    "null_qty_required": [],
                    "degraded": False,
                },
                "MSFT": {
                    "portfolio_id": "equities",
                    "base_currency": "MSFT",
                    "global_qty_base": "0",
                    "global_qty": "0",
                    "aggregation_mode": "partial",
                    "global_qty_base_complete": False,
                    "global_qty_complete": False,
                    "ts_ms": 1_700_000_000_000,
                    "stale_after_ms": 3_000,
                    "components": [],
                    "missing_required": [],
                    "stale_required": [],
                    "null_qty_required": [],
                    "degraded": False,
                },
            },
            "balances": {
                "rows": [
                    {
                        "exchange": "hyperliquid",
                        "account": "hyperliquid-main",
                        "asset": "USD",
                        "free": "250.5",
                        "total": "250.5",
                        "ts_ms": 1_700_000_000_050,
                    },
                ],
            },
            "accounts": {
                "rows": [
                    {
                        "exchange": "ibkr",
                        "account": "U1234567",
                        "asset": "USD",
                        "free": "1000",
                        "total": "1000",
                        "ts_ms": 1_700_000_000_100,
                        "source_scope": "shared_account",
                        "account_scope_id": "ibkr.reference.main",
                        "source_strategy_ids": [
                            primary_strategy_id,
                            secondary_strategy_id,
                        ],
                        "strategy_id": "equities",
                    },
                ],
            },
            "server_ts_ms": 1_700_000_000_150,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v3",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv3",
            strategy_family="maker_v3",
            strategy_version="v3",
        ),
        profile_strategy_map={"equities": [primary_strategy_id, secondary_strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv3",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["source"] == "portfolio_snapshot_v2"
    ibkr_cash_row = next(
        row
        for row in body["data"]["rows"]
        if row["exchange"] == "ibkr" and row["asset"] == "USD" and row.get("kind") != "position"
    )
    assert ibkr_cash_row["source_scope"] == "shared_account"
    assert ibkr_cash_row["account_scope_id"] == "ibkr.reference.main"
    assert ibkr_cash_row["source_strategy_ids"] == [
        primary_strategy_id,
        secondary_strategy_id,
    ]
    assert "strategy_id" not in ibkr_cash_row


def test_namespace_identity_check_import_order_passes_for_flux_api_modules() -> None:
    globals_dict: dict[str, object] = {}

    exec(
        "\n".join(
            [
                "import flux.api.app as a1",
                "import nautilus_trader.flux.api.app as a2",
                "import flux.api.socketio as s1",
                "import nautilus_trader.flux.api.socketio as s2",
                "import flux.api.payloads as p1",
                "import nautilus_trader.flux.api.payloads as p2",
            ],
        ),
        globals_dict,
    )

    assert globals_dict["a1"] is globals_dict["a2"]
    assert globals_dict["s1"] is globals_dict["s2"]
    assert globals_dict["p1"] is globals_dict["p2"]


def test_flux_strategy_package_identity_matches_compat_namespace() -> None:
    root_pkg = importlib.import_module("flux.strategies")
    compat_pkg = importlib.import_module("nautilus_trader.flux.strategies")

    assert root_pkg is compat_pkg


def test_trades_profile_equities_fans_out_allowlisted_strategies_in_global_time_order(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    tertiary_keys = FluxRedisKeys(
        strategy_id="strategy_03",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.add_stream_rows(
        primary_keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "t-primary",
                "seq": 11,
                "ts_ms": 3_000,
                "coin": "AAPL",
                "exchange": "hyperliquid",
                "side": "buy",
            },
        ],
    )
    redis_client.add_stream_rows(
        secondary_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "t-secondary",
                "seq": 12,
                "ts_ms": 2_000,
                "coin": "AAPL",
                "exchange": "hyperliquid",
                "side": "buy",
            },
        ],
    )
    redis_client.add_stream_rows(
        tertiary_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_03",
                "row_id": "t-tertiary",
                "seq": 13,
                "ts_ms": 4_000,
                "coin": "AAPL",
                "exchange": "hyperliquid",
                "side": "sell",
            },
        ],
    )

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/trades", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == ["t-primary", "t-secondary"]
    assert {row["strategy_id"] for row in body["data"]["rows"]} == {
        flux_config.identity.strategy_id,
        "strategy_02",
    }
    assert body["data"]["last_seq"] == 0


def test_socket_profile_equities_joins_room(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_02")

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    socket_server = app.extensions["flux_socketio_server"]

    client = socketio.test_client(app)
    join_ack = client.emit("set_profile", {"profile": "equities"}, callback=True)

    assert join_ack["ok"] is True
    assert join_ack["profile"] == "equities"
    assert join_ack["room"] == "profile:equities"
    assert len(socket_server.manager.rooms.get("/", {}).get("profile:equities") or {}) == 1

    client.disconnect()
