from __future__ import annotations

from decimal import Decimal

from nautilus_trader.flux.common.portfolio_inventory import StrategyInventoryComponent
from nautilus_trader.flux.common.portfolio_inventory import aggregate_components
from nautilus_trader.flux.common.portfolio_inventory import decode_component
from nautilus_trader.flux.common.portfolio_inventory import decode_portfolio_inventory
from nautilus_trader.flux.common.portfolio_inventory import encode_component
from nautilus_trader.flux.common.portfolio_inventory import encode_portfolio_inventory
from nautilus_trader.flux.common.portfolio_inventory import normalize_inventory_by_asset


def test_component_round_trip_preserves_local_qty_and_metadata() -> None:
    component = StrategyInventoryComponent(
        strategy_id="plumeusdt_bybit_perp_makerv3",
        portfolio_id="tokenmm",
        base_currency="PLUME",
        local_qty_base=Decimal("36689"),
        local_position_qty_venue=Decimal("36689"),
        local_position_qty_base=Decimal("36689"),
        qty_conversion_status="identity",
        qty_conversion_source="instrument.info:base_exposure_mode=identity",
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
                local_qty_base=Decimal("36689"),
                local_position_qty_venue=Decimal("36689"),
                local_position_qty_base=Decimal("36689"),
                qty_conversion_status="identity",
                qty_conversion_source="instrument.info:base_exposure_mode=identity",
                ts_ms=1_000,
                maker_instrument_id="PLUMEUSDT-LINEAR.BYBIT",
                state="running",
            ),
            "plumeusdt_okx_perp_makerv3": StrategyInventoryComponent(
                strategy_id="plumeusdt_okx_perp_makerv3",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("-9806"),
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

    assert payload["global_qty_base"] is None
    assert payload["global_qty"] is None
    assert payload["aggregation_mode"] == "strict"
    assert payload["global_qty_base_complete"] is False
    assert payload["global_qty_complete"] is False
    assert payload["degraded"] is True
    assert payload["missing_required"] == ["plumeusdt_bybit_spot_makerv3"]
    assert payload["stale_required"] == []
    assert payload["null_qty_required"] == []
    assert payload["usable_component_count"] == 2
    assert payload["expected_component_count"] == 3
    component_row = next(
        row for row in payload["components"] if row["strategy_id"] == "plumeusdt_bybit_perp_makerv3"
    )
    assert component_row["local_position_qty_venue"] == "36689"
    assert component_row["local_position_qty_base"] == "36689"
    assert component_row["qty_conversion_status"] == "identity"
    assert (
        component_row["qty_conversion_source"]
        == "instrument.info:base_exposure_mode=identity"
    )


def test_aggregate_components_partial_mode_keeps_partial_sum_and_marks_incomplete() -> None:
    payload = aggregate_components(
        portfolio_id="tokenmm",
        base_currency="PLUME",
        components={
            "plumeusdt_bybit_perp_makerv3": StrategyInventoryComponent(
                strategy_id="plumeusdt_bybit_perp_makerv3",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("36689"),
                ts_ms=1_000,
                maker_instrument_id="PLUMEUSDT-LINEAR.BYBIT",
                state="running",
            ),
            "plumeusdt_okx_perp_makerv3": StrategyInventoryComponent(
                strategy_id="plumeusdt_okx_perp_makerv3",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("-9806"),
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
        aggregation_mode="partial",
    )

    assert payload["global_qty_base"] == "26883"
    assert payload["global_qty"] == "26883"
    assert payload["aggregation_mode"] == "partial"
    assert payload["global_qty_base_complete"] is False
    assert payload["global_qty_complete"] is False
    assert payload["degraded"] is True
    assert payload["missing_required"] == ["plumeusdt_bybit_spot_makerv3"]
    assert payload["stale_required"] == []
    assert payload["null_qty_required"] == []
    assert payload["usable_component_count"] == 2
    assert payload["expected_component_count"] == 3


def test_aggregate_components_keeps_multiple_strategy_contributors_in_one_stock_bucket() -> None:
    payload = aggregate_components(
        portfolio_id="equities",
        base_currency="PLTR",
        components={
            "pltr_tradexyz_makerv4": StrategyInventoryComponent(
                strategy_id="pltr_tradexyz_makerv4",
                portfolio_id="equities",
                base_currency="PLTR",
                local_qty_base=Decimal("10"),
                ts_ms=1_000,
                maker_instrument_id="xyz:PLTR-USD-PERP.HYPERLIQUID",
                state="running",
            ),
            "pltr_binance_perp_makerv4": StrategyInventoryComponent(
                strategy_id="pltr_binance_perp_makerv4",
                portfolio_id="equities",
                base_currency="PLTR",
                local_qty_base=Decimal("-4"),
                ts_ms=1_000,
                maker_instrument_id="PLTRUSDT-PERP.BINANCE_PERP",
                state="running",
            ),
        },
        required_strategy_ids={
            "pltr_tradexyz_makerv4",
            "pltr_binance_perp_makerv4",
        },
        now_ms_value=2_000,
    )

    assert payload["base_currency"] == "PLTR"
    assert payload["global_qty_base"] == "6"
    assert payload["global_qty"] == "6"
    assert payload["usable_component_count"] == 2
    assert payload["expected_component_count"] == 2
    assert [row["strategy_id"] for row in payload["components"]] == [
        "pltr_binance_perp_makerv4",
        "pltr_tradexyz_makerv4",
    ]
    assert {row["maker_instrument_id"] for row in payload["components"]} == {
        "PLTRUSDT-PERP.BINANCE_PERP",
        "xyz:PLTR-USD-PERP.HYPERLIQUID",
    }


def test_aggregate_components_flags_stale_and_null_required_components_separately() -> None:
    payload = aggregate_components(
        portfolio_id="tokenmm",
        base_currency="PLUME",
        components={
            "fresh": StrategyInventoryComponent(
                strategy_id="fresh",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("5"),
                ts_ms=1_000,
                state="running",
            ),
            "stale": StrategyInventoryComponent(
                strategy_id="stale",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("7"),
                ts_ms=1_000,
                stale_after_ms=10,
                state="running",
            ),
            "null_qty": StrategyInventoryComponent(
                strategy_id="null_qty",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=None,
                ts_ms=1_000,
                state="running",
            ),
        },
        required_strategy_ids={"fresh", "stale", "null_qty"},
        now_ms_value=2_000,
        aggregation_mode="partial",
    )

    assert payload["global_qty_base"] == "5"
    assert payload["global_qty"] == "5"
    assert payload["global_qty_base_complete"] is False
    assert payload["global_qty_complete"] is False
    assert payload["missing_required"] == []
    assert payload["stale_required"] == ["stale"]
    assert payload["null_qty_required"] == ["null_qty"]


def test_portfolio_payload_round_trip_keeps_global_qty_base() -> None:
    encoded = encode_portfolio_inventory(
        {
            "portfolio_id": "tokenmm",
            "base_currency": "PLUME",
            "global_qty_base": "32317.3519",
            "global_qty": "32317.3519",
            "aggregation_mode": "strict",
            "global_qty_base_complete": True,
            "global_qty_complete": True,
            "usable_component_count": 2,
            "expected_component_count": 2,
            "ts_ms": 1_000,
            "stale_after_ms": 3_000,
            "components": [],
            "missing_required": [],
            "stale_required": [],
            "null_qty_required": [],
            "degraded": False,
        },
    )

    decoded = decode_portfolio_inventory(encoded)

    assert decoded is not None
    assert decoded["global_qty_base"] == "32317.3519"
    assert decoded["global_qty"] == "32317.3519"


def test_aggregate_components_strict_mode_keeps_global_qty_null_when_required_component_is_stale() -> None:
    payload = aggregate_components(
        portfolio_id="tokenmm",
        base_currency="PLUME",
        components={
            "strategy_a": StrategyInventoryComponent(
                strategy_id="strategy_a",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("10"),
                ts_ms=1_000,
                stale_after_ms=2_000,
                state="running",
            ),
            "strategy_b": StrategyInventoryComponent(
                strategy_id="strategy_b",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("5"),
                ts_ms=1_000,
                stale_after_ms=100,
                state="running",
            ),
        },
        required_strategy_ids={"strategy_a", "strategy_b"},
        now_ms_value=2_000,
    )

    assert payload["aggregation_mode"] == "strict"
    assert payload["global_qty_base"] is None
    assert payload["global_qty"] is None
    assert payload["global_qty_base_complete"] is False
    assert payload["global_qty_complete"] is False
    assert payload["usable_component_count"] == 1
    assert payload["expected_component_count"] == 2
    assert payload["missing_required"] == []
    assert payload["stale_required"] == ["strategy_b"]
    assert payload["null_qty_required"] == []
    assert payload["degraded"] is True


def test_aggregate_components_partial_mode_sums_usable_qty_and_reports_required_diagnostics() -> None:
    payload = aggregate_components(
        portfolio_id="tokenmm",
        base_currency="PLUME",
        components={
            "strategy_a": StrategyInventoryComponent(
                strategy_id="strategy_a",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("10"),
                ts_ms=1_000,
                stale_after_ms=2_000,
                state="running",
            ),
            "strategy_b": StrategyInventoryComponent(
                strategy_id="strategy_b",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("5"),
                ts_ms=1_000,
                stale_after_ms=100,
                state="running",
            ),
            "strategy_c": StrategyInventoryComponent(
                strategy_id="strategy_c",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=None,
                ts_ms=1_900,
                stale_after_ms=2_000,
                state="running",
            ),
            "strategy_d": None,
            "strategy_e": StrategyInventoryComponent(
                strategy_id="strategy_e",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("2"),
                ts_ms=1_900,
                stale_after_ms=2_000,
                state="running",
            ),
        },
        required_strategy_ids={"strategy_a", "strategy_b", "strategy_c", "strategy_d"},
        now_ms_value=2_000,
        aggregation_mode="partial",
    )

    assert payload["aggregation_mode"] == "partial"
    assert payload["global_qty_base"] == "12"
    assert payload["global_qty"] == "12"
    assert payload["global_qty_base_complete"] is False
    assert payload["global_qty_complete"] is False
    assert payload["usable_component_count"] == 2
    assert payload["expected_component_count"] == 5
    assert payload["missing_required"] == ["strategy_d"]
    assert payload["stale_required"] == ["strategy_b"]
    assert payload["null_qty_required"] == ["strategy_c"]
    assert payload["degraded"] is True


def test_aggregate_components_deduplicates_shared_quantity_groups_without_hiding_rows() -> None:
    payload = aggregate_components(
        portfolio_id="equities",
        base_currency="AAPL",
        components={
            "aapl_tradexyz_maker": StrategyInventoryComponent(
                strategy_id="aapl_tradexyz_maker",
                portfolio_id="equities",
                base_currency="AAPL",
                local_qty_base=Decimal("10"),
                ts_ms=1_000,
                maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
                state="running",
            ),
            "aapl_tradexyz_taker": StrategyInventoryComponent(
                strategy_id="aapl_tradexyz_taker",
                portfolio_id="equities",
                base_currency="AAPL",
                local_qty_base=Decimal("10"),
                ts_ms=1_100,
                maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
                state="running",
            ),
        },
        required_strategy_ids={"aapl_tradexyz_maker", "aapl_tradexyz_taker"},
        now_ms_value=2_000,
        shared_quantity_groups={
            "aapl_tradexyz_maker": "AAPL|hyperliquid.xyz.main|xyz:AAPL-USD-PERP.HYPERLIQUID",
            "aapl_tradexyz_taker": "AAPL|hyperliquid.xyz.main|xyz:AAPL-USD-PERP.HYPERLIQUID",
        },
    )

    assert payload["global_qty_base"] == "10"
    assert payload["global_qty"] == "10"
    assert payload["global_qty_base_complete"] is True
    assert payload["global_qty_complete"] is True
    assert payload["usable_component_count"] == 1
    assert payload["expected_component_count"] == 2
    assert payload["missing_required"] == []
    assert payload["stale_required"] == []
    assert payload["null_qty_required"] == []
    assert payload["shared_quantity_conflict_strategy_ids"] == []
    assert payload["degraded"] is False
    assert [row["strategy_id"] for row in payload["components"]] == [
        "aapl_tradexyz_maker",
        "aapl_tradexyz_taker",
    ]
    assert [row["local_qty_base"] for row in payload["components"]] == ["10", "10"]


def test_aggregate_components_flags_conflicting_shared_quantity_groups() -> None:
    payload = aggregate_components(
        portfolio_id="equities",
        base_currency="AAPL",
        components={
            "aapl_tradexyz_maker": StrategyInventoryComponent(
                strategy_id="aapl_tradexyz_maker",
                portfolio_id="equities",
                base_currency="AAPL",
                local_qty_base=Decimal("10"),
                ts_ms=1_000,
                maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
                state="running",
            ),
            "aapl_tradexyz_taker": StrategyInventoryComponent(
                strategy_id="aapl_tradexyz_taker",
                portfolio_id="equities",
                base_currency="AAPL",
                local_qty_base=Decimal("4"),
                ts_ms=1_100,
                maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
                state="running",
            ),
        },
        required_strategy_ids={"aapl_tradexyz_maker", "aapl_tradexyz_taker"},
        now_ms_value=2_000,
        shared_quantity_groups={
            "aapl_tradexyz_maker": "AAPL|hyperliquid.xyz.main|xyz:AAPL-USD-PERP.HYPERLIQUID",
            "aapl_tradexyz_taker": "AAPL|hyperliquid.xyz.main|xyz:AAPL-USD-PERP.HYPERLIQUID",
        },
    )

    assert payload["global_qty_base"] is None
    assert payload["global_qty"] is None
    assert payload["global_qty_base_complete"] is False
    assert payload["global_qty_complete"] is False
    assert payload["missing_required"] == []
    assert payload["stale_required"] == []
    assert payload["null_qty_required"] == []
    assert payload["shared_quantity_conflict_strategy_ids"] == [
        "aapl_tradexyz_maker",
        "aapl_tradexyz_taker",
    ]
    assert payload["degraded"] is True


def test_normalize_inventory_by_asset_canonicalizes_asset_keys() -> None:
    normalized = normalize_inventory_by_asset(
        {
            " aapl ": {"global_qty_base": "10"},
            "msft": {"base_currency": "MSFT", "global_qty_base": "5"},
        },
    )

    assert list(normalized) == ["AAPL", "MSFT"]
    assert normalized["AAPL"]["base_currency"] == "AAPL"
    assert normalized["MSFT"]["base_currency"] == "MSFT"


def test_aggregate_components_nets_same_stock_multivenue_routes_and_keeps_route_metadata() -> None:
    payload = aggregate_components(
        portfolio_id="equities",
        base_currency="PLTR",
        components={
            "pltr_tradexyz_makerv4": StrategyInventoryComponent(
                strategy_id="pltr_tradexyz_makerv4",
                portfolio_id="equities",
                base_currency="PLTR",
                local_qty_base=Decimal("10"),
                local_position_qty_venue=Decimal("10"),
                local_position_qty_base=Decimal("10"),
                qty_conversion_status="identity",
                qty_conversion_source="generic:multiplier=1",
                ts_ms=1_000,
                maker_instrument_id="xyz:PLTR-USD-PERP.HYPERLIQUID",
                state="running",
            ),
            "pltr_binance_perp_makerv4": StrategyInventoryComponent(
                strategy_id="pltr_binance_perp_makerv4",
                portfolio_id="equities",
                base_currency="PLTR",
                local_qty_base=Decimal("5"),
                local_position_qty_venue=Decimal("80"),
                local_position_qty_base=Decimal("5"),
                qty_conversion_status="converted",
                qty_conversion_source="generic:multiplier=0.0625",
                ts_ms=1_000,
                maker_instrument_id="PLTRUSDT-PERP.BINANCE_PERP",
                state="running",
            ),
        },
        required_strategy_ids={"pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"},
        now_ms_value=2_000,
    )

    assert payload["global_qty_base"] == "15"
    assert payload["global_qty"] == "15"
    assert payload["global_qty_base_complete"] is True
    assert payload["global_qty_complete"] is True
    assert payload["degraded"] is False
    assert payload["usable_component_count"] == 2
    assert payload["expected_component_count"] == 2
    assert [row["strategy_id"] for row in payload["components"]] == [
        "pltr_binance_perp_makerv4",
        "pltr_tradexyz_makerv4",
    ]
    binance_row = next(
        row for row in payload["components"] if row["strategy_id"] == "pltr_binance_perp_makerv4"
    )
    assert binance_row["local_position_qty_venue"] == "80"
    assert binance_row["local_position_qty_base"] == "5"
    assert binance_row["qty_conversion_status"] == "converted"
    assert binance_row["qty_conversion_source"] == "generic:multiplier=0.0625"
    assert binance_row["maker_instrument_id"] == "PLTRUSDT-PERP.BINANCE_PERP"
