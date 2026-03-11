from __future__ import annotations

from nautilus_trader.flux.api.payloads import collapse_balance_display_rows
from nautilus_trader.flux.api.payloads import merge_portfolio_balances_rows


def test_merge_portfolio_balances_rows_deduplicates_identical_non_stable_cash_across_product_scopes() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
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
        portfolio_id="tokenmm",
        preserve_product_scope_cash=True,
    )

    plume_rows = [
        row
        for row in merged
        if row.get("exchange") == "bybit" and row.get("asset") == "PLUME"
    ]

    assert len(plume_rows) == 1
    row = plume_rows[0]
    assert row["row_id"] == "tokenmm:cash:bybit:BYBIT-UNIFIED:spot:PLUME"
    assert row["product_type"] == "spot"
    assert row["total"] == "-62391.95495260"


def test_duplicate_spot_position_collapse_is_account_aware() -> None:
    collapsed = collapse_balance_display_rows(
        [
            {
                "row_id": "cash:acct-a:plume",
                "exchange": "bybit",
                "account": "acct-a",
                "asset": "PLUME",
                "total": "10",
                "product_type": "spot",
            },
            {
                "row_id": "pos:acct-b:plume",
                "exchange": "bybit",
                "account": "acct-b",
                "kind": "position",
                "instrument_id": "PLUMEUSDT-SPOT.BYBIT",
                "asset": "PLUME",
                "signed_qty": "7",
                "quantity": "7",
                "product_type": "spot",
            },
        ],
    )

    assert [row["row_id"] for row in collapsed] == [
        "cash:acct-a:plume",
        "pos:acct-b:plume",
    ]


def test_merge_portfolio_balances_rows_keeps_bitget_shared_account_stable_cash_scoped_by_product_type() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "plumeusdt_bitget_spot_makerv3": [
                {
                    "strategy_id": "plumeusdt_bitget_spot_makerv3",
                    "exchange": "bitget",
                    "account_id": "BITGET-001",
                    "asset": "USDT",
                    "free": "500",
                    "locked": "0",
                    "total": "500",
                    "ts_ms": 1_700_000_000_100,
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
                    "ts_ms": 1_700_000_000_000,
                    "row_id": "plumeusdt_bitget_perp_makerv3:cash:0",
                    "product_type": "perp",
                },
            ],
        },
        portfolio_id="tokenmm",
        preserve_product_scope_cash=True,
    )

    bitget_rows = sorted(
        [
            row
            for row in merged
            if row.get("exchange") == "bitget" and row.get("asset") == "USDT"
        ],
        key=lambda row: str(row["row_id"]),
    )

    assert [row["row_id"] for row in bitget_rows] == [
        "tokenmm:cash:bitget:BITGET-001:perp:USDT",
        "tokenmm:cash:bitget:BITGET-001:spot:USDT",
    ]
    assert [row["total"] for row in bitget_rows] == ["0", "500"]
    assert [row["product_type"] for row in bitget_rows] == ["perp", "spot"]


def test_collapse_balance_display_rows_keeps_bitget_cash_rows_across_product_scopes() -> None:
    collapsed = collapse_balance_display_rows(
        [
            {
                "row_id": "tokenmm:cash:bitget:BITGET-001:spot:USDT",
                "exchange": "bitget",
                "account": "BITGET-001",
                "asset": "USDT",
                "total": "500",
                "product_type": "spot",
            },
            {
                "row_id": "tokenmm:cash:bitget:BITGET-001:perp:USDT",
                "exchange": "bitget",
                "account": "BITGET-001",
                "asset": "USDT",
                "total": "0",
                "product_type": "perp",
            },
        ],
    )

    assert [row["row_id"] for row in collapsed] == [
        "tokenmm:cash:bitget:BITGET-001:spot:USDT",
        "tokenmm:cash:bitget:BITGET-001:perp:USDT",
    ]
