from __future__ import annotations

from nautilus_trader.flux.api.app import prefer_controller_managed_balance_rows
from nautilus_trader.flux.api._payloads_balances import combine_portfolio_snapshot_rows
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


def test_merge_portfolio_balances_rows_keeps_ibkr_cash_generic_across_product_scopes() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "aapl_tradexyz_makerv4": [
                {
                    "strategy_id": "aapl_tradexyz_makerv4",
                    "exchange": "ibkr",
                    "account_id": "U10015777",
                    "asset": "HKD",
                    "free": "84214.62",
                    "locked": "0",
                    "total": "84214.62",
                    "ts_ms": 1_700_000_000_000,
                    "row_id": "aapl_tradexyz_makerv4:cash:0",
                    "product_type": "perp",
                },
            ],
            "aapl_binance_perp_makerv4": [
                {
                    "strategy_id": "aapl_binance_perp_makerv4",
                    "exchange": "ibkr",
                    "account_id": "U10015777",
                    "asset": "HKD",
                    "free": "84175.66",
                    "locked": "0",
                    "total": "84175.66",
                    "ts_ms": 1_700_000_000_100,
                    "row_id": "aapl_binance_perp_makerv4:cash:0",
                    "product_type": "spot",
                },
            ],
        },
        portfolio_id="equities",
        preserve_product_scope_cash=True,
    )

    hkd_rows = [
        row
        for row in merged
        if row.get("exchange") == "ibkr" and row.get("asset") == "HKD"
    ]

    assert len(hkd_rows) == 1
    row = hkd_rows[0]
    assert row["row_id"] == "equities:cash:ibkr:U10015777:HKD"
    assert row["total"] == "84175.66"
    assert row["display_name_short"] == "HKD"
    assert row["display_name_long"] == "Ibkr HKD"


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


def test_merge_portfolio_balances_rows_uses_account_scope_identity_for_shared_binance_stable_cash() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "plumeusdt_binance_spot_makerv3": [
                {
                    "strategy_id": "plumeusdt_binance_spot_makerv3",
                    "exchange": "binance_spot",
                    "account_id": "BINANCE_SPOT-PORTFOLIO_MARGIN-master",
                    "asset": "USDT",
                    "free": "1285.28070703",
                    "locked": "0",
                    "total": "1285.28070703",
                    "ts_ms": 1_700_000_000_000,
                    "row_id": "plumeusdt_binance_spot_makerv3:cash:0",
                    "product_type": "spot",
                },
            ],
            "plumeusdt_binance_perp_makerv3": [
                {
                    "strategy_id": "plumeusdt_binance_perp_makerv3",
                    "exchange": "binance_spot",
                    "account_id": "BINANCE_SPOT-MARGIN-master",
                    "asset": "USDT",
                    "free": "873.32524016",
                    "locked": "0",
                    "total": "873.32524016",
                    "ts_ms": 1_700_000_000_100,
                    "row_id": "plumeusdt_binance_perp_makerv3:cash:0",
                    "product_type": "spot",
                },
            ],
        },
        portfolio_id="tokenmm",
        preserve_product_scope_cash=True,
        execution_account_scope_by_strategy={
            "plumeusdt_binance_spot_makerv3": "binance.execution.main",
            "plumeusdt_binance_perp_makerv3": "binance.execution.main",
        },
    )

    binance_rows = [
        row
        for row in merged
        if row.get("exchange") == "binance_spot" and row.get("asset") == "USDT"
    ]

    assert len(binance_rows) == 1
    row = binance_rows[0]
    assert row["row_id"] == "tokenmm:cash:binance_spot:binance.execution.main:USDT"
    assert row["account"] == "binance.execution.main"
    assert row["account_scope_id"] == "binance.execution.main"
    assert row["scope"] == "shared_account"
    assert row["source_strategy_ids"] == [
        "plumeusdt_binance_perp_makerv3",
        "plumeusdt_binance_spot_makerv3",
    ]


def test_combine_portfolio_snapshot_rows_uses_account_scope_identity_across_binance_product_scopes() -> None:
    merged = combine_portfolio_snapshot_rows(
        balance_rows=[
            {
                "row_id": "tokenmm:cash:binance_perp:binance.pm.main:USDT",
                "exchange": "binance_perp",
                "account": "binance.pm.main",
                "account_id": "binance.pm.main",
                "account_scope_id": "binance.pm.main",
                "asset": "USDT",
                "free": "1285.28070703",
                "total": "1285.28070703",
                "product_type": "perp",
                "source_scope": "portfolio",
                "ts_ms": 1_700_000_000_000,
            },
        ],
        account_rows=[
            {
                "row_id": "tokenmm:shared:binance.pm.main:cash:binance_spot:BINANCE-main:USDT",
                "exchange": "binance_spot",
                "account": "BINANCE-main",
                "account_id": "BINANCE-main",
                "account_scope_id": "binance.pm.main",
                "asset": "USDT",
                "free": "910.24627913",
                "total": "910.24627913",
                "product_type": "spot",
                "source_scope": "shared_account",
                "source_strategy_ids": [
                    "plumeusdt_binance_perp_makerv3",
                    "plumeusdt_binance_spot_makerv3",
                ],
                "ts_ms": 1_700_000_000_100,
            },
        ],
        portfolio_id="tokenmm",
    )

    binance_rows = [
        row
        for row in merged
        if row.get("account_scope_id") == "binance.pm.main" and row.get("asset") == "USDT"
    ]

    assert len(binance_rows) == 1
    row = binance_rows[0]
    assert row["exchange"] == "binance_spot"
    assert row["account"] == "BINANCE-main"
    assert row["source_scope"] == "shared_account"
    assert row["total"] == "910.24627913"


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


