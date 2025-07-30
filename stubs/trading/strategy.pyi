from typing import Any

from nautilus_trader.core.nautilus_pyo3 import Actor
from nautilus_trader.core.nautilus_pyo3 import Cache
from nautilus_trader.core.nautilus_pyo3 import ClientId
from nautilus_trader.core.nautilus_pyo3 import Clock
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import MessageBus
from nautilus_trader.core.nautilus_pyo3 import OmsType
from nautilus_trader.core.nautilus_pyo3 import Order
from nautilus_trader.core.nautilus_pyo3 import OrderAccepted
from nautilus_trader.core.nautilus_pyo3 import OrderCanceled
from nautilus_trader.core.nautilus_pyo3 import OrderCancelRejected
from nautilus_trader.core.nautilus_pyo3 import OrderDenied
from nautilus_trader.core.nautilus_pyo3 import OrderEmulated
from nautilus_trader.core.nautilus_pyo3 import OrderEvent
from nautilus_trader.core.nautilus_pyo3 import OrderExpired
from nautilus_trader.core.nautilus_pyo3 import OrderFactory
from nautilus_trader.core.nautilus_pyo3 import OrderFilled
from nautilus_trader.core.nautilus_pyo3 import OrderInitialized
from nautilus_trader.core.nautilus_pyo3 import OrderList
from nautilus_trader.core.nautilus_pyo3 import OrderModifyRejected
from nautilus_trader.core.nautilus_pyo3 import OrderPendingCancel
from nautilus_trader.core.nautilus_pyo3 import OrderPendingUpdate
from nautilus_trader.core.nautilus_pyo3 import OrderRejected
from nautilus_trader.core.nautilus_pyo3 import OrderReleased
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderSubmitted
from nautilus_trader.core.nautilus_pyo3 import OrderTriggered
from nautilus_trader.core.nautilus_pyo3 import OrderUpdated
from nautilus_trader.core.nautilus_pyo3 import PortfolioFacade
from nautilus_trader.core.nautilus_pyo3 import Position
from nautilus_trader.core.nautilus_pyo3 import PositionChanged
from nautilus_trader.core.nautilus_pyo3 import PositionClosed
from nautilus_trader.core.nautilus_pyo3 import PositionEvent
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import PositionOpened
from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import TraderId
from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.config import StrategyConfig

