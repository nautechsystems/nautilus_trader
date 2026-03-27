from __future__ import annotations

import importlib
from decimal import Decimal
from pathlib import Path
from types import SimpleNamespace
import time

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
from nautilus_trader.flux.strategies.equities_maker.runtime_params import (
    EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS,
)
from nautilus_trader.flux.strategies.equities_maker.runtime_params import (
    EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA,
)
from nautilus_trader.flux.strategies.equities_taker.runtime_params import (
    EQUITIES_TAKER_RUNTIME_PARAM_DEFAULTS,
)
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


def _split_equities_metadata_for_strategy(strategy_id: str) -> app_module.StrategyMetadata:
    if strategy_id.endswith("_maker"):
        return app_module.StrategyMetadata(
            strategy_class="equities_maker",
            strategy_groups="equities",
            base_asset=strategy_id.split("_", maxsplit=1)[0].upper(),
            quote_asset="USD",
            param_set="equities_maker",
            strategy_family="equities_maker",
            strategy_version="v1",
        )
    if strategy_id.endswith("_taker"):
        return app_module.StrategyMetadata(
            strategy_class="equities_taker",
            strategy_groups="equities",
            base_asset=strategy_id.split("_", maxsplit=1)[0].upper(),
            quote_asset="USD",
            param_set="equities_taker",
            strategy_family="equities_taker",
            strategy_version="v1",
        )
    raise KeyError(strategy_id)


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
                    "maker_venue": "HYPERLIQUID",
                    "maker_symbol": "AAPL",
                    "market_type": "PERP",
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
                "maker_venue": "HYPERLIQUID",
                "maker_symbol": "AAPL",
                "market_type": "PERP",
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
                    "maker_venue": "HYPERLIQUID",
                    "maker_symbol": "AAPL",
                    "market_type": "PERP",
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
    now_ms = int(time.time() * 1000)
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
                "ts_ms": now_ms,
            "maker_role_map": {
                "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                "ref_leg": "AAPL.NASDAQ",
                "hedge_leg": "AAPL.NASDAQ",
            },
            "maker_v4": {
                "quote_snapshot": {
                    "mid_spread_bps": 2.0,
                    "arb_bid_spread_bps": 14.0,
                    "arb_ask_spread_bps": -11.0,
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
                "ts_ms": now_ms - 50,
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
                "ts_ms": now_ms - 1_200,
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
    assert row["meta"]["deprecated"] is True
    assert row["meta"]["replacement"] == "equities_maker/equities_taker"
    assert (
        row["meta"]["deprecation_note"]
        == "Legacy compatibility only; use equities_maker/equities_taker for new equities production enrollment."
    )
    assert row["maker_v4"]["quote_snapshot"]["maker_leg"]["venue"] == "HYPERLIQUID"
    assert row["maker_v4"]["quote_snapshot"]["hedge_leg"]["venue"] == "IBKR"
    assert row["maker_v4"]["quote_snapshot"]["mid_spread_bps"] == 2.0
    assert row["maker_v4"]["quote_snapshot"]["arb_bid_spread_bps"] == 14.0
    assert row["maker_v4"]["quote_snapshot"]["arb_ask_spread_bps"] == -11.0
    assert row["maker_v4"]["quote_snapshot"]["effective_spread_bps"] == 6.5
    assert row["maker_v4"]["quote_snapshot"]["maker_leg"]["feed_state"] == "ok"
    assert row["maker_v4"]["quote_snapshot"]["maker_leg"]["quote_state"] == "fresh"
    assert row["maker_v4"]["quote_snapshot"]["ref_leg"]["feed_state"] == "ok"
    assert row["maker_v4"]["quote_snapshot"]["ref_leg"]["quote_state"] == "old"


def test_signals_profile_equities_scopes_makerv4_contracts_per_strategy(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_ids = ["aapl_tradexyz_makerv4", "amd_tradexyz_makerv4"]
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_ids[0],
            strategy_instance_id=strategy_ids[0],
            trader_id="trader_01",
            external_strategy_id=strategy_ids[0],
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )

    def _seed_strategy(
        *,
        strategy_id: str,
        base: str,
        maker_bid: float,
        maker_ask: float,
        ref_bid: float,
        ref_ask: float,
    ) -> None:
        keys = FluxRedisKeys(
            strategy_id=strategy_id,
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        )
        redis_client.set_json(
            keys.state(),
            {
                "bot_on": False,
                "managed_orders": 0,
                "state": "bot_off",
                "ts_ms": 1_700_000_000_000,
                "maker_role_map": {
                    "maker_leg": f"hyperliquid:XYZ:{base}-USD-PERP.HYPERLIQUID",
                    "ref_leg": f"{base}.NASDAQ",
                    "hedge_leg": f"{base}.NASDAQ",
                },
                "maker_v4": {
                    "quote_snapshot": {
                        "effective_spread_bps": 6.5,
                    },
                },
            },
        )
        redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
        redis_client.set_json(keys.balances_snapshot(), [])
        redis_client.add_stream_rows(keys.fv_stream(), [{"strategy_id": strategy_id, "fv": maker_ask}])
        redis_client.set_json(
            keys.market_last(
                "hyperliquid",
                base,
                "USD",
                instrument_id=f"xyz:{base}-USD-PERP.HYPERLIQUID",
            ),
            {
                "exchange": "hyperliquid",
                "symbol": f"{base}/USD",
                "instrument_id": f"xyz:{base}-USD-PERP.HYPERLIQUID",
                "bid": maker_bid,
                "ask": maker_ask,
                "ts_ms": 1_700_000_000_000,
            },
        )
        redis_client.set_json(
            keys.market_last("ibkr", base, "USD", instrument_id=f"{base}.NASDAQ"),
            {
                "exchange": "ibkr",
                "symbol": f"{base}/USD",
                "instrument_id": f"{base}.NASDAQ",
                "bid": ref_bid,
                "ask": ref_ask,
                "ts_ms": 1_700_000_000_001,
            },
        )

    _seed_strategy(
        strategy_id="aapl_tradexyz_makerv4",
        base="AAPL",
        maker_bid=255.7,
        maker_ask=255.9,
        ref_bid=255.6,
        ref_ask=255.8,
    )
    _seed_strategy(
        strategy_id="amd_tradexyz_makerv4",
        base="AMD",
        maker_bid=197.62,
        maker_ask=197.73,
        ref_bid=197.57,
        ref_ask=197.61,
    )

    contract_catalog = (
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
        app_module.ContractCatalogEntry(
            exchange="hyperliquid",
            symbol="AMD/USD",
            instrument_id="xyz:AMD-USD-PERP.HYPERLIQUID",
        ),
        app_module.ContractCatalogEntry(
            exchange="ibkr",
            symbol="AMD/USD",
            instrument_id="AMD.NASDAQ",
        ),
    )
    contract_catalog_by_strategy = {
        "aapl_tradexyz_makerv4": contract_catalog[:2],
        "amd_tradexyz_makerv4": contract_catalog[2:],
    }
    strategy_metadata_map = {
        "aapl_tradexyz_makerv4": app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
        ),
        "amd_tradexyz_makerv4": app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="AMD",
            quote_asset="USD",
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
        ),
    }

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        contract_catalog_resolver=lambda strategy_id: contract_catalog_by_strategy.get(
            strategy_id,
            contract_catalog,
        ),
        strategy_metadata=strategy_metadata_map[strategy_ids[0]],
        strategy_metadata_resolver=strategy_metadata_map.__getitem__,
        profile_strategy_map={"equities": strategy_ids},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    rows = {row["id"]: row for row in body["data"]["strategies"]}
    amd_row = rows["amd_tradexyz_makerv4"]
    assert amd_row["maker_role_map"]["maker_leg"] == "hyperliquid:XYZ:AMD-USD-PERP.HYPERLIQUID"
    assert amd_row["maker_v4"]["quote_snapshot"]["maker_leg"]["instrument_id"] == (
        "XYZ:AMD-USD-PERP.HYPERLIQUID"
    )
    assert amd_row["legs_order"] == [
        "hyperliquid:XYZ:AMD-USD-PERP.HYPERLIQUID",
        "ibkr:AMD.NASDAQ",
    ]


