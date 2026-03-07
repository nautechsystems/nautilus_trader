from __future__ import annotations

from decimal import ROUND_CEILING
from decimal import ROUND_FLOOR
from decimal import Decimal


def _normalize_tick(value: Decimal, *, field_name: str) -> Decimal:
    if value <= 0:
        raise ValueError(f"`{field_name}` must be > 0")
    return value


def _normalize_side(side: str) -> str:
    normalized = str(side).strip().upper()
    if normalized not in {"BUY", "SELL"}:
        raise ValueError(f"Unsupported side: {side!r}")
    return normalized


def round_hyperliquid_price(price: Decimal, *, tick_size: Decimal, side: str) -> Decimal:
    tick = _normalize_tick(tick_size, field_name="tick_size")
    normalized_side = _normalize_side(side)
    rounding = ROUND_FLOOR if normalized_side == "BUY" else ROUND_CEILING
    steps = (price / tick).to_integral_value(rounding=rounding)
    return steps * tick


def round_ibkr_limit_price(price: Decimal, *, tick_size: Decimal, side: str) -> Decimal:
    tick = _normalize_tick(tick_size, field_name="tick_size")
    normalized_side = _normalize_side(side)
    rounding = ROUND_CEILING if normalized_side == "BUY" else ROUND_FLOOR
    steps = (price / tick).to_integral_value(rounding=rounding)
    return steps * tick


__all__ = [
    "round_hyperliquid_price",
    "round_ibkr_limit_price",
]
