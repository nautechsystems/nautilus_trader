from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
from typing import Any

from flux.common.quantity_units import exposure_from_venue_qty
from nautilus_trader.model.instruments.base import Instrument


@dataclass(frozen=True, slots=True)
class OperatorQuantitySnapshot:
    qty_venue: str | None
    qty_base: str | None
    qty_conversion_status: str
    qty_conversion_source: str


def snapshot_operator_quantity(
    instrument: Instrument | None,
    venue_qty: Any,
    *,
    last_px: Any = None,
    missing_metadata_source: str = "persistence:instrument unavailable",
) -> OperatorQuantitySnapshot:
    raw_qty = _raw_qty_text(venue_qty)
    if raw_qty is None:
        raise ValueError("venue_qty is required")

    if instrument is None:
        return OperatorQuantitySnapshot(
            qty_venue=raw_qty,
            qty_base=None,
            qty_conversion_status="missing_metadata",
            qty_conversion_source=missing_metadata_source,
        )

    exposure = exposure_from_venue_qty(instrument, venue_qty, last_px=last_px)
    return OperatorQuantitySnapshot(
        qty_venue=_decimal_to_text(exposure.venue_qty),
        qty_base=_decimal_to_text(exposure.base_qty),
        qty_conversion_status=exposure.qty_conversion_status,
        qty_conversion_source=exposure.qty_conversion_source,
    )


def _raw_qty_text(value: Any) -> str | None:
    if value is None:
        return None
    return str(value)


def _decimal_to_text(value: Decimal | None) -> str | None:
    if value is None:
        return None
    normalized = value.normalize()
    return format(normalized, "f")
