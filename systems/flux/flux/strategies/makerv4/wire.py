from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal


@dataclass(frozen=True, slots=True)
class MakerFill:
    fill_id: str
    side: str
    qty: Decimal
    price: Decimal
    ts_ms: int


@dataclass(frozen=True, slots=True)
class HedgeExecutionReport:
    order_id: str
    ok: bool
    filled_qty: Decimal
    avg_fill_price: Decimal | None
    error: str | None = None


__all__ = [
    "HedgeExecutionReport",
    "MakerFill",
]
