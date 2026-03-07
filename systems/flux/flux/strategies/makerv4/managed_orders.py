from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal


@dataclass(frozen=True, slots=True)
class HedgeOrderIntent:
    instrument_id: str
    side: str
    qty: Decimal
    limit_price: Decimal
    time_in_force: str = "IOC"
    outside_rth: bool = False


@dataclass(frozen=True, slots=True)
class PendingHedgeState:
    fill_id: str
    side: str
    requested_qty: Decimal
    remaining_qty: Decimal
    limit_price: Decimal
    outside_rth: bool
    order_id: str | None = None


__all__ = [
    "HedgeOrderIntent",
    "PendingHedgeState",
]
