from __future__ import annotations

from decimal import Decimal

from flux.strategies.makerv4.rounding import round_hyperliquid_price
from flux.strategies.makerv4.rounding import round_ibkr_limit_price


def validate_ibkr_quote(
    *,
    bid: Decimal | None,
    ask: Decimal | None,
    quote_age_ms: int | None = None,
    max_quote_age_ms: int | None = None,
    max_spread_bps: Decimal | None = None,
) -> str | None:
    if bid is None:
        return "missing_bid"
    if ask is None:
        return "missing_ask"
    if ask <= bid:
        return "locked_or_crossed"

    if max_quote_age_ms is not None and quote_age_ms is not None and quote_age_ms > max_quote_age_ms:
        return "stale_quote"

    if max_spread_bps is not None:
        mid = (bid + ask) / Decimal("2")
        if mid <= 0:
            return "missing_midpoint"
        spread_bps = ((ask - bid) / mid) * Decimal("10000")
        if spread_bps > max_spread_bps:
            return "spread_too_wide"

    return None


def build_ibkr_ioc_limit(
    *,
    side: str,
    bid: Decimal | None,
    ask: Decimal | None,
    cross_mid_bps: Decimal,
    tick_size: Decimal,
    quote_age_ms: int | None = None,
    max_quote_age_ms: int | None = None,
    max_spread_bps: Decimal | None = None,
) -> Decimal | None:
    invalid_reason = validate_ibkr_quote(
        bid=bid,
        ask=ask,
        quote_age_ms=quote_age_ms,
        max_quote_age_ms=max_quote_age_ms,
        max_spread_bps=max_spread_bps,
    )
    if invalid_reason is not None:
        return None

    assert bid is not None
    assert ask is not None

    normalized_side = str(side).strip().upper()
    if normalized_side not in {"BUY", "SELL"}:
        raise ValueError(f"Unsupported side: {side!r}")

    mid = (bid + ask) / Decimal("2")
    cross_ratio = cross_mid_bps / Decimal("10000")
    raw_price = mid * (Decimal("1") + cross_ratio if normalized_side == "BUY" else Decimal("1") - cross_ratio)
    rounded_price = round_ibkr_limit_price(
        raw_price,
        tick_size=tick_size,
        side=normalized_side,
    )

    if normalized_side == "BUY":
        return min(rounded_price, ask)
    return max(rounded_price, bid)


def build_maker_quote_price(
    *,
    side: str,
    reference_mid: Decimal,
    target_edge_bps: Decimal,
    maker_fee_bps: Decimal,
    hedge_fee_bps: Decimal,
    offset_bps: Decimal,
    tick_size: Decimal,
) -> Decimal:
    if reference_mid <= 0:
        raise ValueError("`reference_mid` must be > 0")

    normalized_side = str(side).strip().upper()
    if normalized_side not in {"BUY", "SELL"}:
        raise ValueError(f"Unsupported side: {side!r}")

    total_bps = target_edge_bps + maker_fee_bps + hedge_fee_bps + offset_bps
    ratio = total_bps / Decimal("10000")
    raw_price = (
        reference_mid * (Decimal("1") - ratio)
        if normalized_side == "BUY"
        else reference_mid * (Decimal("1") + ratio)
    )
    return round_hyperliquid_price(raw_price, tick_size=tick_size, side=normalized_side)


__all__ = [
    "build_ibkr_ioc_limit",
    "build_maker_quote_price",
    "validate_ibkr_quote",
]
