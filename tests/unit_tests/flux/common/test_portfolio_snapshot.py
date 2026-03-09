from __future__ import annotations

from decimal import Decimal

from nautilus_trader.flux.common.portfolio_inventory import StrategyInventoryComponent
from nautilus_trader.flux.common.portfolio_snapshot import build_portfolio_snapshot
from nautilus_trader.flux.common.portfolio_snapshot import decode_portfolio_snapshot
from nautilus_trader.flux.common.portfolio_snapshot import encode_portfolio_snapshot


def test_build_portfolio_snapshot_partial_mode_includes_inventory_and_merged_balances() -> None:
    snapshot = build_portfolio_snapshot(
        portfolio_id="tokenmm",
        base_currency="PLUME",
        inventory_components={
            "strategy_a": StrategyInventoryComponent(
                strategy_id="strategy_a",
                portfolio_id="tokenmm",
                base_currency="PLUME",
                local_qty_base=Decimal("10"),
                ts_ms=1_000,
                stale_after_ms=2_000,
                state="running",
            ),
            "strategy_b": None,
        },
        balance_rows_by_strategy={
            "strategy_a": [
                {
                    "strategy_id": "strategy_a",
                    "exchange": "bybit",
                    "asset": "PLUME",
                    "account": "trading",
                    "total": "10",
                    "ts_ms": 1_900,
                    "mark_raw": 1.5,
                    "mv_raw": 15.0,
                },
            ],
            "strategy_b": [
                {
                    "strategy_id": "strategy_b",
                    "exchange": "okx",
                    "asset": "USDT",
                    "account": "trading",
                    "total": "3",
                    "ts_ms": 1_800,
                    "mark_raw": 1.0,
                    "mv_raw": 3.0,
                },
            ],
        },
        required_strategy_ids={"strategy_a", "strategy_b"},
        aggregation_mode="partial",
        now_ms_value=2_000,
    )

    assert snapshot["inventory"]["global_qty_base"] == "10"
    assert snapshot["inventory"]["global_qty"] == "10"
    assert snapshot["inventory"]["aggregation_mode"] == "partial"
    assert snapshot["inventory"]["global_qty_base_complete"] is False
    assert snapshot["inventory"]["global_qty_complete"] is False
    assert snapshot["inventory"]["missing_required"] == ["strategy_b"]
    assert snapshot["components"] == snapshot["inventory"]["components"]
    assert all(row["strategy_id"] == "tokenmm" for row in snapshot["balances"]["rows"])
    assert snapshot["balances"]["totals"]["mv_raw"] == 18.0


def test_build_portfolio_snapshot_totals_match_netted_position_valuation() -> None:
    snapshot = build_portfolio_snapshot(
        portfolio_id="tokenmm",
        base_currency="PLUME",
        inventory_components={},
        balance_rows_by_strategy={
            "strategy_a": [
                {
                    "strategy_id": "strategy_a",
                    "exchange": "bybit",
                    "kind": "position",
                    "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                    "signed_qty": "10",
                    "quantity": "10",
                    "mark_raw": 2.0,
                    "mv_raw": 20.0,
                },
            ],
            "strategy_b": [
                {
                    "strategy_id": "strategy_b",
                    "exchange": "bybit",
                    "kind": "position",
                    "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                    "signed_qty": "-5",
                    "quantity": "5",
                    "mark_raw": 2.0,
                    "mv_raw": -10.0,
                },
            ],
        },
        required_strategy_ids=set(),
        now_ms_value=2_000,
    )

    assert len(snapshot["balances"]["rows"]) == 1
    row = snapshot["balances"]["rows"][0]
    assert row["strategy_id"] == "tokenmm"
    assert row["exchange"] == "bybit"
    assert row["kind"] == "position"
    assert row["instrument_id"] == "PLUMEUSDT-LINEAR.BYBIT"
    assert row["signed_qty"] == "5"
    assert row["quantity"] == "5"
    assert row["side"] == "LONG"
    assert row["mark_raw"] == 2.0
    assert row["mv_raw"] == 10.0
    assert snapshot["balances"]["totals"]["mv_raw"] == 10.0
    assert snapshot["balances"]["totals"]["mv_display"] == "$10.00"


