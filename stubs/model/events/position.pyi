from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from stubs.core.message import Event
from stubs.core.uuid import UUID4
from stubs.model.events.order import OrderFilled
from stubs.model.identifiers import AccountId
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import PositionId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import TraderId
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.position import Position

class PositionEvent(Event):

    trader_id: TraderId
    strategy_id: StrategyId
    instrument_id: InstrumentId
    position_id: PositionId
    account_id: AccountId
    opening_order_id: ClientOrderId
    closing_order_id: ClientOrderId | None
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

    _event_id: UUID4
    _ts_event: int
    _ts_init: int

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        position_id: PositionId,
        account_id: AccountId,
        opening_order_id: ClientOrderId,
        closing_order_id: ClientOrderId | None,
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
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def id(self) -> UUID4: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...

class PositionOpened(PositionEvent):

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        position_id: PositionId,
        account_id: AccountId,
        opening_order_id: ClientOrderId,
        entry: OrderSide,
        side: PositionSide,
        signed_qty: float,
        quantity: Quantity,
        peak_qty: Quantity,
        last_qty: Quantity,
        last_px: Price,
        currency: Currency,
        avg_px_open: float,
        realized_pnl: Money,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @staticmethod
    def create(
        position: Position,
        fill: OrderFilled,
        event_id: UUID4,
        ts_init: int,
    ) -> PositionOpened: ...
    @staticmethod
    def from_dict(values: dict[str, object]) -> PositionOpened: ...
    @staticmethod
    def to_dict(obj: PositionOpened) -> dict[str, object]: ...

class PositionChanged(PositionEvent):

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        position_id: PositionId,
        account_id: AccountId,
        opening_order_id: ClientOrderId,
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
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @staticmethod
    def create(
        position: Position,
        fill: OrderFilled,
        event_id: UUID4,
        ts_init: int,
    ) -> PositionChanged: ...
    @staticmethod
    def from_dict(values: dict[str, object]) -> PositionChanged: ...
    @staticmethod
    def to_dict(obj: PositionChanged) -> dict[str, object]: ...

class PositionClosed(PositionEvent):

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        position_id: PositionId,
        account_id: AccountId,
        opening_order_id: ClientOrderId,
        closing_order_id: ClientOrderId,
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
        event_id: UUID4,
        ts_opened: int,
        ts_closed: int,
        duration_ns: int,
        ts_init: int,
    ) -> None: ...
    @staticmethod
    def create(
        position: Position,
        fill: OrderFilled,
        event_id: UUID4,
        ts_init: int,
    ) -> PositionClosed: ...
    @staticmethod
    def from_dict(values: dict[str, object]) -> PositionClosed: ...
    @staticmethod
    def to_dict(obj: PositionClosed) -> dict[str, object]: ...