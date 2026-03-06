from __future__ import annotations

from decimal import Decimal

from nautilus_trader.flux.common.portfolio_inventory import StrategyInventoryComponent
from nautilus_trader.flux.common.portfolio_inventory import aggregate_components
from nautilus_trader.flux.common.portfolio_inventory import decode_component
from nautilus_trader.flux.common.portfolio_inventory import decode_portfolio_inventory
from nautilus_trader.flux.common.portfolio_inventory import encode_component
from nautilus_trader.flux.common.portfolio_inventory import encode_portfolio_inventory


def test_component_round_trip_preserves_local_qty_and_metadata() -> None:
    component = StrategyInventoryComponent(
        strategy_id="plumeusdt_bybit_perp_makerv3",
        portfolio_id="tokenmm",
        base_currency="PLUME",
        local_qty=Decimal("36689"),
        ts_ms=1_700_000_000_000,
        stale_after_ms=3_000,
        maker_instrument_id="PLUMEUSDT-LINEAR.BYBIT",
        state="running",
    )

    decoded = decode_component(encode_component(component))

    assert decoded == component


def test_aggregate_components_sums_fresh_local_qty_and_flags_missing_required() -> None:
    payload = aggregate_components(
        portfolio_id="tokenmm",
        base_currency="PLUME",
        components={
            "plumeusdt_bybit_perp_makerv3": StrategyInventoryComponent(
                strategy_id="plumeusdt_bybit_perp_makerv3",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty=Decimal("36689"),
                ts_ms=1_000,
                maker_instrument_id="PLUMEUSDT-LINEAR.BYBIT",
                state="running",
            ),
            "plumeusdt_okx_perp_makerv3": StrategyInventoryComponent(
                strategy_id="plumeusdt_okx_perp_makerv3",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty=Decimal("-9806"),
                ts_ms=1_000,
                maker_instrument_id="PLUME-USDT-SWAP.OKX",
                state="running",
            ),
            "plumeusdt_bybit_spot_makerv3": None,
        },
        required_strategy_ids={
            "plumeusdt_bybit_perp_makerv3",
            "plumeusdt_okx_perp_makerv3",
            "plumeusdt_bybit_spot_makerv3",
        },
        now_ms_value=2_000,
    )

    assert payload["global_qty"] is None
    assert payload["degraded"] is True
    assert payload["missing_required"] == ["plumeusdt_bybit_spot_makerv3"]


def test_portfolio_payload_round_trip_keeps_global_qty() -> None:
    encoded = encode_portfolio_inventory(
        {
            "portfolio_id": "tokenmm",
            "base_currency": "PLUME",
            "global_qty": "32317.3519",
            "ts_ms": 1_000,
            "stale_after_ms": 3_000,
            "components": [],
            "missing_required": [],
            "degraded": False,
        },
    )

    decoded = decode_portfolio_inventory(encoded)

    assert decoded is not None
    assert decoded["global_qty"] == "32317.3519"
