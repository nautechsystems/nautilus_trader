from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal


@dataclass(frozen=True, slots=True)
class IbkrQuoteSnapshot:
    instrument_id: str
    bid: Decimal | None
    ask: Decimal | None
    age_ms: int
    ts_ms: int
    bid_size: Decimal | None = None
    ask_size: Decimal | None = None

    @property
    def mid(self) -> Decimal | None:
        if self.bid is None or self.ask is None:
            return None
        return (self.bid + self.ask) / Decimal("2")


__all__ = [
    "IbkrQuoteSnapshot",
]
