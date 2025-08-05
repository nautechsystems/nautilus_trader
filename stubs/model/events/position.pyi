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
    """
    The base class for all position events.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    position_id : PositionId
        The position IDt.
    account_id : AccountId
        The strategy ID.
    opening_order_id : ClientOrderId
        The client order ID for the order which opened the position.
    closing_order_id : ClientOrderId
        The client order ID for the order which closed the position.
    entry : OrderSide {``BUY``, ``SELL``}
        The position entry order side.
    side : PositionSide {``FLAT``, ``LONG``, ``SHORT``}
        The current position side.
    signed_qty : double
        The current signed quantity (positive for ``LONG``, negative for ``SHORT``).
    quantity : Quantity
        The current open quantity.
    peak_qty : Quantity
        The peak directional quantity reached by the position.
    last_qty : Quantity
        The last fill quantity for the position.
    last_px : Price
        The last fill price for the position (not average price).
    currency : Currency
        The position quote currency.
    avg_px_open : double
        The average open price.
    avg_px_close : double
        The average close price.
    realized_return : double
        The realized return for the position.
    realized_pnl : Money
        The realized PnL for the position.
    unrealized_pnl : Money
        The unrealized PnL for the position.
    event_id : UUID4
        The event ID.
    ts_opened : uint64_t
        UNIX timestamp (nanoseconds) when the position opened event occurred.
    ts_closed : uint64_t
        UNIX timestamp (nanoseconds) when the position closed event occurred.
    duration_ns : uint64_t
        The total open duration (nanoseconds), will be 0 if still open.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

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
    def id(self) -> UUID4:
        """
        The event message identifier.

        Returns
        -------
        UUID4

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the event occurred.

        Returns
        -------
        int

        """
        ...
    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        ...

class PositionOpened(PositionEvent):
    """
    Represents an event where a position has been opened.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    position_id : PositionId
        The position IDt.
    account_id : AccountId
        The strategy ID.
    opening_order_id : ClientOrderId
        The client order ID for the order which opened the position.
    strategy_id : StrategyId
        The strategy ID associated with the event.
    entry : OrderSide {``BUY``, ``SELL``}
        The position entry order side.
    side : PositionSide {``LONG``, ``SHORT``}
        The current position side.
    signed_qty : double
        The current signed quantity (positive for ``LONG``, negative for ``SHORT``).
    quantity : Quantity
        The current open quantity.
    peak_qty : Quantity
        The peak directional quantity reached by the position.
    last_qty : Quantity
        The last fill quantity for the position.
    last_px : Price
        The last fill price for the position (not average price).
    currency : Currency
        The position quote currency.
    avg_px_open : double
        The average open price.
    realized_pnl : Money
        The realized PnL for the position.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the position opened event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

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
    ) -> PositionOpened:
        """
        Return a position opened event from the given params.

        Parameters
        ----------
        position : Position
            The position for the event.
        fill : OrderFilled
            The order fill for the event.
        event_id : UUID4
            The event ID.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        PositionOpened

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, object]) -> PositionOpened:
        """
        Return a position opened event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        PositionOpened

        """
        ...
    @staticmethod
    def to_dict(obj: PositionOpened) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class PositionChanged(PositionEvent):
    """
    Represents an event where a position has changed.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    position_id : PositionId
        The position IDt.
    account_id : AccountId
        The strategy ID.
    opening_order_id : ClientOrderId
        The client order ID for the order which opened the position.
    strategy_id : StrategyId
        The strategy ID associated with the event.
    entry : OrderSide {``BUY``, ``SELL``}
        The position entry order side.
    side : PositionSide {``FLAT``, ``LONG``, ``SHORT``}
        The current position side.
    signed_qty : double
        The current signed quantity (positive for ``LONG``, negative for ``SHORT``).
    quantity : Quantity
        The current open quantity.
    peak_qty : Quantity
        The peak directional quantity reached by the position.
    last_qty : Quantity
        The last fill quantity for the position.
    last_px : Price
        The last fill price for the position (not average price).
    currency : Currency
        The position quote currency.
    avg_px_open : double
        The average open price.
    avg_px_close : double
        The average close price.
    realized_return : double
        The realized return for the position.
    realized_pnl : Money
        The realized PnL for the position.
    unrealized_pnl : Money
        The unrealized PnL for the position.
    event_id : UUID4
        The event ID.
    ts_opened : uint64_t
        UNIX timestamp (nanoseconds) when the position opened event occurred.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the position changed event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

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
    ) -> PositionChanged:
        """
        Return a position changed event from the given params.

        Parameters
        ----------
        position : Position
            The position for the event.
        fill : OrderFilled
            The order fill for the event.
        event_id : UUID4
            The event ID.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        PositionChanged

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, object]) -> PositionChanged:
        """
        Return a position changed event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        PositionChanged

        """
        ...
    @staticmethod
    def to_dict(obj: PositionChanged) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class PositionClosed(PositionEvent):
    """
    Represents an event where a position has been closed.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    position_id : PositionId
        The position IDt.
    account_id : AccountId
        The strategy ID.
    opening_order_id : ClientOrderId
        The client order ID for the order which opened the position.
    closing_order_id : ClientOrderId
        The client order ID for the order which closed the position.
    strategy_id : StrategyId
        The strategy ID associated with the event.
    entry : OrderSide {``BUY``, ``SELL``}
        The position entry order side.
    side : PositionSide {``FLAT``}
        The current position side.
    signed_qty : double
        The current signed quantity (positive for ``LONG``, negative for ``SHORT``).
    quantity : Quantity
        The current open quantity.
    peak_qty : Quantity
        The peak directional quantity reached by the position.
    last_qty : Quantity
        The last fill quantity for the position.
    last_px : Price
        The last fill price for the position (not average price).
    currency : Currency
        The position quote currency.
    avg_px_open : Decimal
        The average open price.
    avg_px_close : Decimal
        The average close price.
    realized_return : Decimal
        The realized return for the position.
    realized_pnl : Money
        The realized PnL for the position.
    event_id : UUID4
        The event ID.
    ts_opened : uint64_t
        UNIX timestamp (nanoseconds) when the position opened event occurred.
    ts_closed : uint64_t
        UNIX timestamp (nanoseconds) when the position closed event occurred.
    duration_ns : uint64_t
        The total open duration (nanoseconds).
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

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
    ) -> PositionClosed:
        """
        Return a position closed event from the given params.

        Parameters
        ----------
        position : Position
            The position for the event.
        fill : OrderFilled
            The order fill for the event.
        event_id : UUID4
            The event ID.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        PositionClosed

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, object]) -> PositionClosed:
        """
        Return a position closed event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        PositionClosed

        """
        ...
    @staticmethod
    def to_dict(obj: PositionClosed) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...