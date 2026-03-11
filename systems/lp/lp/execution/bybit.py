from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
from typing import Protocol
from typing import runtime_checkable


@dataclass(frozen=True, slots=True)
class MarketOrderRequest:
    symbol: str
    side: str
    qty: Decimal
    max_slippage_bps: Decimal


@runtime_checkable
class PerpExecutionClient(Protocol):
    def get_position_size(self, symbol: str) -> Decimal: ...

    def get_mark_price(self, symbol: str) -> Decimal: ...

    def create_market_order(self, order: MarketOrderRequest) -> bool: ...


class BybitPerpClient:
    def __init__(self, client: object) -> None:
        self._client = client

    def get_position_size(self, symbol: str) -> Decimal:
        getter = self._client.get_position_size
        return Decimal(str(getter(symbol)))

    def get_mark_price(self, symbol: str) -> Decimal:
        getter = self._client.get_mark_price
        return Decimal(str(getter(symbol)))

    def create_market_order(self, order: MarketOrderRequest) -> bool:
        creator = self._client.create_market_order
        try:
            result = creator(order)
        except TypeError:
            result = creator(order.symbol, order.side, order.qty)
        return bool(result)


__all__ = ["BybitPerpClient", "MarketOrderRequest", "PerpExecutionClient"]
