from decimal import Decimal
from typing import Any

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import UUID4, OrderInitialized, Symbol, Venue
from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import ExecAlgorithmId
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderStatus
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import TradeId
from nautilus_trader.core.nautilus_pyo3 import TriggerType
from nautilus_trader.core.nautilus_pyo3 import VenueOrderId
from stubs.core.fsm import FiniteStateMachine
from stubs.model.events.order import OrderEvent

STOP_ORDER_TYPES: set[OrderType]
LIMIT_ORDER_TYPES: set[OrderType]
LOCAL_ACTIVE_ORDER_STATUS: set[OrderStatus]

class Order:
    """
    The base class for all orders.

    Parameters
    ----------
    init : OrderInitialized
        The order initialized event.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    trader_id: StrategyId
    strategy_id: StrategyId
    instrument_id: InstrumentId
    client_order_id: ClientOrderId
    venue_order_id: VenueOrderId | None
    position_id: PositionId | None
    account_id: AccountId | None
    last_trade_id: TradeId | None
    side: OrderSide
    order_type: OrderType
    quantity: Quantity
    time_in_force: TimeInForce
    liquidity_side: Any
    is_post_only: bool
    is_reduce_only: bool
    is_quote_quantity: bool
    emulation_trigger: TriggerType | None
    trigger_instrument_id: InstrumentId | None
    contingency_type: Any
    order_list_id: Any | None
    linked_order_ids: list[ClientOrderId] | None
    parent_order_id: ClientOrderId | None
    exec_algorithm_id: ExecAlgorithmId | None
    exec_algorithm_params: dict[str, str] | None
    exec_spawn_id: ClientOrderId | None
    tags: list[str] | None
    filled_qty: Quantity
    leaves_qty: Quantity
    avg_px: float
    slippage: float
    init_id: UUID4
    ts_init: int
    ts_submitted: int
    ts_accepted: int
    ts_closed: int
    ts_last: int

    _events: list[OrderEvent]
    _venue_order_ids: list[VenueOrderId]
    _trade_ids: list[TradeId]
    _commissions: dict[Currency, Money]
    _fsm: FiniteStateMachine
    _previous_status: OrderStatus
    _triggered_price: Price | None

    def __init__(self, init: OrderInitialized) -> None: ...
    def __eq__(self, other: Order) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    def status_string(self) -> str:
        """
        Return the orders current status as a string.

        Returns
        -------
        str

        """
    def side_string(self) -> str:
        """
        Return the orders side as a string.

        Returns
        -------
        str

        """
    def type_string(self) -> str:
        """
        Return the orders type as a string.

        Returns
        -------
        str

        """
    def tif_string(self) -> str:
        """
        Return the orders time in force as a string.

        Returns
        -------
        str

        """
    def info(self) -> str:
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
    def to_dict(self) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
    def to_own_book_order(self) -> nautilus_pyo3.OwnBookOrder:
        """
        Returns an own/user order representation of this order.

        Returns
        -------
        nautilus_pyo3.OwnBookOrder

        """
    @property
    def symbol(self) -> Symbol:
        """
        Return the orders ticker symbol.

        Returns
        -------
        Symbol

        """
    @property
    def venue(self) -> Venue:
        """
        Return the orders trading venue.

        Returns
        -------
        Venue

        """
    @property
    def status(self) -> OrderStatus:
        """
        Return the orders current status.

        Returns
        -------
        OrderStatus

        """
    @property
    def init_event(self) -> OrderInitialized:
        """
        Return the initialization event for the order.

        Returns
        -------
        OrderInitialized

        """
    @property
    def last_event(self) -> OrderEvent:
        """
        Return the last event applied to the order.

        Returns
        -------
        OrderEvent

        """
    @property
    def events(self) -> list[OrderEvent]:
        """
        Return the order events.

        Returns
        -------
        list[OrderEvent]

        """
    @property
    def venue_order_ids(self) -> list[VenueOrderId]:
        """
        Return the venue order IDs.

        Returns
        -------
        list[VenueOrderId]

        """
    @property
    def trade_ids(self) -> list[TradeId]:
        """
        Return the trade match IDs.

        Returns
        -------
        list[TradeId]

        """
    @property
    def event_count(self) -> int:
        """
        Return the count of events applied to the order.

        Returns
        -------
        int

        """
    @property
    def has_price(self) -> bool:
        """
        Return whether the order has a `price` property.

        Returns
        -------
        bool

        """
    @property
    def has_trigger_price(self) -> bool:
        """
        Return whether the order has a `trigger_price` property.

        Returns
        -------
        bool

        """
    @property
    def has_activation_price(self) -> bool:
        """
        Return whether the order has a `activation_price` property.

        Returns
        -------
        bool

        """
    @property
    def is_buy(self) -> bool:
        """
        Return whether the order side is ``BUY``.

        Returns
        -------
        bool

        """
    @property
    def is_sell(self) -> bool:
        """
        Return whether the order side is ``SELL``.

        Returns
        -------
        bool

        """
    @property
    def is_passive(self) -> bool:
        """
        Return whether the order is passive (`order_type` **not** ``MARKET``).

        Returns
        -------
        bool

        """
    @property
    def is_aggressive(self) -> bool:
        """
        Return whether the order is aggressive (`order_type` is ``MARKET``).

        Returns
        -------
        bool

        """
    @property
    def is_emulated(self) -> bool:
        """
        Return whether the order is emulated and held in the local system.

        Returns
        -------
        bool

        """
    @property
    def is_active_local(self) -> bool:
        """
        Return whether the order is active and held in the local system.

        An order is considered active local when its status is any of:
        - ``INITIALIZED``
        - ``EMULATED``
        - ``RELEASED``

        Returns
        -------
        bool

        """
    @property
    def is_primary(self) -> bool:
        """
        Return whether the order is the primary for an execution algorithm sequence.

        Returns
        -------
        bool

        """
    @property
    def is_spawned(self) -> bool:
        """
        Return whether the order was spawned as part of an execution algorithm sequence.

        Returns
        -------
        bool

        """
    @property
    def is_contingency(self) -> bool:
        """
        Return whether the order has a contingency (`contingency_type` is not ``NO_CONTINGENCY``).

        Returns
        -------
        bool

        """
    @property
    def is_parent_order(self) -> bool:
        """
        Return whether the order has **at least** one child order.

        Returns
        -------
        bool

        """
    @property
    def is_child_order(self) -> bool:
        """
        Return whether the order has a parent order.

        Returns
        -------
        bool

        """
    @property
    def is_inflight(self) -> bool:
        """
        Return whether the order is in-flight (order request sent to the trading venue).

        An order is considered in-flight when its status is any of:
        - ``SUBMITTED``
        - ``PENDING_UPDATE``
        - ``PENDING_CANCEL``

        Returns
        -------
        bool

        Warnings
        --------
        An emulated order is never considered in-flight.

        """
    @property
    def is_open(self) -> bool:
        """
        Return whether the order is open at the trading venue.

        An order is considered open when its status is any of:
        - ``ACCEPTED``
        - ``TRIGGERED``
        - ``PENDING_UPDATE``
        - ``PENDING_CANCEL``
        - ``PARTIALLY_FILLED``

        Returns
        -------
        bool

        Warnings
        --------
        An emulated order is never considered open.

        """
    @property
    def is_canceled(self) -> bool:
        """
        Return whether current `status` is ``CANCELED``.

        Returns
        -------
        bool

        """
    @property
    def is_closed(self) -> bool:
        """
        Return whether the order is closed (lifecycle completed).

        An order is considered closed when its status can no longer change.
        The possible statuses of closed orders include;

        - ``DENIED``
        - ``REJECTED``
        - ``CANCELED``
        - ``EXPIRED``
        - ``FILLED``

        Returns
        -------
        bool

        """
    @property
    def is_pending_update(self) -> bool:
        """
        Return whether the current `status` is ``PENDING_UPDATE``.

        Returns
        -------
        bool

        """
    @property
    def is_pending_cancel(self) -> bool:
        """
        Return whether the current `status` is ``PENDING_CANCEL``.

        Returns
        -------
        bool

        """
    @staticmethod
    def opposite_side(side: OrderSide) -> OrderSide:
        """
        Return the opposite order side from the given side.

        Parameters
        ----------
        side : OrderSide {``BUY``, ``SELL``}
            The original order side.

        Returns
        -------
        OrderSide

        Raises
        ------
        ValueError
            If `side` is invalid.

        """
    @staticmethod
    def closing_side(position_side: PositionSide) -> OrderSide:
        """
        Return the order side needed to close a position with the given side.

        Parameters
        ----------
        position_side : PositionSide {``LONG``, ``SHORT``}
            The side of the position to close.

        Returns
        -------
        OrderSide

        Raises
        ------
        ValueError
            If `position_side` is ``FLAT`` or invalid.

        """
    def signed_decimal_qty(self) -> Decimal:
        """
        Return a signed decimal representation of the remaining quantity.

         - If the order is a BUY, the value is positive (e.g. Decimal('10.25'))
         - If the order is a SELL, the value is negative (e.g. Decimal('-10.25'))

        Returns
        -------
        Decimal

        """
    def would_reduce_only(self, position_side: PositionSide, position_qty: Quantity) -> bool:
        """
        Whether the current order would only reduce the given position if applied
        in full.

        Parameters
        ----------
        position_side : PositionSide {``FLAT``, ``LONG``, ``SHORT``}
            The side of the position to check against.
        position_qty : Quantity
            The quantity of the position to check against.

        Returns
        -------
        bool

        """
    def commissions(self) -> list[Money]:
        """
        Return the total commissions generated by the order.

        Returns
        -------
        list[Money]

        """
    def apply(self, event: Any) -> None:
        """
        Apply the given order event to the order.

        Parameters
        ----------
        event : OrderEvent
            The order event to apply.

        Raises
        ------
        ValueError
            If `self.client_order_id` is not equal to `event.client_order_id`.
        ValueError
            If `self.venue_order_id` and `event.venue_order_id` are both not ``None``, and are not equal.
        InvalidStateTrigger
            If `event` is not a valid trigger from the current `order.status`.
        KeyError
            If `event` is `OrderFilled` and `event.trade_id` already applied to the order.

        """