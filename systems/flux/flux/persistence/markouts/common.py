from __future__ import annotations

from decimal import Decimal
from decimal import InvalidOperation
from typing import Any


def decimal_text(value: Decimal | None) -> str | None:
    if value is None:
        return None
    text = format(value, "f")
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    if text == "-0":
        return "0"
    return text or "0"


def to_decimal(value: Any) -> Decimal | None:
    if value is None or value == "" or isinstance(value, bool):
        return None
    try:
        return Decimal(str(value))
    except (InvalidOperation, TypeError, ValueError):
        text = str(value).strip()
        if not text:
            return None
        try:
            return Decimal(text)
        except (InvalidOperation, ValueError):
            return None


def to_optional_int(value: Any) -> int | None:
    if value is None or value == "":
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        text = str(value).strip()
        if not text:
            return None
        try:
            return int(text)
        except ValueError:
            return None


def to_optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def markout_bps(markout_abs: Decimal | None, fill_px: Decimal) -> Decimal | None:
    if markout_abs is None or fill_px <= 0:
        return None
    return (markout_abs / fill_px) * Decimal(10000)


def signed_markout(side: str, fill_px: Decimal, benchmark_px: Decimal) -> Decimal:
    side_upper = side.upper()
    if side_upper == "BUY":
        return benchmark_px - fill_px
    if side_upper == "SELL":
        return fill_px - benchmark_px
    raise ValueError(f"Unsupported side {side!r}")
