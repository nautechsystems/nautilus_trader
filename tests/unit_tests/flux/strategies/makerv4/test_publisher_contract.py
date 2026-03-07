from __future__ import annotations

from nautilus_trader.flux.strategies.makerv4.publisher import build_quote_snapshot_payload
from nautilus_trader.flux.strategies.shared.quote_snapshot import (
    build_quote_snapshot_payload as shared_build_quote_snapshot_payload,
)


def test_makerv4_publisher_reuses_shared_quote_snapshot_contract() -> None:
    assert build_quote_snapshot_payload is shared_build_quote_snapshot_payload

    payload = build_quote_snapshot_payload(
        maker_leg={"venue": "HYPERLIQUID", "symbol": "AAPL/USD"},
        hedge_leg={"venue": "IBKR", "symbol": "AAPL/USD"},
        ref_leg={"venue": "IBKR", "symbol": "AAPL/USD"},
        effective_spread_bps=6.5,
        assumed_hedge_fee_bps=1.0,
    )

    assert payload["maker_leg"]["venue"] == "HYPERLIQUID"
    assert payload["hedge_leg"]["venue"] == "IBKR"
    assert payload["ref_leg"]["venue"] == "IBKR"
    assert payload["effective_spread_bps"] == 6.5