def test_signals_profile_equities_emits_makerv4_execution_mode_overnight_and_fee_operator_contract(
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
            "bot_on": True,
            "managed_orders": 1,
            "state": "running",
            "ts_ms": 1_700_000_000_000,
            "maker_role_map": {
                "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                "ref_leg": "AAPL.NASDAQ",
                "hedge_leg": "AAPL.NASDAQ",
            },
            "maker_v4": {
                "quote_snapshot": {
                    "effective_spread_bps": 6.5,
                    "hedge_route": "SMART",
                },
                "hedge_policy": {
                    "route": "BLUEOCEAN",
                    "time_in_force": "IOC",
                    "outside_rth": False,
                    "include_overnight": False,
                    "cancel_after_ms": 5000,
                },
                "fee_assumptions": {
                    "ibkr_fee_plan": "tiered",
                    "ibkr_fee_min_usd": 0.35,
                    "maker_taker_fee_bps": 4.5,
                    "maker_maker_fee_bps": 0.25,
                    "assumed_hedge_fee_bps": 1.0,
                },
                "pending_hedge": {
                    "route": "SMART",
                    "time_in_force": "DAY",
                    "outside_rth": True,
                    "include_overnight": True,
                    "cancel_after_ms": None,
                },
            },
        },
    )
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "1", "execution_mode": "take_take"},
    )
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
    operator_params_schema = dict(params_schema)
    operator_params_schema["execution_mode"] = {"type": "select"}
    operator_params_defaults = dict(params_defaults)
    operator_params_defaults["execution_mode"] = "maker_hedge"

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
        params_schema=operator_params_schema,
        params_defaults=operator_params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert row["maker_v4"]["operator"] == {
        "execution_mode": "take_take",
        "behavior": "take_take",
        "hedge_policy": {
            "route": "SMART",
            "time_in_force": "DAY",
            "outside_rth": True,
            "include_overnight": True,
            "cancel_after_ms": None,
        },
        "fee_assumptions": {
            "ibkr_fee_plan": "tiered",
            "ibkr_fee_min_usd": 0.35,
            "maker_taker_fee_bps": 4.5,
            "maker_maker_fee_bps": 0.25,
            "assumed_hedge_fee_bps": 1.0,
        },
    }


def test_signals_profile_equities_emits_complete_makerv4_steady_state_hedge_policy_without_pending_hedge(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv4"
    overnight_ns = 1_742_176_800_000_000_000
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
            outside_rth_hedge_enabled=True,
            ibkr_hedge_route="BLUEOCEAN",
        ),
    )
    strategy._runtime_params.update(
        {
            "bot_on": True,
            "execution_mode": "take_take",
            "ibkr_fee_plan": "tiered",
            "ibkr_fee_min_usd": 0.35,
            "maker_taker_fee_bps": 4.5,
            "maker_maker_fee_bps": 0.25,
            "assumed_hedge_fee_bps": 1.0,
        }
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
        maker_id: {"bid": Decimal("255.70"), "ask": Decimal("255.90"), "ts_ns": overnight_ns},
        ref_id: {"bid": Decimal("255.60"), "ask": Decimal("255.80"), "ts_ns": overnight_ns},
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda: [],
        accounts=lambda: [],
    )
    published: list[tuple[str, dict[str, object]]] = []
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_state_snapshot(now_ns=overnight_ns)

    redis_client.set_json(keys.state(), published[-1][1])
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "1", "execution_mode": "take_take"},
    )
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(keys.fv_stream(), [{"strategy_id": strategy_id, "fv": 255.8}])

    operator_params_schema = dict(params_schema)
    operator_params_schema["execution_mode"] = {"type": "select"}
    operator_params_defaults = dict(params_defaults)
    operator_params_defaults["execution_mode"] = "maker_hedge"

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
        params_schema=operator_params_schema,
        params_defaults=operator_params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert "pending_hedge" not in row["state"]["maker_v4"]
    assert row["maker_v4"]["operator"]["execution_mode"] == "take_take"
    assert row["maker_v4"]["operator"]["hedge_policy"] == {
        "route": "SMART",
        "time_in_force": "DAY",
        "outside_rth": True,
        "include_overnight": True,
        "cancel_after_ms": 5000,
    }


def test_signals_profile_equities_surfaces_makerv4_hedge_backlog_in_operator_payload(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv4"
    now_ms = int(time.time() * 1000)
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
            "strategy_id": strategy_id,
            "state": "blocked_stale_quote",
            "bot_on": True,
            "ts_ms": now_ms,
            "maker_v4": {
                "quote_snapshot": {
                    "ts_ms": now_ms,
                    "maker_leg": {
                        "venue": "HYPERLIQUID",
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    },
                    "hedge_leg": {
                        "venue": "IBKR",
                        "instrument_id": "AAPL.NASDAQ",
                    },
                    "ref_leg": {
                        "venue": "IBKR",
                        "instrument_id": "AAPL.NASDAQ",
                    },
                },
                "hedge_backlog": {
                    "fill_id": "take_take:order-1",
                    "side": "SELL",
                    "requested_qty": "1",
                    "blocked_reason": "stale_quote",
                    "fill_ts_ms": now_ms - 500,
                    "maker_fee_bps": "0.25",
                },
            },
        },
    )
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "1", "execution_mode": "take_take"},
    )
    redis_client.set_json(keys.balances_snapshot(), [])
    operator_params_schema = dict(params_schema)
    operator_params_schema["execution_mode"] = {"type": "select"}
    operator_params_defaults = dict(params_defaults)
    operator_params_defaults["execution_mode"] = "maker_hedge"

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
        params_schema=operator_params_schema,
        params_defaults=operator_params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert row["tradeable"] is False
    assert row["blocked"] is True
    assert row["maker_v4"]["operator"]["hedge_backlog"] == {
        "fill_id": "take_take:order-1",
        "side": "SELL",
        "requested_qty": "1",
        "blocked_reason": "stale_quote",
        "fill_ts_ms": now_ms - 500,
        "maker_fee_bps": 0.25,
    }