class Strategy(Actor):
    """
    The base class for all trading strategies.

    This class allows traders to implement their own customized trading strategies.
    A trading strategy can configure its own order management system type, which
    determines how positions are handled by the `ExecutionEngine`.

    Strategy OMS (Order Management System) types:
     - ``UNSPECIFIED``: No specific type has been configured, will therefore
       default to the native OMS type for each venue.
     - ``HEDGING``: A position ID will be assigned for each new position which
       is opened per instrument.
     - ``NETTING``: There will only be a single position for the strategy per
       instrument. The position ID naming convention is `{instrument_id}-{strategy_id}`.

    Parameters
    ----------
    config : StrategyConfig, optional
        The trading strategy configuration.

    Raises
    ------
    TypeError
        If `config` is not of type `StrategyConfig`.

    Warnings
    --------
    - This class should not be used directly, but through a concrete subclass.
    - Do not call components such as `clock` and `logger` in the `__init__` prior to registration.
    """

    id: StrategyId
    order_id_tag: str
    use_uuid_client_order_ids: bool
    use_hyphens_in_client_order_ids: bool
    config: StrategyConfig
    oms_type: OmsType
    external_order_claims: list[InstrumentId]
    manage_contingent_orders: bool
    manage_gtd_expiry: bool
    clock: Clock
    cache: Cache
    portfolio: PortfolioFacade
    order_factory: OrderFactory

    def __init__(self, config: StrategyConfig | None = None): ...
    def to_importable_config(self) -> ImportableStrategyConfig:
        """
        Returns an importable configuration for this strategy.

        Returns
        -------
        ImportableStrategyConfig

        """
        ...
    def on_start(self) -> None: ...
    def on_stop(self) -> None: ...
    def on_resume(self) -> None: ...
    def on_reset(self) -> None: ...
    def register(
        self,
        trader_id: TraderId,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: Cache,
        clock: Clock,
    ) -> None:
        """
        Register the strategy with a trader.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID for the strategy.
        portfolio : PortfolioFacade
            The read-only portfolio for the strategy.
        msgbus : MessageBus
            The message bus for the strategy.
        cache : CacheFacade
            The read-only cache for the strategy.
        clock : Clock
            The clock for the strategy.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def change_id(self, strategy_id: StrategyId) -> None:
        """
        Change the strategies identifier to the given `strategy_id`.

        Parameters
        ----------
        strategy_id : StrategyId
            The new strategy ID to change to.

        """
        ...
    def change_order_id_tag(self, order_id_tag: str) -> None:
        """
        Change the order identifier tag to the given `order_id_tag`.

        Parameters
        ----------
        order_id_tag : str
            The new order ID tag to change to.

        """
        ...
    def on_order_event(self, event: OrderEvent) -> None:
        """
        Actions to be performed when running and receives an order event.

        Parameters
        ----------
        event : OrderEvent
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_initialized(self, event: OrderInitialized) -> None:
        """
        Actions to be performed when running and receives an order initialized event.

        Parameters
        ----------
        event : OrderInitialized
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_denied(self, event: OrderDenied) -> None:
        """
        Actions to be performed when running and receives an order denied event.

        Parameters
        ----------
        event : OrderDenied
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_emulated(self, event: OrderEmulated) -> None:
        """
        Actions to be performed when running and receives an order emulated event.

        Parameters
        ----------
        event : OrderEmulated
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_released(self, event: OrderReleased) -> None:
        """
        Actions to be performed when running and receives an order released event.

        Parameters
        ----------
        event : OrderReleased
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_submitted(self, event: OrderSubmitted) -> None:
        """
        Actions to be performed when running and receives an order submitted event.

        Parameters
        ----------
        event : OrderSubmitted
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_rejected(self, event: OrderRejected) -> None:
        """
        Actions to be performed when running and receives an order rejected event.

        Parameters
        ----------
        event : OrderRejected
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_accepted(self, event: OrderAccepted) -> None:
        """
        Actions to be performed when running and receives an order accepted event.

        Parameters
        ----------
        event : OrderAccepted
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_canceled(self, event: OrderCanceled) -> None:
        """
        Actions to be performed when running and receives an order canceled event.

        Parameters
        ----------
        event : OrderCanceled
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_expired(self, event: OrderExpired) -> None:
        """
        Actions to be performed when running and receives an order expired event.

        Parameters
        ----------
        event : OrderExpired
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_triggered(self, event: OrderTriggered) -> None:
        """
        Actions to be performed when running and receives an order triggered event.

        Parameters
        ----------
        event : OrderTriggered
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_pending_update(self, event: OrderPendingUpdate) -> None:
        """
        Actions to be performed when running and receives an order pending update event.

        Parameters
        ----------
        event : OrderPendingUpdate
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_pending_cancel(self, event: OrderPendingCancel) -> None:
        """
        Actions to be performed when running and receives an order pending cancel event.

        Parameters
        ----------
        event : OrderPendingCancel
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_modify_rejected(self, event: OrderModifyRejected) -> None:
        """
        Actions to be performed when running and receives an order modify rejected event.

        Parameters
        ----------
        event : OrderModifyRejected
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_cancel_rejected(self, event: OrderCancelRejected) -> None:
        """
        Actions to be performed when running and receives an order cancel rejected event.

        Parameters
        ----------
        event : OrderCancelRejected
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_updated(self, event: OrderUpdated) -> None:
        """
        Actions to be performed when running and receives an order updated event.

        Parameters
        ----------
        event : OrderUpdated
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_filled(self, event: OrderFilled) -> None:
        """
        Actions to be performed when running and receives an order filled event.

        Parameters
        ----------
        event : OrderFilled
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_position_event(self, event: PositionEvent) -> None:
        """
        Actions to be performed when running and receives a position event.

        Parameters
        ----------
        event : PositionEvent
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_position_opened(self, event: PositionOpened) -> None:
        """
        Actions to be performed when running and receives a position opened event.

        Parameters
        ----------
        event : PositionOpened
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_position_changed(self, event: PositionChanged) -> None:
        """
        Actions to be performed when running and receives a position changed event.

        Parameters
        ----------
        event : PositionChanged
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_position_closed(self, event: PositionClosed) -> None:
        """
        Actions to be performed when running and receives a position closed event.

        Parameters
        ----------
        event : PositionClosed
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def submit_order(
        self,
        order: Order,
        position_id: PositionId | None = None,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Submit the given order with optional position ID, execution algorithm
        and routing instructions.

        A `SubmitOrder` command will be created and sent to **either** an
        `ExecAlgorithm`, the `OrderEmulator` or the `RiskEngine` (depending whether
        the order is emulated and/or has an `exec_algorithm_id` specified).

        If the client order ID is duplicate, then the order will be denied.

        Parameters
        ----------
        order : Order
            The order to submit.
        position_id : PositionId, optional
            The position ID to submit the order against. If a position does not
            yet exist, then any position opened will have this identifier assigned.
        client_id : ClientId, optional
            The specific execution client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Raises
        ------
        ValueError
            If `order.status` is not ``INITIALIZED``.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by the order will have this position ID assigned. This may
        not be what you intended.

        """
        ...
    def submit_order_list(
        self,
        order_list: OrderList,
        position_id: PositionId | None = None,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Submit the given order list with optional position ID, execution algorithm
        and routing instructions.

        A `SubmitOrderList` command will be created and sent to **either** the
        `OrderEmulator`, or the `RiskEngine` (depending whether an order is emulated).

        If the order list ID is duplicate, or any client order ID is duplicate,
        then all orders will be denied.

        Parameters
        ----------
        order_list : OrderList
            The order list to submit.
        position_id : PositionId, optional
            The position ID to submit the order against. If a position does not
            yet exist, then any position opened will have this identifier assigned.
        client_id : ClientId, optional
            The specific execution client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Raises
        ------
        ValueError
            If any `order.status` is not ``INITIALIZED``.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by an order will have this position ID assigned. This may
        not be what you intended.

        """
        ...
    def modify_order(
        self,
        order: Order,
        quantity: Quantity | None = None,
        price: Price | None = None,
        trigger_price: Price | None = None,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Modify the given order with optional parameters and routing instructions.

        An `ModifyOrder` command will be created and then sent to **either** the
        `OrderEmulator` or the `RiskEngine` (depending on whether the order is emulated).

        At least one value must differ from the original order for the command to be valid.

        Will use an Order Cancel/Replace Request (a.k.a Order Modification)
        for FIX protocols, otherwise if order update is not available for
        the API, then will cancel and replace with a new order using the
        original `ClientOrderId`.

        Parameters
        ----------
        order : Order
            The order to update.
        quantity : Quantity, optional
            The updated quantity for the given order.
        price : Price, optional
            The updated price for the given order (if applicable).
        trigger_price : Price, optional
            The updated trigger price for the given order (if applicable).
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Raises
        ------
        ValueError
            If `price` is not ``None`` and order does not have a `price`.
        ValueError
            If `trigger` is not ``None`` and order does not have a `trigger_price`.

        Warnings
        --------
        If the order is already closed or at `PENDING_CANCEL` status
        then the command will not be generated, and a warning will be logged.

        References
        ----------
        https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_G_71.html

        """
        ...
    def cancel_order(self, order: Order, client_id: ClientId | None = None, params: dict[str, Any] | None = None) -> None:
        """
        Cancel the given order with optional routing instructions.

        A `CancelOrder` command will be created and then sent to **either** the
        `OrderEmulator` or the `ExecutionEngine` (depending on whether the order is emulated).

        Parameters
        ----------
        order : Order
            The order to cancel.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def cancel_orders(self, orders: list[Order], client_id: ClientId | None = None, params: dict[str, Any] | None = None) -> None:
        """
        Batch cancel the given list of orders with optional routing instructions.

        For each order in the list, a `CancelOrder` command will be created and added to a
        `BatchCancelOrders` command. This command is then sent to the `ExecutionEngine`.

        Logs an error if the `orders` list contains local/emulated orders.

        Parameters
        ----------
        orders : list[Order]
            The orders to cancel.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Raises
        ------
        ValueError
            If `orders` is empty.
        TypeError
            If `orders` contains a type other than `Order`.

        """
        ...
    def cancel_all_orders(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide = OrderSide.NO_ORDER_SIDE,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Cancel all orders for this strategy for the given instrument ID.

        A `CancelAllOrders` command will be created and then sent to **both** the
        `OrderEmulator` and the `ExecutionEngine`.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the orders to cancel.
        order_side : OrderSide, default ``NO_ORDER_SIDE`` (both sides)
            The side of the orders to cancel.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def close_position(
        self,
        position: Position,
        client_id: ClientId | None = None,
        tags: list[str] | None = None,
        time_in_force: TimeInForce = TimeInForce.GTC,
        reduce_only: bool = True,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Close the given position.

        A closing `MarketOrder` for the position will be created, and then sent
        to the `ExecutionEngine` via a `SubmitOrder` command.

        Parameters
        ----------
        position : Position
            The position to close.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        tags : list[str], optional
            The tags for the market order closing the position.
        time_in_force : TimeInForce, default ``GTC``
            The time in force for the market order closing the position.
        reduce_only : bool, default True
            If the market order to close the position should carry the 'reduce-only' execution instruction.
            Optional, as not all venues support this feature.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def close_all_positions(
        self,
        instrument_id: InstrumentId,
        position_side: PositionSide = PositionSide.NO_POSITION_SIDE,
        client_id: ClientId | None = None,
        tags: list[str] | None = None,
        time_in_force: TimeInForce = TimeInForce.GTC,
        reduce_only: bool = True,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Close all positions for the given instrument ID for this strategy.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the positions to close.
        position_side : PositionSide, default ``NO_POSITION_SIDE`` (both sides)
            The side of the positions to close.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        tags : list[str], optional
            The tags for the market orders closing the positions.
        time_in_force : TimeInForce, default ``GTC``
            The time in force for the market orders closing the positions.
        reduce_only : bool, default True
            If the market orders to close positions should carry the 'reduce-only' execution instruction.
            Optional, as not all venues support this feature.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def query_order(self, order: Order, client_id: ClientId | None = None, params: dict[str, Any] | None = None) -> None:
        """
        Query the given order with optional routing instructions.

        A `QueryOrder` command will be created and then sent to the
        `ExecutionEngine`.

        Logs an error if no `VenueOrderId` has been assigned to the order.

        Parameters
        ----------
        order : Order
            The order to query.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def cancel_gtd_expiry(self, order: Order) -> None:
        """
        Cancel the managed GTD expiry for the given order.

        If there is no current GTD expiry timer, then an error will be logged.

        Parameters
        ----------
        order : Order
            The order to cancel the GTD expiry for.

        """
        ...
    def handle_event(self, event: Event) -> None:
        """
        Handle the given event.

        If state is ``RUNNING`` then passes to `on_event`.

        Parameters
        ----------
        event : Event
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
