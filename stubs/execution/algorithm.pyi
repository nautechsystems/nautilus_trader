import datetime as dt

from nautilus_trader.execution.config import ExecAlgorithmConfig
from nautilus_trader.execution.config import ImportableExecAlgorithmConfig

class ExecAlgorithm(Actor):
    """
    The base class for all execution algorithms.

    This class allows traders to implement their own customized execution algorithms.

    Parameters
    ----------
    config : ExecAlgorithmConfig, optional
        The execution algorithm configuration.

    Raises
    ------
    TypeError
        If `config` is not of type `ExecAlgorithmConfig`.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    id: ExecAlgorithmId
    portfolio: PortfolioFacade
    config: ExecAlgorithmConfig

    _log_events: bool
    _log_commands: bool
    _exec_spawn_ids: dict[ClientOrderId, int]
    _subscribed_strategies: set[StrategyId]

    def __init__(self, config: ExecAlgorithmConfig | None = None) -> None: ...
    def to_importable_config(self) -> ImportableExecAlgorithmConfig:
        """
        Returns an importable configuration for this execution algorithm.

        Returns
        -------
        ImportableExecAlgorithmConfig

        """
        ...
    def register(
        self,
        trader_id: TraderId,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: CacheFacade,
        clock: Clock,
    ) -> None:
        """
        Register the execution algorithm with a trader.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID for the execution algorithm.
        portfolio : PortfolioFacade
            The read-only portfolio for the execution algorithm.
        msgbus : MessageBus
            The message bus for the execution algorithm.
        cache : CacheFacade
            The read-only cache for the execution algorithm.
        clock : Clock
            The clock for the execution algorithm.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def _reset(self) -> None: ...
    def execute(self, command: TradingCommand) -> None:
        """
        Handle the given trading command by processing it with the execution algorithm.

        Parameters
        ----------
        command : SubmitOrder
            The command to handle.

        Raises
        ------
        ValueError
            If `command.exec_algorithm_id` is not equal to `self.id`.

        """
        ...
    def on_order(self, order: Order) -> None:
        """
        Actions to be performed when running and receives an order.

        Parameters
        ----------
        order : Order
            The order to be handled.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_list(self, order_list: OrderList) -> None:
        """
        Actions to be performed when running and receives an order list.

        Parameters
        ----------
        order_list : OrderList
            The order list to be handled.

        Warnings
        --------
        System method (not intended to be called by user code).

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
        Actions to be performed when running and receives an order initialized event.

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
    def spawn_market(
        self,
        primary: Order,
        quantity: Quantity,
        time_in_force: TimeInForce = ...,
        reduce_only: bool = False,
        tags: list[str] | None = None,
        reduce_primary: bool = True,
    ) -> MarketOrder:
        """
        Spawn a new ``MARKET`` order from the given primary order.

        Parameters
        ----------
        primary : Order
            The primary order from which this order will spawn.
        quantity : Quantity
            The spawned orders quantity (> 0).
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
            The spawned orders time in force. Often not applicable for market orders.
        reduce_only : bool, default False
            If the spawned order carries the 'reduce-only' execution instruction.
        tags : list[str], optional
            The custom user tags for the order.
        reduce_primary : bool, default True
            If the primary order quantity should be reduced by the given `quantity`.

        Returns
        -------
        MarketOrder

        Raises
        ------
        ValueError
            If `primary.exec_algorithm_id` is not equal to `self.id`.
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD``.

        """
        ...
    def spawn_limit(
        self,
        primary: Order,
        quantity: Quantity,
        price: Price,
        time_in_force: TimeInForce = ...,
        expire_time: dt.datetime | None = None,
        post_only: bool = False,
        reduce_only: bool = False,
        display_qty: Quantity | None = None,
        emulation_trigger: TriggerType = ...,
        tags: list[str] | None = None,
        reduce_primary: bool = True,
    ) -> LimitOrder:
        """
        Spawn a new ``LIMIT`` order from the given primary order.

        Parameters
        ----------
        primary : Order
            The primary order from which this order will spawn.
        quantity : Quantity
            The spawned orders quantity (> 0). Must be less than `primary.quantity`.
        price : Price
            The spawned orders price.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
            The spawned orders time in force.
        expire_time : datetime, optional
            The spawned order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the spawned order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the spawned order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the spawned order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        tags : list[str], optional
            The custom user tags for the order.
        reduce_primary : bool, default True
            If the primary order quantity should be reduced by the given `quantity`.

        Returns
        -------
        LimitOrder

        Raises
        ------
        ValueError
            If `primary.exec_algorithm_id` is not equal to `self.id`.
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        ...
    def spawn_market_to_limit(
        self,
        primary: Order,
        quantity: Quantity,
        time_in_force: TimeInForce = ...,
        expire_time: dt.datetime | None = None,
        reduce_only: bool = False,
        display_qty: Quantity | None = None,
        emulation_trigger: TriggerType = ...,
        tags: list[str] | None = None,
        reduce_primary: bool = True,
    ) -> MarketToLimitOrder:
        """
        Spawn a new ``MARKET_TO_LIMIT`` order from the given primary order.

        Parameters
        ----------
        primary : Order
            The primary order from which this order will spawn.
        quantity : Quantity
            The spawned orders quantity (> 0). Must be less than `primary.quantity`.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
            The spawned orders time in force.
        expire_time : datetime, optional
            The spawned order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the spawned order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the spawned order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        tags : list[str], optional
            The custom user tags for the order.
        reduce_primary : bool, default True
            If the primary order quantity should be reduced by the given `quantity`.

        Returns
        -------
        MarketToLimitOrder

        Raises
        ------
        ValueError
            If `primary.exec_algorithm_id` is not equal to `self.id`.
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        ...
    def submit_order(self, order: Order) -> None:
        """
        Submit the given order (may be the primary or spawned order).

        A `SubmitOrder` command will be created and sent to the `RiskEngine`.

        If the client order ID is duplicate, then the order will be denied.

        Parameters
        ----------
        order : Order
            The order to submit.
        parent_order_id : ClientOrderId, optional
            The parent client order identifier. If provided then will be considered a child order
            of the parent.

        Raises
        ------
        ValueError
            If `order.status` is not ``INITIALIZED`` or ``RELEASED``.
        ValueError
            If `order.emulation_trigger` is not ``NO_TRIGGER``.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by the order will have this position ID assigned. This may
        not be what you intended.

        Emulated orders cannot be sent from execution algorithms (intentionally constraining complexity).

        """
        ...
    def modify_order(
        self,
        order: Order,
        quantity: Quantity | None = None,
        price: Price | None = None,
        trigger_price: Price | None = None,
        client_id: ClientId | None = None,
    ) -> None:
        """
        Modify the given order with optional parameters and routing instructions.

        An `ModifyOrder` command will be created and then sent to the `RiskEngine`.

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
    def modify_order_in_place(
        self,
        order: Order,
        quantity: Quantity | None = None,
        price: Price | None = None,
        trigger_price: Price | None = None,
    ) -> None:
        """
        Modify the given ``INITIALIZED`` order in place (immediately) with optional parameters.

        At least one value must differ from the original order for the command to be valid.

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

        Raises
        ------
        ValueError
            If `order.status` is not ``INITIALIZED`` or ``RELEASED``.
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
    def cancel_order(self, order: Order, client_id: ClientId | None = None) -> None:
        """
        Cancel the given order with optional routing instructions.

        A `CancelOrder` command will be created and then sent to **either** the
        `OrderEmulator` or the `ExecutionEngine` (depending on whether the order is emulated).

        Logs an error if no `VenueOrderId` has been assigned to the order.

        Parameters
        ----------
        order : Order
            The order to cancel.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        ...