def test_signals_profile_equities_makerv4_tradeable_respects_leg_quote_health(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv4"
    now_ms = int(time.time() * 1000)
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
            "strategy_id": strategy_id,
            "state": "running",
            "bot_on": True,
            "ts_ms": now_ms,
            "maker_v4": {
                "quote_snapshot": {
                    "ts_ms": now_ms,
                    "maker_leg": {
                        "venue": "HYPERLIQUID",
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "bid": 100.0,
                        "ask": 100.1,
                        "age_ms": 12_000,
                        "feed_state": "ok",
                        "quote_state": "old",
                        "pricing_usable": False,
                        "hedge_usable": False,
                        "reason_code": "maker_quote_old",
                    },
                    "hedge_leg": {
                        "venue": "IBKR",
                        "instrument_id": "AAPL.NASDAQ",
                        "feed_state": "ok",
                        "quote_state": "fresh",
                        "pricing_usable": True,
                        "hedge_usable": True,
                    },
                    "ref_leg": {
                        "venue": "IBKR",
                        "instrument_id": "AAPL.NASDAQ",
                        "feed_state": "ok",
                        "quote_state": "fresh",
                        "pricing_usable": True,
                        "hedge_usable": True,
                    },
                },
            },
        },
    )
    operator_params_schema = dict(params_schema)
    operator_params_schema["execution_mode"] = {"type": "select"}
    operator_params_defaults = dict(params_defaults)
    operator_params_defaults["execution_mode"] = "maker_hedge"
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "1", "execution_mode": "take_take"},
    )
    redis_client.set_json(keys.balances_snapshot(), [])

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
        params_schema=operator_params_schema,
        params_defaults=operator_params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert row["tradeable"] is False
    assert row["blocked"] is True
    assert row["maker_v4"]["quote_snapshot"]["maker_leg"]["quote_state"] == "old"


def test_signals_profile_equities_makerv4_uses_published_ibkr_quote_age_budget(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv4"
    now_ms = int(time.time() * 1000)
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
            "strategy_id": strategy_id,
            "state": "running",
            "bot_on": True,
            "ts_ms": now_ms,
            "maker_v4": {
                "quote_snapshot": {
                    "ts_ms": now_ms,
                    "max_ibkr_quote_age_ms": 300_000,
                    "maker_leg": {
                        "venue": "HYPERLIQUID",
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "bid": 100.0,
                        "ask": 100.1,
                        "age_ms": 500,
                        "feed_state": "ok",
                        "quote_state": "fresh",
                        "pricing_usable": True,
                        "hedge_usable": True,
                    },
                    "hedge_leg": {
                        "venue": "IBKR",
                        "instrument_id": "AAPL.NASDAQ",
                        "bid": 100.0,
                        "ask": 100.1,
                        "age_ms": 5_000,
                        "feed_state": "ok",
                        "quote_state": "fresh",
                        "pricing_usable": True,
                        "hedge_usable": True,
                    },
                    "ref_leg": {
                        "venue": "IBKR",
                        "instrument_id": "AAPL.NASDAQ",
                        "bid": 100.0,
                        "ask": 100.1,
                        "age_ms": 5_000,
                        "feed_state": "ok",
                        "quote_state": "fresh",
                        "pricing_usable": True,
                        "hedge_usable": True,
                    },
                },
            },
        },
    )
    operator_params_schema = dict(params_schema)
    operator_params_schema["execution_mode"] = {"type": "select"}
    operator_params_defaults = dict(params_defaults)
    operator_params_defaults["execution_mode"] = "maker_hedge"
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "1", "execution_mode": "take_take"},
    )
    redis_client.set_json(keys.balances_snapshot(), [])

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
        params_schema=operator_params_schema,
        params_defaults=operator_params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert row["tradeable"] is True
    assert row["blocked"] is False
    assert row["maker_v4"]["quote_snapshot"]["ref_leg"]["quote_state"] == "fresh"
    assert row["maker_v4"]["quote_snapshot"]["hedge_leg"]["quote_state"] == "fresh"


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
    assert position_rows[0]["product_type"] == "spot"
    assert position_rows[0]["contract_type"] == "equity"
    assert position_rows[0]["display_name_short"] == "AAPL Stock"
    assert position_rows[0]["instrument_uid"] == "ibkr:equity:AAPL.NASDAQ"


def test_balances_profile_equities_does_not_degrade_on_empty_strategy_snapshots(
    monkeypatch,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_001_000)
    strategy_id = "aapl_tradexyz_maker"
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
    _seed_required_schema_keys(redis_client, flux_config)

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
            strategy_class="equities_maker",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="equities_maker",
            strategy_family="equities_maker",
            strategy_version="v1",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="equities_maker",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["degraded"] is False
    assert body["data"]["missing_required"] == []
    assert body["data"]["rows"] == []
    assert body["data"]["components"] == [
        {
            "strategy_id": strategy_id,
            "snapshot_present": True,
            "rows": 0,
            "latest_ts_ms": None,
            "age_ms": None,
            "stale": False,
            "required": True,
            "missing": False,
        },
    ]


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
    assert body["data"][0]["meta"]["strategy_id"] == flux_config.identity.strategy_id
    assert body["data"][0]["meta"]["class"] == strategy_metadata.strategy_class


