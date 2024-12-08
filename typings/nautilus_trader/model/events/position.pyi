from typing import Dict, Optional

from nautilus_trader.core.message import Event
from nautilus_trader.core.model import OrderSide, PositionSide
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.identifiers import (
    AccountId,
    ClientOrderId,
    InstrumentId,
    PositionId,
    StrategyId,
    TraderId,
)
from nautilus_trader.model.objects import Currency, Money, Price, Quantity
from nautilus_trader.model.position import Position

class PositionEvent(Event):
    trader_id: TraderId
    strategy_id: StrategyId
    instrument_id: InstrumentId
    position_id: PositionId
    account_id: AccountId
    opening_order_id: ClientOrderId
    closing_order_id: Optional[ClientOrderId]
    entry: OrderSide
    side: PositionSide
    signed_qty: float
    quantity: Quantity
    peak_qty: Quantity
    last_qty: Quantity
    last_px: Price
    currency: Currency
    avg_px_open: float
    avg_px_close: float
    realized_return: float
    realized_pnl: Money
    unrealized_pnl: Money
    ts_opened: int
    ts_closed: int
    duration_ns: int

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        position_id: PositionId,
        account_id: AccountId,
        opening_order_id: ClientOrderId,
        closing_order_id: Optional[ClientOrderId],
        entry: OrderSide,
        side: PositionSide,
        signed_qty: float,
        quantity: Quantity,
        peak_qty: Quantity,
        last_qty: Quantity,
        last_px: Price,
        currency: Currency,
        avg_px_open: float,
        avg_px_close: float,
        realized_return: float,
        realized_pnl: Money,
        unrealized_pnl: Money,
        event_id: UUID4,
        ts_opened: int,
        ts_closed: int,
        duration_ns: int,
        ts_event: int,
        ts_init: int,
    ) -> None: ...

class PositionOpened(PositionEvent):
    @staticmethod
    def create(
        position: Position,
        fill: OrderFilled,
        event_id: UUID4,
        ts_init: int,
    ) -> PositionOpened: ...
    @staticmethod
    def from_dict(values: Dict[str, object]) -> PositionOpened: ...
    @staticmethod
    def to_dict(obj: PositionOpened) -> Dict[str, object]: ...

class PositionChanged(PositionEvent):
    @staticmethod
    def create(
        position: Position,
        fill: OrderFilled,
        event_id: UUID4,
        ts_init: int,
    ) -> PositionChanged: ...
    @staticmethod
    def from_dict(values: Dict[str, object]) -> PositionChanged: ...
    @staticmethod
    def to_dict(obj: PositionChanged) -> Dict[str, object]: ...

class PositionClosed(PositionEvent):
    @staticmethod
    def create(
        position: Position,
        fill: OrderFilled,
        event_id: UUID4,
        ts_init: int,
    ) -> PositionClosed: ...
    @staticmethod
    def from_dict(values: Dict[str, object]) -> PositionClosed: ...
    @staticmethod
    def to_dict(obj: PositionClosed) -> Dict[str, object]: ...
