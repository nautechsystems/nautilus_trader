from __future__ import annotations

from decimal import Decimal

import pytest

from nautilus_trader.flux.strategies.makerv4.instruments import (
    hyperliquid_perp_to_ibkr_instrument_id,
)
from nautilus_trader.flux.strategies.makerv4.instruments import (
    translate_hyperliquid_fill_to_ibkr_shares,
)


def test_translate_hyperliquid_fill_to_ibkr_shares_rounds_down_to_int() -> None:
    shares = translate_hyperliquid_fill_to_ibkr_shares(
        fill_qty=Decimal("1.87"),
        min_share_increment=Decimal("1"),
    )

    assert shares == Decimal("1")


def test_translate_hyperliquid_fill_to_ibkr_shares_returns_zero_for_tiny_fill() -> None:
    shares = translate_hyperliquid_fill_to_ibkr_shares(
        fill_qty=Decimal("0.42"),
        min_share_increment=Decimal("1"),
    )

    assert shares == Decimal("0")


def test_translate_hyperliquid_fill_to_ibkr_shares_preserves_sign() -> None:
    shares = translate_hyperliquid_fill_to_ibkr_shares(
        fill_qty=Decimal("-3.99"),
        min_share_increment=Decimal("1"),
    )

    assert shares == Decimal("-3")


def test_hyperliquid_perp_to_ibkr_instrument_id_maps_equity_symbol() -> None:
    assert (
        hyperliquid_perp_to_ibkr_instrument_id(
            "xyz:AAPL-USD-PERP.HYPERLIQUID",
            primary_exchange="NASDAQ",
        )
        == "AAPL.NASDAQ"
    )


def test_hyperliquid_perp_to_ibkr_instrument_id_preserves_dotted_share_class_symbols() -> None:
    assert (
        hyperliquid_perp_to_ibkr_instrument_id(
            "xyz:BRK.B-USD-PERP.HYPERLIQUID",
            primary_exchange="NYSE",
        )
        == "BRK.B.NYSE"
    )


@pytest.mark.parametrize(
    "instrument_id",
    [
        "AAPL.NASDAQ",
        "xyz:AAPL-USD.HYPERLIQUID",
        "xyz:-USD-PERP.HYPERLIQUID",
    ],
)
def test_hyperliquid_perp_to_ibkr_instrument_id_rejects_invalid_ids(instrument_id: str) -> None:
    with pytest.raises(ValueError, match="instrument_id"):
        hyperliquid_perp_to_ibkr_instrument_id(
            instrument_id,
            primary_exchange="NASDAQ",
        )
