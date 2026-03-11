from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from decimal import Decimal
from typing import Any

from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


OKX_QUANTITY_UNIT_KEYS = (
    "okx_ct_val",
    "okx_ct_val_ccy",
    "okx_ct_type",
    "okx_lot_sz",
)
KNOWN_BASE_EXPOSURE_MODES = {
    "identity",
    "exact_multiplier",
    "price_based",
    "unsupported",
}
VALID_ORDER_QTY_UNITS = frozenset({"venue", "base"})


@dataclass(frozen=True, slots=True)
class QuantityExposure:
    venue_qty: Decimal | None
    base_qty: Decimal | None
    qty_conversion_status: str
    qty_conversion_source: str


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def normalize_order_qty_unit(
    value: Any,
    *,
    context: str = "qty_unit",
) -> str:
    text = _optional_text(value)
    if text is None:
        raise ValueError(f"Missing qty_unit for {context}")

    qty_unit = text.lower()
    if qty_unit not in VALID_ORDER_QTY_UNITS:
        raise ValueError(
            f"Unsupported qty_unit for {context}: {value!r}. Expected one of {sorted(VALID_ORDER_QTY_UNITS)!r}",
        )
    return qty_unit


def _decimal_from_value(value: Any) -> Decimal | None:
    if value is None:
        return None

    as_decimal = getattr(value, "as_decimal", None)
    if callable(as_decimal):
        return as_decimal()

    if isinstance(value, Decimal):
        return value

    return Decimal(str(value))


def _currency_code(value: Any) -> str:
    if value is None:
        return ""
    code = getattr(value, "code", None)
    if code is not None:
        return str(code).strip().upper()
    return str(value).strip().upper()


def _instrument_info(instrument: Instrument) -> Mapping[str, Any]:
    info = getattr(instrument, "info", None)
    if isinstance(info, Mapping):
        return info
    return {}


def _classify_from_info(
    instrument: Instrument,
    *,
    last_px: Any = None,
) -> tuple[str, str] | None:
    info = _instrument_info(instrument)
    mode = _optional_text(info.get("base_exposure_mode"))
    if mode is None:
        return None

    mode = mode.lower()
    if mode not in KNOWN_BASE_EXPOSURE_MODES:
        return None

    has_any_okx_key = any(key in info for key in OKX_QUANTITY_UNIT_KEYS)
    has_incomplete_okx_metadata = has_any_okx_key and any(
        _optional_text(info.get(key)) is None for key in OKX_QUANTITY_UNIT_KEYS
    )
    if has_incomplete_okx_metadata:
        return "missing_metadata", "instrument.info:incomplete_okx_quantity_unit_metadata"

    if mode == "price_based" and last_px is None:
        return "missing_price", "instrument.info:base_exposure_mode=price_based requires last_px"

    return mode, f"instrument.info:base_exposure_mode={mode}"


def _classify_generically(
    instrument: Instrument,
    *,
    last_px: Any = None,
) -> tuple[str, str]:
    multiplier = _decimal_from_value(getattr(instrument, "multiplier", None))
    if multiplier is None:
        return "missing_metadata", "generic:instrument multiplier unavailable"

    base_currency = _currency_code(getattr(instrument, "base_currency", None))
    quote_currency = _currency_code(getattr(instrument, "quote_currency", None))
    settlement_currency = _currency_code(getattr(instrument, "settlement_currency", None))
    is_inverse = bool(getattr(instrument, "is_inverse", False))

    if settlement_currency and base_currency and quote_currency:
        if settlement_currency not in {base_currency, quote_currency}:
            return "unsupported", "generic:quanto instrument"

    if is_inverse:
        if last_px is None:
            return "missing_price", "generic:inverse instrument requires last_px"
        return "price_based", "generic:is_inverse"

    if multiplier == Decimal(1):
        return "identity", "generic:multiplier=1"

    return "exact_multiplier", "generic:multiplier"


def _classify_conversion(
    instrument: Instrument,
    *,
    last_px: Any = None,
) -> tuple[str, str]:
    classified = _classify_from_info(instrument, last_px=last_px)
    if classified is not None:
        return classified

    return _classify_generically(instrument, last_px=last_px)


def _to_quantity(instrument: Instrument, value: Any) -> Quantity:
    if isinstance(value, Quantity):
        return value
    return instrument.make_qty(value)


def _to_price(instrument: Instrument, value: Any) -> Price:
    if isinstance(value, Price):
        return value
    make_price = getattr(instrument, "make_price", None)
    if callable(make_price):
        return make_price(value)
    return Price.from_str(str(value))


