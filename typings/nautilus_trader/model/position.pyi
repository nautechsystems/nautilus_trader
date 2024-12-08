from decimal import Decimal
from typing import List, Optional

from nautilus_trader.core.model import OrderSide, PositionSide
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.identifiers import (
    AccountId,
    ClientOrderId,
    InstrumentId,
    PositionId,
    StrategyId,
    Symbol,
    TradeId,
    TraderId,
    Venue,
    VenueOrderId,
)
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Currency, Money, Price, Quantity

class Position:
    trader_id: TraderId
    strategy_id: StrategyId
    instrument_id: InstrumentId
    id: PositionId
    account_id: AccountId
    opening_order_id: ClientOrderId
    closing_order_id: Optional[ClientOrderId]
    entry: OrderSide
    side: PositionSide
    signed_qty: float
    quantity: Quantity
    peak_qty: Quantity
    price_precision: int
    size_precision: int
    multiplier: Quantity
    is_inverse: bool
    quote_currency: Currency
    base_currency: Optional[Currency]
    settlement_currency: Currency
    ts_init: int
    ts_opened: int
    ts_last: int
    ts_closed: int
    duration_ns: int
    avg_px_open: float
    avg_px_close: float
    realized_return: float
    realized_pnl: Optional[Money]

    def __init__(self, instrument: Instrument, fill: OrderFilled) -> None: ...
    def __eq__(self, other: Position) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    def info(self) -> str: ...
    def to_dict(self) -> dict: ...
    @property
    def symbol(self) -> Symbol: ...
    @property
    def venue(self) -> Venue: ...
    @property
    def client_order_ids(self) -> List[ClientOrderId]: ...
    @property
    def venue_order_ids(self) -> List[VenueOrderId]: ...
    @property
    def trade_ids(self) -> List[TradeId]: ...
    @property
    def events(self) -> List[OrderFilled]: ...
    @property
    def last_event(self) -> OrderFilled: ...
    @property
    def last_trade_id(self) -> TradeId: ...
    @property
    def event_count(self) -> int: ...
    @property
    def is_open(self) -> bool: ...
    @property
    def is_closed(self) -> bool: ...
    @property
    def is_long(self) -> bool: ...
    @property
    def is_short(self) -> bool: ...
    @staticmethod
    def side_from_order_side(side: OrderSide) -> PositionSide: ...
    def signed_decimal_qty(self) -> Decimal: ...
    def is_opposite_side(self, side: OrderSide) -> bool: ...
    def apply(self, fill: OrderFilled) -> None: ...
    def notional_value(self, last: Price) -> Money: ...
    def calculate_pnl(
        self, avg_px_open: float, avg_px_close: float, quantity: Quantity
    ) -> Money: ...
    def unrealized_pnl(self, last: Price) -> Money: ...
    def total_pnl(self, last: Price) -> Money: ...
    def commissions(self) -> List[Money]: ...
