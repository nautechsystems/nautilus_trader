from __future__ import annotations

from dataclasses import dataclass
from decimal import ROUND_FLOOR
from decimal import Decimal
import re

from flux.common.quantity_units import exposure_from_venue_qty


_HYPERLIQUID_EQUITY_PERP_RE = re.compile(
    r"^(?:[a-z0-9_]+:)?(?P<symbol>[A-Z0-9]+(?:\.[A-Z0-9]+)*)-USD-PERP\.HYPERLIQUID$",
)


def _normalize_increment(value: Decimal, *, field_name: str) -> Decimal:
    if value <= 0:
        raise ValueError(f"`{field_name}` must be > 0")
    return value


@dataclass(frozen=True, slots=True)
class MakerFillHedgeTranslation:
    venue_qty: Decimal
    base_qty: Decimal | None
    hedge_qty: Decimal | None
    qty_conversion_status: str
    qty_conversion_source: str


def translate_hyperliquid_fill_to_ibkr_shares(
    *,
    fill_qty: Decimal,
    min_share_increment: Decimal = Decimal("1"),
) -> Decimal:
    return round_base_fill_to_ibkr_shares(
        fill_qty=fill_qty,
        min_share_increment=min_share_increment,
    )


def round_base_fill_to_ibkr_shares(
    *,
    fill_qty: Decimal,
    min_share_increment: Decimal = Decimal("1"),
) -> Decimal:
    increment = _normalize_increment(min_share_increment, field_name="min_share_increment")
    sign = Decimal("-1") if fill_qty < 0 else Decimal("1")
    scaled = (abs(fill_qty) / increment).to_integral_value(rounding=ROUND_FLOOR)
    shares = scaled * increment
    return sign * shares


def translate_maker_fill_to_ibkr_shares(
    *,
    maker_instrument,
    fill_qty: Decimal,
    fill_price: Decimal | None = None,
    min_share_increment: Decimal = Decimal("1"),
) -> MakerFillHedgeTranslation:
    exposure = exposure_from_venue_qty(
        maker_instrument,
        fill_qty,
        last_px=fill_price,
    )
    if exposure.base_qty is None:
        if exposure.qty_conversion_status == "missing_metadata":
            base_qty = fill_qty
            hedge_qty = round_base_fill_to_ibkr_shares(
                fill_qty=base_qty,
                min_share_increment=min_share_increment,
            )
            return MakerFillHedgeTranslation(
                venue_qty=fill_qty,
                base_qty=base_qty,
                hedge_qty=hedge_qty,
                qty_conversion_status="identity_fallback",
                qty_conversion_source="maker_instrument:missing_metadata_identity_fallback",
            )
        return MakerFillHedgeTranslation(
            venue_qty=fill_qty,
            base_qty=None,
            hedge_qty=None,
            qty_conversion_status=exposure.qty_conversion_status,
            qty_conversion_source=exposure.qty_conversion_source,
        )

    hedge_qty = round_base_fill_to_ibkr_shares(
        fill_qty=exposure.base_qty,
        min_share_increment=min_share_increment,
    )
    return MakerFillHedgeTranslation(
        venue_qty=fill_qty,
        base_qty=exposure.base_qty,
        hedge_qty=hedge_qty,
        qty_conversion_status=exposure.qty_conversion_status,
        qty_conversion_source=exposure.qty_conversion_source,
    )


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
    "MakerFillHedgeTranslation",
    "round_base_fill_to_ibkr_shares",
    "translate_maker_fill_to_ibkr_shares",
    "translate_hyperliquid_fill_to_ibkr_shares",
]