def test_portfolio_snapshot_round_trip_preserves_strict_inventory_metadata() -> None:
    encoded = encode_portfolio_snapshot(
        build_portfolio_snapshot(
            portfolio_id="tokenmm",
            base_currency="PLUME",
            inventory_components={
                "strategy_a": StrategyInventoryComponent(
                    strategy_id="strategy_a",
                    portfolio_id="tokenmm",
                    base_currency="PLUME",
                    local_qty_base=Decimal("10"),
                    ts_ms=1_000,
                    stale_after_ms=2_000,
                    state="running",
                ),
                "strategy_b": None,
            },
            balance_rows_by_strategy={},
            required_strategy_ids={"strategy_a", "strategy_b"},
            now_ms_value=2_000,
        ),
    )

    decoded = decode_portfolio_snapshot(encoded)

    assert decoded is not None
    assert decoded["inventory"]["aggregation_mode"] == "strict"
    assert decoded["inventory"]["global_qty_base"] is None
    assert decoded["inventory"]["global_qty"] is None
    assert decoded["inventory"]["global_qty_base_complete"] is False
    assert decoded["inventory"]["global_qty_complete"] is False
    assert decoded["balances"] == {"rows": [], "totals": {"mv_raw": 0.0, "mv_display": "$0.00"}}


def test_build_portfolio_snapshot_merges_same_account_stable_cash_across_product_scopes() -> None:
    snapshot = build_portfolio_snapshot(
        portfolio_id="tokenmm",
        base_currency="PLUME",
        inventory_components={},
        balance_rows_by_strategy={
            "plumeusdt_bitget_spot_makerv3": [
                {
                    "strategy_id": "plumeusdt_bitget_spot_makerv3",
                    "exchange": "bitget",
                    "account_id": "BITGET-001",
                    "asset": "USDT",
                    "free": "500",
                    "locked": "0",
                    "total": "500",
                    "ts_ms": 1_700_000_000_000,
                    "row_id": "plumeusdt_bitget_spot_makerv3:cash:0",
                    "product_type": "spot",
                },
            ],
            "plumeusdt_bitget_perp_makerv3": [
                {
                    "strategy_id": "plumeusdt_bitget_perp_makerv3",
                    "exchange": "bitget",
                    "account_id": "BITGET-001",
                    "asset": "USDT",
                    "free": "0",
                    "locked": "0",
                    "total": "0",
                    "ts_ms": 1_700_000_000_100,
                    "row_id": "plumeusdt_bitget_perp_makerv3:cash:0",
                    "product_type": "perp",
                },
            ],
        },
        required_strategy_ids=set(),
        now_ms_value=2_000,
    )

    bitget_rows = [
        row
        for row in snapshot["balances"]["rows"]
        if row.get("exchange") == "bitget" and row.get("asset") == "USDT"
    ]

    assert len(bitget_rows) == 1
    row = bitget_rows[0]
    assert row["row_id"] == "tokenmm:cash:bitget:BITGET-001:USDT"
    assert row["total"] == "500"


def test_build_portfolio_snapshot_deduplicates_identical_non_stable_cash_across_product_scopes() -> None:
    snapshot = build_portfolio_snapshot(
        portfolio_id="tokenmm",
        base_currency="PLUME",
        inventory_components={},
        balance_rows_by_strategy={
            "plumeusdt_bybit_spot_makerv3": [
                {
                    "strategy_id": "plumeusdt_bybit_spot_makerv3",
                    "exchange": "bybit",
                    "account_id": "BYBIT-UNIFIED",
                    "asset": "PLUME",
                    "free": "-62391.95495260",
                    "locked": "0",
                    "total": "-62391.95495260",
                    "ts_ms": 1_700_000_000_100,
                    "row_id": "plumeusdt_bybit_spot_makerv3:cash:0",
                    "product_type": "spot",
                },
            ],
            "plumeusdt_bybit_perp_makerv3": [
                {
                    "strategy_id": "plumeusdt_bybit_perp_makerv3",
                    "exchange": "bybit",
                    "account_id": "BYBIT-UNIFIED",
                    "asset": "PLUME",
                    "free": "-62391.9549526",
                    "locked": "0",
                    "total": "-62391.9549526",
                    "ts_ms": 1_700_000_000_000,
                    "row_id": "plumeusdt_bybit_perp_makerv3:cash:0",
                    "product_type": "perp",
                },
            ],
        },
        required_strategy_ids=set(),
        now_ms_value=2_000,
    )

    bybit_rows = [
        row
        for row in snapshot["balances"]["rows"]
        if row.get("exchange") == "bybit" and row.get("asset") == "PLUME"
    ]

    assert len(bybit_rows) == 1
    row = bybit_rows[0]
    assert row["row_id"] == "tokenmm:cash:bybit:BYBIT-UNIFIED:spot:PLUME"
    assert row["product_type"] == "spot"
    assert row["total"] == "-62391.95495260"
