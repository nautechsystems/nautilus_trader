from __future__ import annotations

from nautilus_trader.adapters.binance.spot.schemas.account import BinancePortfolioMarginAccountInfo
from nautilus_trader.adapters.binance.spot.schemas.account import BinancePortfolioMarginBalanceInfo
from nautilus_trader.flux.runners.shared.profile_accounts import (
    _build_binance_spot_margin_account_snapshot,
)


def test_build_binance_spot_margin_account_snapshot_publishes_shared_cash_rows() -> None:
    snapshot = _build_binance_spot_margin_account_snapshot(
        account_info=BinancePortfolioMarginAccountInfo(
            balances=[
                BinancePortfolioMarginBalanceInfo(
                    asset="USDT",
                    totalWalletBalance="1285.28070703",
                    crossMarginAsset="1285.28070703",
                    crossMarginBorrowed="0",
                    crossMarginInterest="0",
                    crossMarginLocked="0",
                    updateTime=1_700_000_000_100,
                ),
            ],
            updateTime=1_700_000_000_100,
        ),
        account_id="BINANCE-main",
        exchange="binance_spot",
        ts_ms=1_700_000_000_100,
    )

    assert snapshot["source_scope"] == "shared_account"
    assert snapshot["totals"] == {}
    assert len(snapshot["rows"]) == 1
    row = snapshot["rows"][0]
    assert row["account"] == "BINANCE-main"
    assert row["account_id"] == "BINANCE-main"
    assert row["asset"] == "USDT"
    assert row["base"] == "USDT"
    assert row["coin"] == "USDT"
    assert row["exchange"] == "binance_spot"
    assert row["free"] == "1285.28070703"
    assert row["locked"] == "0.00000000"
    assert row["mark_raw"] == 1.0
    assert row["market_type"] == "spot"
    assert row["mv_raw"] == 1285.28070703
    assert row["product_type"] == "spot"
    assert row["total"] == "1285.28070703"
    assert row["ts_ms"] == 1_700_000_000_100