def test_merge_portfolio_balances_rows_deduplicates_binance_split_pair_but_keeps_multivenue_same_asset_positions() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "aapl_binance_perp_maker": [
                {
                    "strategy_id": "aapl_binance_perp_maker",
                    "kind": "position",
                    "exchange": "binance_perp",
                    "account": "BINANCE-main",
                    "instrument_id": "AAPLUSDT-PERP.BINANCE_PERP",
                    "asset": "AAPL",
                    "coin": "AAPL",
                    "signed_qty": "4",
                    "quantity": "4",
                    "product_type": "perp",
                    "contract_type": "perp",
                    "source_scope": "shared_account",
                    "account_scope_id": "binance.futures.main",
                    "ts_ms": 1_700_000_000_000,
                },
            ],
            "aapl_binance_perp_taker": [
                {
                    "strategy_id": "aapl_binance_perp_taker",
                    "kind": "position",
                    "exchange": "binance_perp",
                    "account": "BINANCE-main",
                    "instrument_id": "AAPLUSDT-PERP.BINANCE_PERP",
                    "asset": "AAPL",
                    "coin": "AAPL",
                    "signed_qty": "4",
                    "quantity": "4",
                    "product_type": "perp",
                    "contract_type": "perp",
                    "source_scope": "shared_account",
                    "account_scope_id": "binance.futures.main",
                    "ts_ms": 1_700_000_000_100,
                },
            ],
            "aapl_tradexyz_maker": [
                {
                    "strategy_id": "aapl_tradexyz_maker",
                    "kind": "position",
                    "exchange": "hyperliquid",
                    "account": "HYPERLIQUID-master",
                    "instrument_id": "XYZ:AAPL-USD-PERP.HYPERLIQUID",
                    "asset": "AAPL",
                    "coin": "AAPL",
                    "signed_qty": "10",
                    "quantity": "10",
                    "product_type": "perp",
                    "contract_type": "perp",
                    "source_scope": "shared_account",
                    "account_scope_id": "hyperliquid.xyz.main",
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
                    "asset": "AAPL",
                    "coin": "AAPL",
                    "signed_qty": "10",
                    "quantity": "10",
                    "product_type": "perp",
                    "contract_type": "perp",
                    "source_scope": "shared_account",
                    "account_scope_id": "hyperliquid.xyz.main",
                    "ts_ms": 1_700_000_000_100,
                },
            ],
        },
        portfolio_id="equities",
        shared_position_groups_by_strategy={
            "aapl_binance_perp_maker": "AAPL|binance.futures.main|AAPLUSDT-PERP.BINANCE_PERP",
            "aapl_binance_perp_taker": "AAPL|binance.futures.main|AAPLUSDT-PERP.BINANCE_PERP",
            "aapl_tradexyz_maker": "AAPL|hyperliquid.xyz.main|xyz:AAPL-USD-PERP.HYPERLIQUID",
            "aapl_tradexyz_taker": "AAPL|hyperliquid.xyz.main|xyz:AAPL-USD-PERP.HYPERLIQUID",
        },
    )

    position_rows = [
        row
        for row in merged
        if row.get("kind") == "position" and row.get("asset") == "AAPL"
    ]

    assert len(position_rows) == 2
    assert {row["instrument_id"] for row in position_rows} == {
        "AAPLUSDT-PERP.BINANCE_PERP",
        "XYZ:AAPL-USD-PERP.HYPERLIQUID",
    }
    assert {row["exchange"] for row in position_rows} == {"binance_perp", "hyperliquid"}
    assert {row["signed_qty"] for row in position_rows} == {"4", "10"}
    assert all(row["strategy_id"] == "equities" for row in position_rows)


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


def test_prefer_controller_managed_balance_rows_keeps_one_authoritative_binance_shared_row() -> None:
    rows = prefer_controller_managed_balance_rows(
        [
            {
                "row_id": "tokenmm:cash:binance_spot:BINANCE-old:USDT",
                "exchange": "binance_spot",
                "account": "BINANCE-old",
                "account_id": "BINANCE-old",
                "account_scope_id": "binance.pm.main",
                "asset": "USDT",
                "total": "873.32524016",
                "product_type": "spot",
                "source_scope": "strategy_local",
                "ts_ms": 1_700_000_000_000,
            },
            {
                "row_id": "tokenmm:shared:binance.pm.main:cash:binance_spot:BINANCE-main:USDT",
                "exchange": "binance_spot",
                "account": "BINANCE-main",
                "account_id": "BINANCE-main",
                "account_scope_id": "binance.pm.main",
                "asset": "USDT",
                "total": "910.24627913",
                "product_type": "spot",
                "source_scope": "shared_account",
                "ts_ms": 1_700_000_000_100,
            },
        ],
        controller_scope_by_account_scope={"binance.pm.main": "tokenmm.binance.pm.main"},
    )

    binance_rows = [
        row
        for row in rows
        if row.get("account_scope_id") == "binance.pm.main" and row.get("asset") == "USDT"
    ]

    assert len(binance_rows) == 1
    row = binance_rows[0]
    assert row["row_id"] == "tokenmm:shared:binance.pm.main:cash:binance_spot:BINANCE-main:USDT"
    assert row["controller_scope_id"] == "tokenmm.binance.pm.main"
    assert row["authority_state"] == "active"