def test_params_profile_equities_uses_per_strategy_param_contracts_for_split_families(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
) -> None:
    maker_id = "aapl_tradexyz_maker"
    taker_id = "aapl_tradexyz_taker"
    maker_keys = FluxRedisKeys(
        strategy_id=maker_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    taker_keys = FluxRedisKeys(
        strategy_id=taker_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(
        maker_keys.params_hash_key(),
        {
            "qty": "1.0",
            "n_orders1": "3",
        },
    )
    redis_client.set_hash_json(
        taker_keys.params_hash_key(),
        {
            "qty": "1.0",
            "bid_edge_take_bps": "6.5",
        },
    )

    def _metadata_for_strategy(strategy_id: str) -> app_module.StrategyMetadata:
        if strategy_id == maker_id:
            return app_module.StrategyMetadata(
                strategy_class="equities_maker",
                strategy_groups="equities",
                base_asset="AAPL",
                quote_asset="USD",
                param_set="equities_maker",
                strategy_family="equities_maker",
                strategy_version="v1",
            )
        if strategy_id == taker_id:
            return app_module.StrategyMetadata(
                strategy_class="equities_taker",
                strategy_groups="equities",
                base_asset="AAPL",
                quote_asset="USD",
                param_set="equities_taker",
                strategy_family="equities_taker",
                strategy_version="v1",
            )
        raise KeyError(strategy_id)

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        strategy_metadata_resolver=_metadata_for_strategy,
        profile_strategy_map={"equities": [maker_id, taker_id]},
        params_schema=EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA,
        params_defaults=EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS,
        param_set="equities_maker",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/params", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    rows_by_strategy = {row["strategy_id"]: row for row in body["data"]}
    assert rows_by_strategy[maker_id]["params"]["n_orders1"] == 3
    assert rows_by_strategy[maker_id]["meta"]["param_set"] == "equities_maker"
    assert rows_by_strategy[taker_id]["params"]["bid_edge_take_bps"] == 6.5
    assert rows_by_strategy[taker_id]["meta"]["param_set"] == "equities_taker"
    assert "bid_edge_take_bps" in rows_by_strategy[taker_id]["schema"]
    assert "n_orders1" not in rows_by_strategy[taker_id]["schema"]


def test_signals_profile_equities_keeps_external_strategy_ids_for_grouped_pairs(
    flux_config,
    redis_client,
    contract_catalog,
) -> None:
    strategy_ids = [
        "aapl_tradexyz_maker",
        "aapl_tradexyz_taker",
        "amzn_binance_perp_maker",
        "amzn_binance_perp_taker",
    ]
    for strategy_id in strategy_ids:
        _seed_required_schema_keys_for_strategy(redis_client, flux_config, strategy_id)

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=_split_equities_metadata_for_strategy(strategy_ids[0]),
        strategy_metadata_resolver=_split_equities_metadata_for_strategy,
        profile_strategy_map={"equities": strategy_ids},
        params_schema=EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA,
        params_defaults=EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS,
        param_set="equities_maker",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    rows_by_strategy = {row["id"]: row for row in body["data"]["strategies"]}
    assert set(rows_by_strategy) == set(strategy_ids)
    for strategy_id in strategy_ids:
        row = rows_by_strategy[strategy_id]
        assert row["meta"]["strategy_id"] == strategy_id
        assert "node_group_id" not in row
        assert "node_group_id" not in row["meta"]
        assert "job_id" not in row


def test_param_schema_profile_equities_strategy_id_selectors_remain_external_for_grouped_pairs(
    flux_config,
    redis_client,
    contract_catalog,
) -> None:
    tradexyz_maker_id = "aapl_tradexyz_maker"
    tradexyz_taker_id = "aapl_tradexyz_taker"
    binance_maker_id = "amzn_binance_perp_maker"
    binance_taker_id = "amzn_binance_perp_taker"

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=_split_equities_metadata_for_strategy(tradexyz_maker_id),
        strategy_metadata_resolver=_split_equities_metadata_for_strategy,
        profile_strategy_map={
            "equities": [
                tradexyz_maker_id,
                tradexyz_taker_id,
                binance_maker_id,
                binance_taker_id,
            ],
        },
        params_schema=EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA,
        params_defaults=EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS,
        param_set="equities_maker",
    )

    with app.test_client() as client:
        tradexyz_maker_response = client.get(
            "/api/v1/param-schema",
            query_string={"profile": "equities", "strategy": tradexyz_maker_id},
        )
        tradexyz_maker_body = tradexyz_maker_response.get_json()
        tradexyz_taker_response = client.get(
            "/api/v1/param-schema",
            query_string={"profile": "equities", "strategy": tradexyz_taker_id},
        )
        tradexyz_taker_body = tradexyz_taker_response.get_json()
        binance_maker_response = client.get(
            "/api/v1/param-schema",
            query_string={"profile": "equities", "strategy": binance_maker_id},
        )
        binance_maker_body = binance_maker_response.get_json()
        binance_taker_response = client.get(
            "/api/v1/param-schema",
            query_string={"profile": "equities", "strategy": binance_taker_id},
        )
        binance_taker_body = binance_taker_response.get_json()

    for response, body in (
        (tradexyz_maker_response, tradexyz_maker_body),
        (tradexyz_taker_response, tradexyz_taker_body),
        (binance_maker_response, binance_maker_body),
        (binance_taker_response, binance_taker_body),
    ):
        assert response.status_code == 200
        assert "node_group_id" not in body["data"]

    for body in (tradexyz_maker_body, binance_maker_body):
        assert body["data"]["param_set"] == "equities_maker"
        assert "n_orders1" in body["data"]["params"]
        assert "bid_edge_take_bps" not in body["data"]["params"]

    for body in (tradexyz_taker_body, binance_taker_body):
        assert body["data"]["param_set"] == "equities_taker"
        assert "bid_edge_take_bps" in body["data"]["params"]
        assert body["data"]["params_defaults"] == EQUITIES_TAKER_RUNTIME_PARAM_DEFAULTS


def test_params_profile_equities_strategy_id_selectors_remain_external_for_grouped_pairs(
    flux_config,
    redis_client,
    contract_catalog,
) -> None:
    tradexyz_maker_id = "aapl_tradexyz_maker"
    tradexyz_taker_id = "aapl_tradexyz_taker"
    binance_maker_id = "amzn_binance_perp_maker"
    binance_taker_id = "amzn_binance_perp_taker"
    tradexyz_maker_keys = FluxRedisKeys(
        strategy_id=tradexyz_maker_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    tradexyz_taker_keys = FluxRedisKeys(
        strategy_id=tradexyz_taker_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    binance_maker_keys = FluxRedisKeys(
        strategy_id=binance_maker_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    binance_taker_keys = FluxRedisKeys(
        strategy_id=binance_taker_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(
        tradexyz_maker_keys.params_hash_key(),
        {"qty": "1.0", "n_orders1": "4"},
    )
    redis_client.set_hash_json(
        tradexyz_taker_keys.params_hash_key(),
        {"qty": "1.0", "bid_edge_take_bps": "6.75"},
    )
    redis_client.set_hash_json(
        binance_maker_keys.params_hash_key(),
        {"qty": "1.0", "n_orders1": "5"},
    )
    redis_client.set_hash_json(
        binance_taker_keys.params_hash_key(),
        {"qty": "1.0", "bid_edge_take_bps": "7.25"},
    )

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=_split_equities_metadata_for_strategy(tradexyz_maker_id),
        strategy_metadata_resolver=_split_equities_metadata_for_strategy,
        profile_strategy_map={
            "equities": [
                tradexyz_maker_id,
                tradexyz_taker_id,
                binance_maker_id,
                binance_taker_id,
            ],
        },
        params_schema=EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA,
        params_defaults=EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS,
        param_set="equities_maker",
    )

    with app.test_client() as client:
        tradexyz_maker_response = client.get(
            "/api/v1/params",
            query_string={"profile": "equities", "strategy": tradexyz_maker_id},
        )
        tradexyz_maker_body = tradexyz_maker_response.get_json()
        tradexyz_taker_response = client.get(
            "/api/v1/params",
            query_string={"profile": "equities", "strategy": tradexyz_taker_id},
        )
        tradexyz_taker_body = tradexyz_taker_response.get_json()
        binance_maker_response = client.get(
            "/api/v1/params",
            query_string={"profile": "equities", "strategy": binance_maker_id},
        )
        binance_maker_body = binance_maker_response.get_json()
        binance_taker_response = client.get(
            "/api/v1/params",
            query_string={"profile": "equities", "strategy": binance_taker_id},
        )
        binance_taker_body = binance_taker_response.get_json()

    expectations = (
        (tradexyz_maker_response, tradexyz_maker_body, tradexyz_maker_id),
        (tradexyz_taker_response, tradexyz_taker_body, tradexyz_taker_id),
        (binance_maker_response, binance_maker_body, binance_maker_id),
        (binance_taker_response, binance_taker_body, binance_taker_id),
    )
    for response, body, strategy_id in expectations:
        assert response.status_code == 200
        assert [row["strategy_id"] for row in body["data"]] == [strategy_id]
        assert "node_group_id" not in body["data"][0]
        assert "job_id" not in body["data"][0]

    assert tradexyz_maker_body["data"][0]["params"]["n_orders1"] == 4
    assert binance_maker_body["data"][0]["params"]["n_orders1"] == 5
    assert tradexyz_taker_body["data"][0]["params"]["bid_edge_take_bps"] == 6.75
    assert binance_taker_body["data"][0]["params"]["bid_edge_take_bps"] == 7.25


def test_balances_profile_equities_keeps_profile_readiness_metadata_for_grouped_strategy_ids(
    flux_config,
    redis_client,
    contract_catalog,
) -> None:
    strategy_ids = ["aapl_tradexyz_maker", "aapl_tradexyz_taker"]
    for strategy_id in strategy_ids:
        _seed_required_schema_keys_for_strategy(redis_client, flux_config, strategy_id)

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=_split_equities_metadata_for_strategy(strategy_ids[0]),
        strategy_metadata_resolver=_split_equities_metadata_for_strategy,
        profile_strategy_map={"equities": strategy_ids},
        params_schema=EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA,
        params_defaults=EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS,
        param_set="equities_maker",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["degraded"] is False
    assert body["data"]["missing_required"] == []
    components_by_strategy = {row["strategy_id"]: row for row in body["data"]["components"]}
    assert set(components_by_strategy) == set(strategy_ids)
    for strategy_id in strategy_ids:
        component = components_by_strategy[strategy_id]
        assert component["required"] is True
        assert component["missing"] is False
        assert "node_group_id" not in component
        assert "job_id" not in component


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


def test_balances_profile_equities_includes_shared_hyperliquid_xyz_positions_from_portfolio_snapshot(
    monkeypatch,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_000_200)
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
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="equities",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "equities",
            "inventory_by_asset": {
                "NVDA": {
                    "portfolio_id": "equities",
                    "base_currency": "NVDA",
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
            "balances": {"rows": []},
            "accounts": {
                "totals": {
                    "account_equity_raw": 8314.466609,
                    "account_equity_display": "$8314.47",
                    "withdrawable_raw": 0.0,
                    "withdrawable_display": "$0.00",
                },
                "rows": [
                    {
                        "exchange": "hyperliquid",
                        "account": "HYPERLIQUID-master",
                        "asset": "USDE",
                        "free": "1075.37415731",
                        "total": "1075.37415731",
                        "product_type": "spot",
                        "contract_type": "cash",
                        "ts_ms": 1_700_000_000_100,
                        "source_scope": "shared_account",
                        "account_scope_id": "hyperliquid.xyz.main",
                        "source_strategy_ids": [strategy_id],
                        "strategy_id": "equities",
                    },
                    {
                        "exchange": "hyperliquid",
                        "account": "HYPERLIQUID-master",
                        "asset": "NVDA",
                        "kind": "position",
                        "instrument_id": "xyz:NVDA-USD-PERP.HYPERLIQUID",
                        "signed_qty": "-9.111",
                        "quantity": "9.111",
                        "product_type": "perp",
                        "contract_type": "perp",
                        "ts_ms": 1_700_000_000_110,
                        "source_scope": "shared_account",
                        "account_scope_id": "hyperliquid.xyz.main",
                        "source_strategy_ids": [strategy_id],
                        "strategy_id": "equities",
                    },
                    {
                        "exchange": "hyperliquid",
                        "account": "HYPERLIQUID-master",
                        "asset": "COIN",
                        "kind": "position",
                        "instrument_id": "xyz:COIN-USD-PERP.HYPERLIQUID",
                        "signed_qty": "-22.715",
                        "quantity": "22.715",
                        "product_type": "perp",
                        "contract_type": "perp",
                        "ts_ms": 1_700_000_000_120,
                        "source_scope": "shared_account",
                        "account_scope_id": "hyperliquid.xyz.main",
                        "source_strategy_ids": [strategy_id],
                        "strategy_id": "equities",
                    },
                    {
                        "exchange": "hyperliquid",
                        "account": "HYPERLIQUID-master",
                        "asset": "GOOGL",
                        "kind": "position",
                        "instrument_id": "xyz:GOOGL-USD-PERP.HYPERLIQUID",
                        "signed_qty": "-6",
                        "quantity": "6",
                        "product_type": "perp",
                        "contract_type": "perp",
                        "ts_ms": 1_700_000_000_130,
                        "source_scope": "shared_account",
                        "account_scope_id": "hyperliquid.xyz.main",
                        "source_strategy_ids": [strategy_id],
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
                exchange="hyperliquid",
                symbol="NVDA/USD",
                instrument_id="xyz:NVDA-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="COIN/USD",
                instrument_id="xyz:COIN-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="GOOGL/USD",
                instrument_id="xyz:GOOGL-USD-PERP.HYPERLIQUID",
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
    hyperliquid_position_rows = [
        row
        for row in body["data"]["rows"]
        if row["exchange"] == "hyperliquid" and row.get("kind") == "position"
    ]
    assert {row["coin"] for row in hyperliquid_position_rows} >= {"NVDA", "COIN", "GOOGL"}
    assert {row["instrument_id"] for row in hyperliquid_position_rows} >= {
        "XYZ:NVDA-USD-PERP.HYPERLIQUID",
        "XYZ:COIN-USD-PERP.HYPERLIQUID",
        "XYZ:GOOGL-USD-PERP.HYPERLIQUID",
    }
    assert {row["source_scope"] for row in hyperliquid_position_rows} == {"shared_account"}
    assert {row["account_scope_id"] for row in hyperliquid_position_rows} == {"hyperliquid.xyz.main"}
    assert all(row["product_type"] == "perp" for row in hyperliquid_position_rows)
    assert all(row["contract_type"] == "perp" for row in hyperliquid_position_rows)
    assert body["data"]["totals"]["account_equity_raw"] == pytest.approx(8314.466609)
    assert body["data"]["totals"]["withdrawable_raw"] == pytest.approx(0.0)


def test_balances_profile_equities_keeps_position_row_ids_distinct_per_shared_account(
    monkeypatch,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_000_200)
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
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="equities",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "equities",
            "inventory_by_asset": {},
            "balances": {"rows": []},
            "accounts": {
                "rows": [
                    {
                        "exchange": "hyperliquid",
                        "account": "HYPERLIQUID-master",
                        "asset": "AAPL",
                        "kind": "position",
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "signed_qty": "-1",
                        "quantity": "1",
                        "product_type": "perp",
                        "contract_type": "perp",
                        "ts_ms": 1_700_000_000_110,
                        "source_scope": "shared_account",
                        "account_scope_id": "hyperliquid.xyz.main",
                        "source_strategy_ids": [strategy_id],
                        "strategy_id": "equities",
                    },
                    {
                        "exchange": "hyperliquid",
                        "account": "HYPERLIQUID-alt",
                        "asset": "AAPL",
                        "kind": "position",
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "signed_qty": "-2",
                        "quantity": "2",
                        "product_type": "perp",
                        "contract_type": "perp",
                        "ts_ms": 1_700_000_000_120,
                        "source_scope": "shared_account",
                        "account_scope_id": "hyperliquid.xyz.alt",
                        "source_strategy_ids": [strategy_id],
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
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv3",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    position_rows = [
        row
        for row in body["data"]["rows"]
        if row["exchange"] == "hyperliquid" and row.get("kind") == "position"
    ]
    rows_by_account = {row["account"]: row for row in position_rows}

    assert sorted(rows_by_account) == ["HYPERLIQUID-alt", "HYPERLIQUID-master"]
    assert rows_by_account["HYPERLIQUID-master"]["row_id"] == (
        "equities:pos:hyperliquid:HYPERLIQUID-MASTER:XYZ:AAPL-USD-PERP.HYPERLIQUID"
    )
    assert rows_by_account["HYPERLIQUID-alt"]["row_id"] == (
        "equities:pos:hyperliquid:HYPERLIQUID-ALT:XYZ:AAPL-USD-PERP.HYPERLIQUID"
    )


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


def test_balances_profile_equities_fallback_marks_stale_shared_account_scope_status(
    monkeypatch,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_000_200)
    strategy_id = "intc_binance_perp_makerv4"
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
            execution_venue="binance_perp",
            reference_venue="ibkr",
            execution_symbol="INTC/USDT",
            reference_symbol="INTC/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": strategy_id,
                "exchange": "binance_perp",
                "account": "BINANCE_PERP-master",
                "asset": "USDT",
                "free": "1",
                "total": "1",
                "ts_ms": 1_700_000_000_150,
            },
        ],
    )
    redis_client.set_json(
        FluxRedisKeys.profile_account_projection(
            profile_id="equities",
            account_scope_id="ibkr.reference.main",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "profile_id": "equities",
            "account_scope_ids": ["ibkr.reference.main"],
            "rows": [
                {
                    "row_id": "equities:shared:ibkr.reference.main:cash:ibkr:U1234567:USD",
                    "exchange": "ibkr",
                    "account": "U1234567",
                    "asset": "USD",
                    "free": "1000",
                    "total": "1000",
                    "ts_ms": 1_700_000_000_000,
                    "source_scope": "shared_account",
                    "account_scope_id": "ibkr.reference.main",
                    "source_strategy_ids": [strategy_id],
                    "stale": True,
                    "include_in_reconciliation": False,
                },
            ],
            "totals": {
                "account_equity_raw": 1000.0,
            },
            "scope_status": [
                {
                    "account_scope_id": "ibkr.reference.main",
                    "source_scope": "shared_account",
                    "projection_status": {
                        "healthy": False,
                        "last_success_ts_ms": 1_700_000_000_000,
                        "last_attempt_ts_ms": 1_700_000_000_150,
                        "last_error_type": "TimeoutError",
                        "last_error_message": "",
                        "stale_after_ms": 15_000,
                    },
                },
            ],
            "server_ts_ms": 1_700_000_000_150,
        },
    )
    redis_client.set_json(
        FluxRedisKeys.profile_account_projection(
            profile_id="equities",
            account_scope_id="binance.futures.main",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "profile_id": "equities",
            "account_scope_ids": ["binance.futures.main"],
            "rows": [
                {
                    "row_id": "equities:shared:binance.futures.main:cash:binance_perp:BINANCE_PERP-master:USDT",
                    "exchange": "binance_perp",
                    "account": "BINANCE_PERP-master",
                    "asset": "USDT",
                    "free": "5000",
                    "total": "5000",
                    "ts_ms": 1_700_000_000_180,
                    "source_scope": "shared_account",
                    "account_scope_id": "binance.futures.main",
                    "source_strategy_ids": [strategy_id],
                    "stale": False,
                    "include_in_reconciliation": True,
                },
            ],
            "totals": {
                "account_equity_raw": 5000.0,
            },
            "scope_status": [
                {
                    "account_scope_id": "binance.futures.main",
                    "source_scope": "shared_account",
                    "projection_status": {
                        "healthy": True,
                        "last_success_ts_ms": 1_700_000_000_180,
                        "last_attempt_ts_ms": 1_700_000_000_180,
                        "last_error_type": None,
                        "last_error_message": None,
                        "stale_after_ms": 15_000,
                    },
                },
            ],
            "server_ts_ms": 1_700_000_000_180,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="binance_perp",
                symbol="INTC/USDT",
                instrument_id="INTCUSDT-PERP.BINANCE_PERP",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="INTC/USD",
                instrument_id="INTC.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="INTC",
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
    assert body["data"]["degraded"] is True
    assert body["data"]["totals"]["mv_raw"] == pytest.approx(5000.0)
    assert body["data"]["totals"]["account_equity_raw"] == pytest.approx(5000.0)
    assert sum((group.get("gross_mv") or 0.0) for group in body["data"]["risk_groups"]) == pytest.approx(5000.0)
    assert len(body["data"]["scope_status"]) == 2
    assert {
        scope["account_scope_id"]
        for scope in body["data"]["scope_status"]
    } == {"ibkr.reference.main", "binance.futures.main"}
    scope_status = {
        scope["account_scope_id"]: scope
        for scope in body["data"]["scope_status"]
    }
    assert scope_status == {
        "binance.futures.main": {
            "account_scope_id": "binance.futures.main",
            "source_scope": "shared_account",
            "projection_status": {
                "healthy": True,
                "last_success_ts_ms": 1_700_000_000_180,
                "last_attempt_ts_ms": 1_700_000_000_180,
                "last_error_type": None,
                "last_error_message": None,
                "stale_after_ms": 15_000,
            },
        },
        "ibkr.reference.main": {
            "account_scope_id": "ibkr.reference.main",
            "source_scope": "shared_account",
            "projection_status": {
                "healthy": False,
                "last_success_ts_ms": 1_700_000_000_000,
                "last_attempt_ts_ms": 1_700_000_000_150,
                "last_error_type": "TimeoutError",
                "last_error_message": "",
                "stale_after_ms": 15_000,
            },
        },
    }
    stale_row = next(
        row
        for row in body["data"]["rows"]
        if row.get("account_scope_id") == "ibkr.reference.main"
    )
    healthy_row = next(
        row
        for row in body["data"]["rows"]
        if row.get("account_scope_id") == "binance.futures.main"
    )
    assert stale_row["stale"] is True
    assert stale_row["include_in_reconciliation"] is False
    assert healthy_row["stale"] is False
    assert healthy_row["include_in_reconciliation"] is True


def test_balances_profile_equities_overlay_marks_stale_shared_account_scope_status(
    monkeypatch,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_000_200)
    strategy_id = "intc_binance_perp_makerv4"
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
            execution_venue="binance_perp",
            reference_venue="ibkr",
            execution_symbol="INTC/USDT",
            reference_symbol="INTC/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="equities",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "equities",
            "inventory_by_asset": {
                "INTC": {
                    "portfolio_id": "equities",
                    "base_currency": "INTC",
                    "global_qty_base": "0",
                    "global_qty": "0",
                    "aggregation_mode": "partial",
                    "global_qty_base_complete": False,
                    "global_qty_complete": False,
                    "ts_ms": 1_700_000_000_150,
                    "stale_after_ms": 3_000,
                    "components": [],
                    "missing_required": [],
                    "stale_required": [],
                    "null_qty_required": [],
                    "degraded": False,
                },
            },
            "balances": {"rows": []},
            "accounts": {"rows": []},
            "server_ts_ms": 1_700_000_000_190,
        },
    )
    redis_client.set_json(
        FluxRedisKeys.profile_account_projection(
            profile_id="equities",
            account_scope_id="ibkr.reference.main",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "profile_id": "equities",
            "account_scope_ids": ["ibkr.reference.main"],
            "rows": [
                {
                    "row_id": "equities:shared:ibkr.reference.main:cash:ibkr:U1234567:USD",
                    "exchange": "ibkr",
                    "account": "U1234567",
                    "asset": "USD",
                    "free": "1000",
                    "total": "1000",
                    "ts_ms": 1_700_000_000_000,
                    "source_scope": "shared_account",
                    "account_scope_id": "ibkr.reference.main",
                    "source_strategy_ids": [strategy_id],
                    "stale": True,
                    "include_in_reconciliation": False,
                },
            ],
            "totals": {"account_equity_raw": 1000.0},
            "scope_status": [
                {
                    "account_scope_id": "ibkr.reference.main",
                    "source_scope": "shared_account",
                    "projection_status": {
                        "healthy": False,
                        "last_success_ts_ms": 1_700_000_000_000,
                        "last_attempt_ts_ms": 1_700_000_000_150,
                        "last_error_type": "TimeoutError",
                        "last_error_message": "",
                        "stale_after_ms": 15_000,
                    },
                },
            ],
            "server_ts_ms": 1_700_000_000_150,
        },
    )
    redis_client.set_json(
        FluxRedisKeys.profile_account_projection(
            profile_id="equities",
            account_scope_id="binance.futures.main",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "profile_id": "equities",
            "account_scope_ids": ["binance.futures.main"],
            "rows": [
                {
                    "row_id": "equities:shared:binance.futures.main:cash:binance_perp:BINANCE_PERP-master:USDT",
                    "exchange": "binance_perp",
                    "account": "BINANCE_PERP-master",
                    "asset": "USDT",
                    "free": "5000",
                    "total": "5000",
                    "ts_ms": 1_700_000_000_180,
                    "source_scope": "shared_account",
                    "account_scope_id": "binance.futures.main",
                    "source_strategy_ids": [strategy_id],
                    "stale": False,
                    "include_in_reconciliation": True,
                },
            ],
            "totals": {"account_equity_raw": 5000.0},
            "scope_status": [
                {
                    "account_scope_id": "binance.futures.main",
                    "source_scope": "shared_account",
                    "projection_status": {
                        "healthy": True,
                        "last_success_ts_ms": 1_700_000_000_180,
                        "last_attempt_ts_ms": 1_700_000_000_180,
                        "last_error_type": None,
                        "last_error_message": None,
                        "stale_after_ms": 15_000,
                    },
                },
            ],
            "server_ts_ms": 1_700_000_000_180,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="binance_perp",
                symbol="INTC/USDT",
                instrument_id="INTCUSDT-PERP.BINANCE_PERP",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="INTC/USD",
                instrument_id="INTC.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="INTC",
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
    assert body["data"]["source"] == "portfolio_snapshot_v2"
    assert body["data"]["degraded"] is True
    assert body["data"]["totals"]["mv_raw"] == pytest.approx(5000.0)
    assert body["data"]["totals"]["account_equity_raw"] == pytest.approx(5000.0)
    assert sum((group.get("gross_mv") or 0.0) for group in body["data"]["risk_groups"]) == pytest.approx(5000.0)
    assert len(body["data"]["scope_status"]) == 2
    assert {
        scope["account_scope_id"]
        for scope in body["data"]["scope_status"]
    } == {"ibkr.reference.main", "binance.futures.main"}
    scope_status = {
        scope["account_scope_id"]: scope
        for scope in body["data"]["scope_status"]
    }
    assert scope_status == {
        "binance.futures.main": {
            "account_scope_id": "binance.futures.main",
            "source_scope": "shared_account",
            "projection_status": {
                "healthy": True,
                "last_success_ts_ms": 1_700_000_000_180,
                "last_attempt_ts_ms": 1_700_000_000_180,
                "last_error_type": None,
                "last_error_message": None,
                "stale_after_ms": 15_000,
            },
        },
        "ibkr.reference.main": {
            "account_scope_id": "ibkr.reference.main",
            "source_scope": "shared_account",
            "projection_status": {
                "healthy": False,
                "last_success_ts_ms": 1_700_000_000_000,
                "last_attempt_ts_ms": 1_700_000_000_150,
                "last_error_type": "TimeoutError",
                "last_error_message": "",
                "stale_after_ms": 15_000,
            },
        },
    }
    stale_row = next(
        row
        for row in body["data"]["rows"]
        if row.get("account_scope_id") == "ibkr.reference.main"
    )
    healthy_row = next(
        row
        for row in body["data"]["rows"]
        if row.get("account_scope_id") == "binance.futures.main"
    )
    assert stale_row["stale"] is True
    assert stale_row["include_in_reconciliation"] is False
    assert healthy_row["stale"] is False
    assert healthy_row["include_in_reconciliation"] is True


def test_balances_profile_equities_preserves_shared_account_provenance_when_makerv3_and_makerv4_coexist(
    monkeypatch,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_000_200)
    primary_strategy_id = "aapl_tradexyz_makerv3"
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
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="equities",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "equities",
            "inventory_by_asset": {},
            "balances": {"rows": []},
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
        strategy_metadata_resolver=lambda strategy_id: {
            primary_strategy_id: app_module.StrategyMetadata(
                strategy_class="maker_v3",
                strategy_groups="equities",
                base_asset="AAPL",
                quote_asset="USD",
                param_set="makerv3",
                strategy_family="maker_v3",
                strategy_version="v3",
            ),
            secondary_strategy_id: app_module.StrategyMetadata(
                strategy_class="maker_v4",
                strategy_groups="equities",
                base_asset="MSFT",
                quote_asset="USD",
                param_set="makerv4",
                strategy_family="maker_v4",
                strategy_version="v4",
            ),
        }[strategy_id],
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
    assert ibkr_cash_row["row_id"] == "equities:cash:ibkr:U1234567:USD"
    assert ibkr_cash_row["source_scope"] == "shared_account"
    assert ibkr_cash_row["account_scope_id"] == "ibkr.reference.main"
    assert ibkr_cash_row["source_strategy_ids"] == [
        primary_strategy_id,
        secondary_strategy_id,
    ]
    assert "strategy_id" not in ibkr_cash_row


def test_balances_profile_equities_keeps_shared_account_rows_outside_contract_catalog(
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
            },
            "balances": {
                "rows": [],
            },
            "accounts": {
                "rows": [
                    {
                        "exchange": "ibkr",
                        "account": "U1234567",
                        "asset": "HKD",
                        "free": "85671.33",
                        "total": "85671.33",
                        "ts_ms": 1_700_000_000_100,
                        "source_scope": "shared_account",
                        "account_scope_id": "ibkr.reference.main",
                        "source_strategy_ids": [
                            primary_strategy_id,
                            secondary_strategy_id,
                        ],
                        "strategy_id": "equities",
                    },
                    {
                        "exchange": "ibkr",
                        "account": "U1234567",
                        "asset": "F",
                        "kind": "position",
                        "instrument_id": "F.NYSE",
                        "signed_qty": "-6",
                        "quantity": "6",
                        "total": "-6",
                        "free": "-6",
                        "ts_ms": 1_700_000_000_110,
                        "source_scope": "shared_account",
                        "account_scope_id": "ibkr.reference.main",
                        "source_strategy_ids": [
                            primary_strategy_id,
                            secondary_strategy_id,
                        ],
                        "strategy_id": "equities",
                    },
                    {
                        "exchange": "ibkr",
                        "account": "U1234567",
                        "asset": "AAPL",
                        "kind": "position",
                        "instrument_id": "AAPL.NASDAQ",
                        "signed_qty": "5",
                        "quantity": "5",
                        "total": "5",
                        "free": "5",
                        "ts_ms": 1_700_000_000_120,
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
    assets = {(row["exchange"], row["asset"], row.get("kind")) for row in body["data"]["rows"]}
    assert ("ibkr", "HKD", None) in assets
    assert ("ibkr", "F", "position") in assets
    assert ("ibkr", "AAPL", "position") in assets


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


def test_alerts_profile_equities_fans_out_allowlisted_strategies_in_global_time_order(
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
    redis_client.add_stream_rows(
        primary_keys.alerts(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "a-primary",
                "level": "ERROR",
                "message": "primary venue protection",
                "alert_key": "venue_protection_circuit_breaker",
                "ts_ms": 1_000,
            },
        ],
    )
    redis_client.add_stream_rows(
        secondary_keys.alerts(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "a-secondary",
                "level": "CRITICAL",
                "message": "secondary market exit",
                "alert_key": "market_exit_fill",
                "market_exit": True,
                "ts_ms": 2_000,
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
        response = client.get("/api/v1/alerts", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["total"] == 2
    assert [row["row_id"] for row in body["data"]["rows"]] == ["a-secondary", "a-primary"]
    assert body["data"]["rows"][0]["market_exit"] is True


def test_alerts_profile_equities_merges_synthetic_pulse_alerts_with_stream_rows(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.add_stream_rows(
        primary_keys.alerts(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "a-primary",
                "level": "ERROR",
                "message": "primary venue protection",
                "alert_key": "venue_protection_circuit_breaker",
                "ts_ms": 1_000,
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
        strategy_alerts_resolver=lambda strategy_ids: {
            flux_config.identity.strategy_id: [
                {
                    "strategy_id": flux_config.identity.strategy_id,
                    "row_id": "pulse-primary",
                    "id": "pulse-primary",
                    "level": "CRITICAL",
                    "message": "Pulse runner failed: Invalid API-key",
                    "alert_key": "pulse_job_failed",
                    "ts_ms": 3_000,
                    "source": "pulse",
                },
            ],
            "strategy_02": [
                {
                    "strategy_id": "strategy_02",
                    "row_id": "pulse-secondary",
                    "id": "pulse-secondary",
                    "level": "WARNING",
                    "message": "Pulse runner restarting",
                    "alert_key": "pulse_job_restarting",
                    "ts_ms": 2_000,
                    "source": "pulse",
                },
            ],
        },
    )

    with app.test_client() as client:
        response = client.get("/api/v1/alerts", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["total"] == 3
    assert [row["row_id"] for row in body["data"]["rows"]] == [
        "pulse-primary",
        "pulse-secondary",
        "a-primary",
    ]
    assert body["data"]["rows"][0]["source"] == "pulse"


def test_trades_profile_equities_keeps_external_strategy_id_attribution_for_grouped_pairs(
    flux_config,
    redis_client,
    contract_catalog,
) -> None:
    maker_id = "aapl_tradexyz_maker"
    taker_id = "amzn_binance_perp_taker"
    maker_keys = FluxRedisKeys(
        strategy_id=maker_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    taker_keys = FluxRedisKeys(
        strategy_id=taker_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.add_stream_rows(
        maker_keys.trades_stream(),
        [
            {
                "strategy_id": maker_id,
                "row_id": "trade-maker",
                "qty": "1",
                "price": "250.0",
                "ts_ms": 1_000,
            },
        ],
    )
    redis_client.add_stream_rows(
        taker_keys.trades_stream(),
        [
            {
                "strategy_id": taker_id,
                "row_id": "trade-taker",
                "qty": "2",
                "price": "208.0",
                "ts_ms": 2_000,
            },
        ],
    )

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=_split_equities_metadata_for_strategy(maker_id),
        strategy_metadata_resolver=_split_equities_metadata_for_strategy,
        profile_strategy_map={"equities": [maker_id, taker_id]},
        params_schema=EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA,
        params_defaults=EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS,
        param_set="equities_maker",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/trades", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == ["trade-taker", "trade-maker"]
    assert {row["strategy_id"] for row in body["data"]["rows"]} == {maker_id, taker_id}
    for row in body["data"]["rows"]:
        assert "node_group_id" not in row
        assert "job_id" not in row


def test_alerts_profile_equities_keeps_external_strategy_id_attribution_for_grouped_pairs(
    flux_config,
    redis_client,
    contract_catalog,
) -> None:
    maker_id = "aapl_tradexyz_maker"
    taker_id = "amzn_binance_perp_taker"
    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=_split_equities_metadata_for_strategy(maker_id),
        strategy_metadata_resolver=_split_equities_metadata_for_strategy,
        profile_strategy_map={"equities": [maker_id, taker_id]},
        params_schema=EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA,
        params_defaults=EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS,
        param_set="equities_maker",
        strategy_alerts_resolver=lambda strategy_ids: {
            maker_id: [
                {
                    "strategy_id": maker_id,
                    "row_id": "pulse-maker",
                    "id": "pulse-maker",
                    "level": "ERROR",
                    "message": "Pulse runner failed",
                    "alert_key": "pulse_job_failed",
                    "ts_ms": 3_000,
                    "source": "pulse",
                },
            ],
            taker_id: [
                {
                    "strategy_id": taker_id,
                    "row_id": "pulse-taker",
                    "id": "pulse-taker",
                    "level": "WARNING",
                    "message": "Pulse runner restarting",
                    "alert_key": "pulse_job_restarting",
                    "ts_ms": 2_000,
                    "source": "pulse",
                },
            ],
        },
    )

    with app.test_client() as client:
        response = client.get("/api/v1/alerts", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == ["pulse-maker", "pulse-taker"]
    assert {row["strategy_id"] for row in body["data"]["rows"]} == {maker_id, taker_id}
    for row in body["data"]["rows"]:
        assert row["source"] == "pulse"
        assert "node_group_id" not in row
        assert "job_id" not in row


def test_trades_profile_equities_preserves_market_exit_rows(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.add_stream_rows(
        primary_keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "t-market-exit",
                "seq": 14,
                "ts_ms": 5_000,
                "coin": "MSFT",
                "exchange": "nasdaq",
                "side": "buy",
                "market_exit": True,
                "fill_context": "market_exit",
            },
        ],
    )

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/trades", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["rows"][0]["row_id"] == "t-market-exit"
    assert body["data"]["rows"][0]["market_exit"] is True
    assert body["data"]["rows"][0]["fill_context"] == "market_exit"


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
