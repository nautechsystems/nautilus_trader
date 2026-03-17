from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal


@dataclass(frozen=True, slots=True)
class HedgeOrderIntent:
    instrument_id: str
    side: str
    qty: Decimal
    limit_price: Decimal
    route: str = "SMART"
    time_in_force: str = "IOC"
    outside_rth: bool = False
    include_overnight: bool = False
    cancel_after_ms: int | None = None


@dataclass(frozen=True, slots=True)
class PendingHedgeState:
    fill_id: str
    side: str
    requested_qty: Decimal
    remaining_qty: Decimal
    limit_price: Decimal
    route: str
    time_in_force: str
    outside_rth: bool
    include_overnight: bool = False
    cancel_after_ms: int | None = None
    order_id: str | None = None


@dataclass(frozen=True, slots=True)
class HedgeBacklogState:
    fill_id: str
    side: str
    requested_qty: Decimal
    blocked_reason: str
    fill_ts_ms: int
    maker_fee_bps: Decimal


@dataclass(frozen=True, slots=True)
class ManagedMakerOrderState:
    client_order_id: str
    instrument_id: str
    side: str
    quantity: Decimal
    price: Decimal
    post_only: bool = True
    pending_cancel: bool = False


__all__ = [
    "HedgeBacklogState",
    "HedgeOrderIntent",
    "ManagedMakerOrderState",
    "PendingHedgeState",
]
