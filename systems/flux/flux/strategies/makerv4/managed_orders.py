from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal

from flux.strategies.shared.equities_arb.hedging import HedgeBacklogState
from flux.strategies.shared.equities_arb.hedging import HedgeOrderIntent
from flux.strategies.shared.equities_arb.hedging import PendingHedgeState


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
