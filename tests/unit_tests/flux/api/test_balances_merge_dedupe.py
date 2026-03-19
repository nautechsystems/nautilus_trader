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


def test_merge_portfolio_balances_rows_canonicalizes_bitget_shared_account_stable_cash() -> None:
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
                    "free": "500",
                    "locked": "0",
                    "total": "500",
                    "ts_ms": 1_700_000_000_100,
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

    assert len(bitget_rows) == 1
    row = bitget_rows[0]
    assert row["row_id"] == "tokenmm:cash:bitget:BITGET-001:USDT"
    assert row["total"] == "500"
    assert row["product_type"] == "spot"
    assert row["display_name_short"] == "USDT"
    assert row["display_name_long"] == "Bitget USDT"


def test_merge_portfolio_balances_rows_deduplicates_shared_position_snapshots_by_group() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "aapl_tradexyz_maker": [
                {
                    "strategy_id": "aapl_tradexyz_maker",
                    "kind": "position",
                    "exchange": "hyperliquid",
                    "account": "HYPERLIQUID-master",
                    "instrument_id": "XYZ:AAPL-USD-PERP.HYPERLIQUID",
                    "signed_qty": "10",
                    "quantity": "10",
                    "ts_ms": 1_700_000_000_000,
                },
            ],
            "aapl_tradexyz_taker": [
                {
                    "strategy_id": "aapl_tradexyz_taker",
                    "kind": "position",
                    "exchange": "hyperliquid",
                    "account": "HYPERLIQUID-master",
                    "instrument_id": "XYZ:AAPL-USD-PERP.HYPERLIQUID",
                    "signed_qty": "10",
                    "quantity": "10",
                    "ts_ms": 1_700_000_000_100,
                },
            ],
        },
        portfolio_id="equities",
        shared_position_groups_by_strategy={
            "aapl_tradexyz_maker": "AAPL|hyperliquid.xyz.main|xyz:AAPL-USD-PERP.HYPERLIQUID",
            "aapl_tradexyz_taker": "AAPL|hyperliquid.xyz.main|xyz:AAPL-USD-PERP.HYPERLIQUID",
        },
    )

    position_rows = [
        row
        for row in merged
        if row.get("kind") == "position"
        and row.get("exchange") == "hyperliquid"
        and row.get("instrument_id") == "XYZ:AAPL-USD-PERP.HYPERLIQUID"
    ]

    assert len(position_rows) == 1
    assert position_rows[0]["strategy_id"] == "equities"
    assert position_rows[0]["signed_qty"] == "10"


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


def test_collapse_balance_display_rows_canonicalizes_shared_account_stable_cash_for_non_usdt_assets() -> None:
    collapsed = collapse_balance_display_rows(
        [
            {
                "row_id": "tokenmm:cash:bitget:BITGET-001:spot:USDC",
                "exchange": "bitget",
                "account": "BITGET-001",
                "account_id": "BITGET-001",
                "asset": "USDC",
                "total": "500",
                "product_type": "spot",
                "scope": "shared_account",
                "ts_ms": 1_700_000_000_100,
            },
            {
                "row_id": "tokenmm:cash:bitget:BITGET-001:perp:USDC",
                "exchange": "bitget",
                "account": "BITGET-001",
                "account_id": "BITGET-001",
                "asset": "USDC",
                "total": "0",
                "product_type": "perp",
                "scope": "shared_account",
                "ts_ms": 1_700_000_000_000,
            },
        ],
    )

    assert len(collapsed) == 1
    row = collapsed[0]
    assert row["row_id"] == "tokenmm:cash:bitget:BITGET-001:USDC"
    assert row["total"] == "500"
    assert row["product_type"] == "spot"


def test_collapse_balance_display_rows_keeps_shared_account_stable_cash_spot_shaped_when_perp_row_is_newer() -> None:
    collapsed = collapse_balance_display_rows(
        [
            {
                "row_id": "tokenmm:cash:bitget:BITGET-001:spot:USDT",
                "exchange": "bitget",
                "account": "BITGET-001",
                "account_id": "BITGET-001",
                "asset": "USDT",
                "total": "500",
                "product_type": "spot",
                "scope": "shared_account",
                "ts_ms": 1_700_000_000_000,
                "display_name_short": "USDT Spot",
                "display_name_long": "Bitget USDT Spot",
            },
            {
                "row_id": "tokenmm:cash:bitget:BITGET-001:perp:USDT",
                "exchange": "bitget",
                "account": "BITGET-001",
                "account_id": "BITGET-001",
                "asset": "USDT",
                "total": "500",
                "product_type": "perp",
                "scope": "shared_account",
                "ts_ms": 1_700_000_000_100,
                "display_name_short": "USDT Perp",
                "display_name_long": "Bitget USDT Perp",
            },
            {
                "row_id": "tokenmm:pos:bitget:USDT.BITGET",
                "exchange": "bitget",
                "account": "BITGET-001",
                "account_id": "BITGET-001",
                "kind": "position",
                "instrument_id": "USDT.BITGET",
                "asset": "USDT",
                "signed_qty": "500",
                "quantity": "500",
                "product_type": "spot",
            },
        ],
    )

    assert len(collapsed) == 1
    row = collapsed[0]
    assert row["row_id"] == "tokenmm:cash:bitget:BITGET-001:USDT"
    assert row["product_type"] == "spot"
    assert row["display_name_short"] == "USDT"
    assert row["display_name_long"] == "Bitget USDT"
