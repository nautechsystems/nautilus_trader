from __future__ import annotations

from nautilus_trader.flux.api import StrategyMetadata
from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.api.payloads import build_legs_payload
from nautilus_trader.flux.api.payloads import build_signals_payload
from nautilus_trader.flux.common.keys import FluxRedisKeys
import pytest


def test_build_signals_payload_projects_canonical_inventory_fields_from_strategy_state(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="PLUME",
        quote_asset="USDT",
    )
    legs = build_legs_payload(
        contracts=contract_catalog,
        market_rows={},
        now_ms_value=1_700_000_001_000,
    )

    payload = build_signals_payload(
        strategy_id="plumeusdt_bybit_perp_makerv3",
        metadata=metadata,
        state={
            "bot_on": True,
            "managed_orders": 1,
            "state": "running",
            "ts_ms": 1_700_000_000_000,
            "pricing_adjustments": [{"type": "inventory_skew"}],
            "pricing_debug": {
                "skew": {
                    "local_position_qty_venue": "197764",
                    "local_position_qty_base": "197764",
                    "local_position_qty_complete": True,
                    "local_position_qty_conversion_status": "identity",
                    "local_position_qty_conversion_source": "generic:multiplier=1",
                    "local_inventory_qty_base": "197764",
                    "local_inventory_qty": "197764",
                    "global_inventory_qty_base": "98658.50735752",
                    "global_inventory_qty": "98658.50735752",
                    "global_inventory_qty_base_complete": False,
                    "global_inventory_qty_complete": False,
                    "global_inventory_aggregation_mode": "partial",
                },
            },
        },
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    assert payload["position_qty_venue"] == 197764.0
    assert payload["position_qty_base"] == 197764.0
    assert payload["local_qty_base"] == 197764.0
    assert payload["local_qty"] == 197764.0
    assert payload["global_qty_base"] == 98658.50735752
    assert payload["global_qty"] == 98658.50735752
    assert payload["global_qty_base_complete"] is False
    assert payload["global_qty_complete"] is False
    assert payload["aggregation_mode"] == "partial"
    assert payload["qty_conversion_status"] == "identity"
    assert payload["qty_conversion_source"] == "generic:multiplier=1"


def test_signals_endpoint_projects_canonical_inventory_fields_from_strategy_state(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.state(),
        {
            "bot_on": True,
            "managed_orders": 1,
            "state": "running",
            "ts_ms": 1_700_000_000_000,
            "pricing_adjustments": [{"type": "inventory_skew"}],
            "pricing_debug": {
                "skew": {
                    "local_position_qty_venue": "197764",
                    "local_position_qty_base": "197764",
                    "local_position_qty_complete": True,
                    "local_position_qty_conversion_status": "identity",
                    "local_position_qty_conversion_source": "generic:multiplier=1",
                    "local_inventory_qty_base": "197764",
                    "local_inventory_qty": "197764",
                    "global_inventory_qty_base": "98658.50735752",
                    "global_inventory_qty": "98658.50735752",
                    "global_inventory_qty_base_complete": False,
                    "global_inventory_qty_complete": False,
                    "global_inventory_aggregation_mode": "partial",
                },
            },
        },
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

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals")
        body = response.get_json()

    assert response.status_code == 200
    strategy = body["data"]["strategies"][0]
    assert strategy["position_qty_venue"] == 197764.0
    assert strategy["position_qty_base"] == 197764.0
    assert strategy["local_qty_base"] == 197764.0
    assert strategy["local_qty"] == 197764.0
    assert strategy["global_qty_base"] == 98658.50735752
    assert strategy["global_qty"] == 98658.50735752
    assert strategy["global_qty_base_complete"] is False
    assert strategy["global_qty_complete"] is False
    assert strategy["aggregation_mode"] == "partial"
    assert strategy["qty_conversion_status"] == "identity"
    assert strategy["qty_conversion_source"] == "generic:multiplier=1"


def test_build_signals_payload_exposes_canonical_signed_skew_from_strategy_state(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="PLUME",
        quote_asset="USDT",
    )
    legs = build_legs_payload(
        contracts=contract_catalog,
        market_rows={},
        now_ms_value=1_700_000_001_000,
    )

    payload = build_signals_payload(
        strategy_id="plumeusdt_okx_perp_makerv3",
        metadata=metadata,
        state={
            "bot_on": True,
            "managed_orders": 1,
            "state": "running",
            "ts_ms": 1_700_000_000_000,
                "pricing_adjustments": [
                    {
                        "type": "inventory_skew",
                        "skew_bps_signed": -3.86,
                        "inv_skew": 999.0,
                        "eff_bid_edge_bps": "13.86",
                        "eff_ask_edge_bps": "6.14",
                    }
                ],
        },
        fv_row={"fv": 0.012285},
        params={"qty": 10.0},
        balances=[],
        legs=legs,
    )

    adjustment = payload["pricing_adjustments"][0]
    assert adjustment["type"] == "inventory_skew"
    assert adjustment["skew_bps_signed"] == -3.86
    assert adjustment["inv_skew"] == 999.0


def test_build_signals_payload_keeps_spread_contract_aligned_with_makerv3_quote_snapshot(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="ABC",
        quote_asset="USDT",
    )
    legs = build_legs_payload(
        contracts=contract_catalog,
        market_rows={
            "venue_a:ABC/USDT": {"bid": 103.0, "ask": 105.0, "ts_ms": 1_700_000_000_100},
            "venue_b:ABC/USDT": {"bid": 103.0, "ask": 105.0, "ts_ms": 1_700_000_000_100},
        },
        now_ms_value=1_700_000_001_000,
    )

    payload = build_signals_payload(
        strategy_id="strategy_01",
        metadata=metadata,
        state={
            "bot_on": False,
            "managed_orders": 0,
            "state": "bot_off",
            "ts_ms": 1_700_000_000_000,
            "spread_net_bps": 0.0,
            "pricing_debug": {
                "pricing": {
                    "place_bid": "100.0",
                    "place_ask": "102.0",
                    "ref_bid": "103.0",
                    "ref_ask": "105.0",
                },
            },
            "maker_v3": {
                "quote_snapshot": {
                    "ts_ms": 1_700_000_000_000,
                    "mode": "OFF",
                    "reason": "bot_off",
                    "place_bid": 100.0,
                    "place_ask": 102.0,
                    "ref_bid": 103.0,
                    "ref_ask": 105.0,
                },
            },
        },
        fv_row={"fv": 104.0},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    quote_snapshot = payload["maker_v3"]["quote_snapshot"]
    expected_spread_bps = ((101.0 - 104.0) / 104.0) * 10_000

    assert quote_snapshot["place_bid"] == 100.0
    assert quote_snapshot["place_ask"] == 102.0
    assert quote_snapshot["ref_bid"] == 103.0
    assert quote_snapshot["ref_ask"] == 105.0
    assert payload["spread_net_bps"] == pytest.approx(expected_spread_bps, abs=0.1)
    assert payload["decision_edge_bps"] == pytest.approx(expected_spread_bps, abs=0.1)


def test_build_signals_payload_preserves_explicit_makerv3_quote_snapshot_epoch(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="ABC",
        quote_asset="USDT",
    )
    legs = build_legs_payload(
        contracts=contract_catalog,
        market_rows={
            "venue_a:ABC/USDT": {"bid": 103.0, "ask": 105.0, "ts_ms": 1_700_000_009_000},
            "venue_b:ABC/USDT": {"bid": 103.0, "ask": 105.0, "ts_ms": 1_700_000_009_000},
        },
        now_ms_value=1_700_000_010_000,
    )

    payload = build_signals_payload(
        strategy_id="strategy_01",
        metadata=metadata,
        state={
            "bot_on": False,
            "managed_orders": 0,
            "state": "bot_off",
            "ts_ms": 1_700_000_010_000,
            "maker_v3": {
                "quote_snapshot": {
                    "ts_ms": 1_700_000_000_000,
                    "mode": "STALE",
                    "reason": "bot_off",
                    "maker_top_bid": 99.0,
                    "maker_top_ask": 100.0,
                    "place_bid": 98.5,
                    "place_ask": 100.5,
                    "ref_bid": 101.0,
                    "ref_ask": 102.0,
                    "skew_bps_signed": -12.5,
                },
            },
        },
        fv_row={"fv": 101.5},
        params={"qty": 1.0},
        balances=[],
        legs=legs,
    )

    quote_snapshot = payload["maker_v3"]["quote_snapshot"]
    assert quote_snapshot["ts_ms"] == 1_700_000_000_000
    assert quote_snapshot["mode"] == "STALE"
    assert quote_snapshot["maker_top_bid"] == 99.0
    assert quote_snapshot["maker_top_ask"] == 100.0
    assert quote_snapshot["place_bid"] == 98.5
    assert quote_snapshot["place_ask"] == 100.5
    assert quote_snapshot["ref_bid"] == 101.0
    assert quote_snapshot["ref_ask"] == 102.0
    assert quote_snapshot["skew_bps_signed"] == -12.5


def test_build_signals_payload_marks_stale_strategy_state_at_top_level(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="ABC",
        quote_asset="USDT",
    )
    legs = build_legs_payload(
        contracts=contract_catalog,
        market_rows={
            "venue_a:ABC/USDT": {"bid": 103.0, "ask": 105.0, "ts_ms": 1_700_000_039_000},
            "venue_b:ABC/USDT": {"bid": 103.0, "ask": 105.0, "ts_ms": 1_700_000_039_000},
        },
        now_ms_value=1_700_000_040_000,
    )

    payload = build_signals_payload(
        strategy_id="strategy_01",
        metadata=metadata,
        state={
            "bot_on": False,
            "managed_orders": 2,
            "state": "bot_off",
            "ts_ms": 1_700_000_000_000,
            "pricing_adjustments": [{"type": "inventory_skew", "skew_bps_signed": -12.5}],
            "maker_v3": {
                "quote_snapshot": {
                    "ts_ms": 1_700_000_000_000,
                    "mode": "OFF",
                    "reason": "bot_off",
                    "place_bid": 98.5,
                    "place_ask": 100.5,
                    "ref_bid": 101.0,
                    "ref_ask": 102.0,
                    "skew_bps_signed": -12.5,
                },
            },
        },
        fv_row={"fv": 101.5},
        params={"qty": 1.0},
        balances=[],
        legs=legs,
    )

    assert payload["debug"]["md_health"]["state_stale"] is True
    assert payload["mode"] == "STALE"
    assert payload["reason"] == "stale_state"
    assert payload["skew_bps_signed"] is None


def test_build_signals_payload_marks_timestamp_less_pricing_state_stale(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="ABC",
        quote_asset="USDT",
    )

    payload = build_signals_payload(
        strategy_id="strategy_01",
        metadata=metadata,
        state={
            "bot_on": False,
            "managed_orders": 0,
            "state": "bot_off",
            "pricing_adjustments": [{"type": "inventory_skew", "skew_bps_signed": 7.25}],
        },
        fv_row={"fv": 101.5},
        params={"qty": 1.0},
        balances=[],
        legs=build_legs_payload(
            contracts=contract_catalog,
            market_rows={},
            now_ms_value=1_700_000_040_000,
        ),
    )

    assert payload["ts_ms"] is None
    assert payload["debug"]["md_health"]["state_stale"] is True
    assert payload["mode"] == "STALE"
    assert payload["reason"] == "stale_state"
    assert payload["skew_bps_signed"] is None


def test_build_signals_payload_marks_explicit_quote_snapshot_stale_without_leg_timestamps(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="ABC",
        quote_asset="USDT",
    )

    payload = build_signals_payload(
        strategy_id="strategy_01",
        metadata=metadata,
        state={
            "bot_on": True,
            "managed_orders": 2,
            "state": "running",
            "ts_ms": 1_700_000_000_000,
            "maker_v3": {
                "quote_snapshot": {
                    "ts_ms": 1_700_000_000_000,
                    "mode": "ON",
                    "reason": "quoting",
                    "place_bid": 98.5,
                    "place_ask": 100.5,
                    "ref_bid": 101.0,
                    "ref_ask": 102.0,
                },
            },
        },
        fv_row={"fv": 101.5},
        params={"qty": 1.0},
        balances=[],
        legs=build_legs_payload(
            contracts=contract_catalog,
            market_rows={},
            now_ms_value=1_700_000_040_000,
        ),
    )

    assert payload["debug"]["md_health"]["state_stale"] is True
    assert payload["debug"]["md_health"]["signal_state_age_ms"] == 40_000
    assert payload["mode"] == "STALE"
    assert payload["reason"] == "stale_state"
    assert payload["managed_orders"] == 0


def test_build_signals_payload_promotes_operator_quote_fields_to_top_level(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="ABC",
        quote_asset="USDT",
    )
    legs = build_legs_payload(
        contracts=contract_catalog,
        market_rows={},
        now_ms_value=1_700_000_001_000,
    )

    payload = build_signals_payload(
        strategy_id="strategy_01",
        metadata=metadata,
        state={
            "bot_on": False,
            "managed_orders": 0,
            "state": "bot_off",
            "ts_ms": 1_700_000_000_000,
            "pricing_adjustments": [
                {
                    "type": "inventory_skew",
                    "skew_bps_signed": -12.5,
                },
            ],
            "maker_v3": {
                "quote_snapshot": {
                    "ts_ms": 1_700_000_000_123,
                    "mode": "OFF",
                    "reason": "bot_off",
                    "skew_bps_signed": -12.5,
                    "place_bid": 98.5,
                    "place_ask": 100.5,
                    "ref_bid": 101.0,
                    "ref_ask": 102.0,
                },
            },
        },
        fv_row={"fv": 101.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    assert payload["ts_ms"] == 1_700_000_000_123
    assert payload["mode"] == "OFF"
    assert payload["reason"] == "bot_off"
    assert payload["skew_bps_signed"] == -12.5
