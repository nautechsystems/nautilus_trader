from __future__ import annotations

from decimal import Decimal

from nautilus_trader.flux.strategies.makerv4.pricing import build_ibkr_ioc_limit
from nautilus_trader.flux.strategies.makerv4.pricing import build_maker_quote_price
from nautilus_trader.flux.strategies.makerv4.pricing import validate_ibkr_quote
from nautilus_trader.flux.strategies.makerv4.rounding import round_hyperliquid_price


def test_round_hyperliquid_price_is_side_aware_on_tick_size() -> None:
    assert round_hyperliquid_price(
        Decimal("190.037"),
        tick_size=Decimal("0.01"),
        side="BUY",
    ) == Decimal("190.03")
    assert round_hyperliquid_price(
        Decimal("190.037"),
        tick_size=Decimal("0.01"),
        side="SELL",
    ) == Decimal("190.04")


def test_build_hedge_limit_caps_buy_at_best_ask_after_rounding() -> None:
    limit_price = build_ibkr_ioc_limit(
        side="BUY",
        bid=Decimal("190.00"),
        ask=Decimal("190.04"),
        cross_mid_bps=Decimal("5"),
        tick_size=Decimal("0.01"),
    )

    assert limit_price == Decimal("190.04")


def test_build_hedge_limit_caps_sell_at_best_bid_after_rounding() -> None:
    limit_price = build_ibkr_ioc_limit(
        side="SELL",
        bid=Decimal("190.00"),
        ask=Decimal("190.04"),
        cross_mid_bps=Decimal("5"),
        tick_size=Decimal("0.01"),
    )

    assert limit_price == Decimal("190.00")


def test_build_hedge_limit_caps_cross_mid_bps_before_rounding() -> None:
    limit_price = build_ibkr_ioc_limit(
        side="BUY",
        bid=Decimal("190.00"),
        ask=Decimal("195.00"),
        cross_mid_bps=Decimal("50"),
        max_cross_bps=Decimal("10"),
        tick_size=Decimal("0.01"),
    )

    assert limit_price == Decimal("192.70")


def test_build_hedge_limit_returns_none_for_locked_market() -> None:
    assert (
        build_ibkr_ioc_limit(
            side="BUY",
            bid=Decimal("190.04"),
            ask=Decimal("190.04"),
            cross_mid_bps=Decimal("5"),
            tick_size=Decimal("0.01"),
        )
        is None
    )


def test_build_hedge_limit_returns_none_for_one_sided_quote() -> None:
    assert (
        build_ibkr_ioc_limit(
            side="BUY",
            bid=None,
            ask=Decimal("190.04"),
            cross_mid_bps=Decimal("5"),
            tick_size=Decimal("0.01"),
        )
        is None
    )


def test_validate_ibkr_quote_rejects_stale_quotes() -> None:
    assert validate_ibkr_quote(
        bid=Decimal("190.00"),
        ask=Decimal("190.04"),
        quote_age_ms=1_001,
        max_quote_age_ms=1_000,
        max_spread_bps=Decimal("25"),
    ) == "stale_quote"


def test_validate_ibkr_quote_rejects_very_wide_spread() -> None:
    assert validate_ibkr_quote(
        bid=Decimal("190.00"),
        ask=Decimal("192.00"),
        quote_age_ms=50,
        max_quote_age_ms=1_000,
        max_spread_bps=Decimal("25"),
    ) == "spread_too_wide"


def test_validate_ibkr_quote_rejects_missing_midpoint() -> None:
    assert validate_ibkr_quote(
        bid=Decimal("-1.00"),
        ask=Decimal("1.00"),
        quote_age_ms=50,
        max_quote_age_ms=1_000,
        max_spread_bps=Decimal("25"),
    ) == "missing_midpoint"


def test_build_maker_quote_price_includes_target_edge_and_fee_gross_up() -> None:
    bid_quote = build_maker_quote_price(
        side="BUY",
        reference_mid=Decimal("190.02"),
        target_edge_bps=Decimal("5"),
        maker_fee_bps=Decimal("0.25"),
        hedge_fee_bps=Decimal("1.25"),
        offset_bps=Decimal("1"),
        tick_size=Decimal("0.01"),
    )
    ask_quote = build_maker_quote_price(
        side="SELL",
        reference_mid=Decimal("190.02"),
        target_edge_bps=Decimal("5"),
        maker_fee_bps=Decimal("0.25"),
        hedge_fee_bps=Decimal("1.25"),
        offset_bps=Decimal("1"),
        tick_size=Decimal("0.01"),
    )

    assert bid_quote == Decimal("189.87")
    assert ask_quote == Decimal("190.17")