def _degraded_result(
    *,
    venue_qty: Decimal | None,
    base_qty: Decimal | None,
    status: str,
    source: str,
) -> QuantityExposure:
    return QuantityExposure(
        venue_qty=venue_qty,
        base_qty=base_qty,
        qty_conversion_status=status,
        qty_conversion_source=source,
    )


def _sign_multiplier(value: Decimal) -> Decimal:
    return Decimal("-1") if value < 0 else Decimal("1")


def exposure_from_venue_qty(
    instrument: Instrument,
    venue_qty: Any,
    last_px: Any = None,
) -> QuantityExposure:
    venue_qty_dec = _decimal_from_value(venue_qty)
    if venue_qty_dec is None:
        raise ValueError("venue_qty is required")

    status, source = _classify_conversion(instrument, last_px=last_px)
    if status in {"missing_metadata", "missing_price", "unsupported"}:
        return _degraded_result(
            venue_qty=venue_qty_dec,
            base_qty=None,
            status=status,
            source=source,
        )

    sign = _sign_multiplier(venue_qty_dec)
    qty = _to_quantity(instrument, abs(venue_qty_dec))
    price = _to_price(instrument, last_px) if last_px is not None else None

    try:
        base_qty = instrument.calculate_base_exposure_qty(qty, price)
    except ValueError as exc:
        message = str(exc)
        if "last_px" in message:
            return _degraded_result(
                venue_qty=venue_qty_dec,
                base_qty=None,
                status="missing_price",
                source=f"{source} requires last_px",
            )
        if "Quanto" in message or "not supported" in message:
            return _degraded_result(
                venue_qty=venue_qty_dec,
                base_qty=None,
                status="unsupported",
                source=source,
            )
        raise

    return QuantityExposure(
        venue_qty=venue_qty_dec,
        base_qty=(_decimal_from_value(base_qty) or Decimal(0)) * sign,
        qty_conversion_status=status,
        qty_conversion_source=source,
    )


def venue_qty_from_base_qty(
    instrument: Instrument,
    base_qty: Any,
    last_px: Any = None,
) -> QuantityExposure:
    base_qty_dec = _decimal_from_value(base_qty)
    if base_qty_dec is None:
        raise ValueError("base_qty is required")

    status, source = _classify_conversion(instrument, last_px=last_px)
    if status in {"missing_metadata", "missing_price", "unsupported"}:
        return _degraded_result(
            venue_qty=None,
            base_qty=base_qty_dec,
            status=status,
            source=source,
        )

    multiplier = _decimal_from_value(getattr(instrument, "multiplier", None))
    if multiplier is None or multiplier == 0:
        return _degraded_result(
            venue_qty=None,
            base_qty=base_qty_dec,
            status="missing_metadata",
            source="generic:instrument multiplier unavailable",
        )

    sign = _sign_multiplier(base_qty_dec)
    base_qty_abs = abs(base_qty_dec)

    if status == "identity":
        candidate_venue_qty = base_qty_abs
    elif status == "exact_multiplier":
        candidate_venue_qty = base_qty_abs / multiplier
    else:
        last_px_dec = _decimal_from_value(last_px)
        if last_px_dec is None:
            return _degraded_result(
                venue_qty=None,
                base_qty=base_qty_dec,
                status="missing_price",
                source=f"{source} requires last_px",
            )
        candidate_venue_qty = (base_qty_abs * last_px_dec) / multiplier

    try:
        venue_qty_obj = _to_quantity(instrument, candidate_venue_qty)
    except ValueError:
        return _degraded_result(
            venue_qty=None,
            base_qty=base_qty_dec,
            status="non_integral_venue_qty",
            source=f"{source}:base_qty_not_representable",
        )

    venue_qty_dec = _decimal_from_value(venue_qty_obj)
    if venue_qty_dec is None:
        return _degraded_result(
            venue_qty=None,
            base_qty=base_qty_dec,
            status="missing_metadata",
            source="generic:failed to normalize venue quantity",
        )

    venue_qty_dec *= sign
    roundtrip = exposure_from_venue_qty(instrument, venue_qty_dec, last_px=last_px)
    if roundtrip.base_qty != base_qty_dec:
        return _degraded_result(
            venue_qty=None,
            base_qty=base_qty_dec,
            status="non_integral_venue_qty",
            source=f"{source}:base_qty_not_representable",
        )

    return QuantityExposure(
        venue_qty=venue_qty_dec,
        base_qty=base_qty_dec,
        qty_conversion_status=status,
        qty_conversion_source=source,
    )


__all__ = [
    "QuantityExposure",
    "exposure_from_venue_qty",
    "normalize_order_qty_unit",
    "venue_qty_from_base_qty",
]
