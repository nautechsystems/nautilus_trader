from __future__ import annotations

from decimal import ROUND_FLOOR
from decimal import Decimal
import re


_HYPERLIQUID_EQUITY_PERP_RE = re.compile(
    r"^(?:[a-z0-9_]+:)?(?P<symbol>[A-Z0-9]+(?:\.[A-Z0-9]+)*)-USD-PERP\.HYPERLIQUID$",
)


def _normalize_increment(value: Decimal, *, field_name: str) -> Decimal:
    if value <= 0:
        raise ValueError(f"`{field_name}` must be > 0")
    return value


def translate_hyperliquid_fill_to_ibkr_shares(
    *,
    fill_qty: Decimal,
    min_share_increment: Decimal = Decimal("1"),
) -> Decimal:
    increment = _normalize_increment(min_share_increment, field_name="min_share_increment")
    sign = Decimal("-1") if fill_qty < 0 else Decimal("1")
    scaled = (abs(fill_qty) / increment).to_integral_value(rounding=ROUND_FLOOR)
    shares = scaled * increment
    return sign * shares


def hyperliquid_perp_to_ibkr_instrument_id(
    instrument_id: str,
    *,
    primary_exchange: str,
) -> str:
    match = _HYPERLIQUID_EQUITY_PERP_RE.match(str(instrument_id).strip())
    if match is None:
        raise ValueError(f"Unsupported Hyperliquid equity perp instrument_id: {instrument_id!r}")

    exchange = str(primary_exchange).strip().upper()
    if not exchange:
        raise ValueError("`primary_exchange` must be non-empty")

    return f"{match.group('symbol')}.{exchange}"


__all__ = [
    "hyperliquid_perp_to_ibkr_instrument_id",
    "translate_hyperliquid_fill_to_ibkr_shares",
]
