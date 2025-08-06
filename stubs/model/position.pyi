from decimal import Decimal
from typing import Any

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from stubs.model.events.order import OrderFilled
from stubs.model.identifiers import AccountId
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import PositionId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import Symbol
from stubs.model.identifiers import TradeId
from stubs.model.identifiers import TraderId
from stubs.model.identifiers import Venue
from stubs.model.identifiers import VenueOrderId
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class Position:

    trader_id: TraderId
    strategy_id: StrategyId
    instrument_id: InstrumentId
    id: PositionId
    account_id: AccountId
    opening_order_id: ClientOrderId
    closing_order_id: ClientOrderId | None
    entry: OrderSide
    side: PositionSide
    signed_qty: float
    quantity: Quantity
    peak_qty: Quantity
    ts_init: int
    ts_opened: int
    ts_last: int
    ts_closed: int
    duration_ns: int
    avg_px_open: float
    avg_px_close: float
    price_precision: int
    size_precision: int
    multiplier: Quantity
    is_inverse: bool
    quote_currency: Currency
    base_currency: Currency | None
    settlement_currency: Currency
    realized_return: float
    realized_pnl: Money | None

    _events: list[OrderFilled]
    _trade_ids: list[TradeId]
    _buy_qty: Quantity
    _sell_qty: Quantity
    _commissions: dict[Currency, Money]

    def __init__(
        self,
        instrument: Instrument,
        fill: OrderFilled,
    ) -> None: ...
    def __eq__(self, other: Position) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    def purge_events_for_order(self, client_order_id: ClientOrderId) -> None: ...
    def info(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...
    @property
    def symbol(self) -> Symbol: ...
    @property
    def venue(self) -> Venue: ...
    @property
    def client_order_ids(self) -> list[ClientOrderId]: ...
    @property
    def venue_order_ids(self) -> list[VenueOrderId]: ...
    @property
    def trade_ids(self) -> list[TradeId]: ...
    @property
    def events(self) -> list[OrderFilled]: ...
    @property
    def last_event(self) -> OrderFilled | None: ...
    @property
    def last_trade_id(self) -> TradeId | None: ...
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
    def closing_order_side(self) -> OrderSide: ...
    def signed_decimal_qty(self) -> Decimal: ...
    def is_opposite_side(self, side: OrderSide) -> bool: ...
    def apply(self, fill: OrderFilled) -> None: ...
    def notional_value(self, price: Price) -> Money: ...
    def calculate_pnl(
        self,
        avg_px_open: float,
        avg_px_close: float,
        quantity: Quantity,
    ) -> Money: ...
    def unrealized_pnl(self, price: Price) -> Money: ...
    def total_pnl(self, price: Price) -> Money: ...
    def commissions(self) -> list[Money]: ...
