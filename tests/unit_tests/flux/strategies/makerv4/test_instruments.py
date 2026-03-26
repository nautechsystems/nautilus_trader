from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

import pytest

from nautilus_trader.flux.strategies.makerv4.instruments import (
    hyperliquid_perp_to_ibkr_instrument_id,
)
from nautilus_trader.flux.strategies.makerv4.instruments import (
    translate_maker_fill_to_ibkr_shares,
)
from nautilus_trader.flux.strategies.makerv4.instruments import (
    translate_hyperliquid_fill_to_ibkr_shares,
)


def _instrument(*, multiplier: str = "1", settlement_currency: str = "USD") -> SimpleNamespace:
    return SimpleNamespace(
        multiplier=Decimal(multiplier),
        is_inverse=False,
        base_currency=SimpleNamespace(code="AAPL"),
        quote_currency=SimpleNamespace(code="USD"),
        settlement_currency=SimpleNamespace(code=settlement_currency),
        info={},
        make_qty=lambda value: Decimal(str(value)),
        make_price=lambda value: Decimal(str(value)),
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


def test_translate_maker_fill_to_ibkr_shares_conversion_uses_canonical_base_before_rounding() -> None:
    translation = translate_maker_fill_to_ibkr_shares(
        maker_instrument=_instrument(multiplier="0.0625"),
        fill_qty=Decimal("16"),
        fill_price=Decimal("190"),
        min_share_increment=Decimal("1"),
    )

    assert translation.base_qty == Decimal("1")
    assert translation.hedge_qty == Decimal("1")
    assert translation.qty_conversion_status == "exact_multiplier"
    assert translation.qty_conversion_source == "generic:multiplier"


def test_translate_maker_fill_to_ibkr_shares_conversion_fails_closed_when_base_exposure_is_unsupported() -> None:
    translation = translate_maker_fill_to_ibkr_shares(
        maker_instrument=_instrument(settlement_currency="USDC"),
        fill_qty=Decimal("1"),
        fill_price=Decimal("190"),
        min_share_increment=Decimal("1"),
    )

    assert translation.base_qty is None
    assert translation.hedge_qty is None
    assert translation.qty_conversion_status == "unsupported"
    assert translation.qty_conversion_source == "generic:quanto instrument"


def test_translate_maker_fill_to_ibkr_shares_falls_back_to_identity_when_metadata_is_missing() -> None:
    translation = translate_maker_fill_to_ibkr_shares(
        maker_instrument=SimpleNamespace(raw_symbol="AAPL/USD"),
        fill_qty=Decimal("2.7"),
        fill_price=Decimal("190"),
        min_share_increment=Decimal("1"),
    )

    assert translation.base_qty == Decimal("2.7")
    assert translation.hedge_qty == Decimal("2")
    assert translation.qty_conversion_status == "identity_fallback"
    assert translation.qty_conversion_source == "maker_instrument:missing_metadata_identity_fallback"


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
