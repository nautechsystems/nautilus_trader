from __future__ import annotations

from nautilus_trader.flux.api import StrategyMetadata
from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.api.payloads import build_legs_payload
from nautilus_trader.flux.api.payloads import build_signals_payload
from nautilus_trader.flux.common.keys import FluxRedisKeys


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
