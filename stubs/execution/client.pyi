from nautilus_trader.common.config import NautilusConfig
from typing import Any

class ExecutionClient(Component):
    """
    The base class for all execution clients.

    Parameters
    ----------
    client_id : ClientId
        The client ID.
    venue : Venue or ``None``
        The client venue. If multi-venue then can be ``None``.
    oms_type : OmsType
        The venues order management system type.
    account_type : AccountType
        The account type for the client.
    base_currency : Currency or ``None``
        The account base currency. Use ``None`` for multi-currency accounts.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    config : NautilusConfig, optional
        The configuration for the instance.

    Raises
    ------
    ValueError
        If `client_id` is not equal to `account_id.get_issuer()`.
    ValueError
        If `oms_type` is ``UNSPECIFIED`` (must be specified).

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    trader_id: Any
    venue: Any
    oms_type: Any
    account_id: Any
    account_type: Any
    base_currency: Any
    is_connected: bool

    def __init__(
        self,
        client_id: ClientId,
        venue: Venue | None,
        oms_type: OmsType,
        account_type: AccountType,
        base_currency: Currency | None,
        msgbus: MessageBus,
        cache: Cache,
        clock: Clock,
        config: NautilusConfig | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def _set_connected(self, value: bool = True) -> None: ...
    def _set_account_id(self, account_id: AccountId) -> None: ...
    def get_account(self) -> Account | None:
        """
        Return the account for the client (if registered).

        Returns
        -------
        Account or ``None``

        """
        ...
    def submit_order(self, command: SubmitOrder) -> None:
        """
        Submit the order contained in the given command for execution.

        Parameters
        ----------
        command : SubmitOrder
            The command to execute.

        """
        ...
    def submit_order_list(self, command: SubmitOrderList) -> None:
        """
        Submit the order list contained in the given command for execution.

        Parameters
        ----------
        command : SubmitOrderList
            The command to execute.

        """
        ...
    def modify_order(self, command: ModifyOrder) -> None:
        """
        Modify the order with parameters contained in the command.

        Parameters
        ----------
        command : ModifyOrder
            The command to execute.

        """
        ...
    def cancel_order(self, command: CancelOrder) -> None:
        """
        Cancel the order with the client order ID contained in the given command.

        Parameters
        ----------
        command : CancelOrder
            The command to execute.

        """
        ...
    def cancel_all_orders(self, command: CancelAllOrders) -> None:
        """
        Cancel all orders for the instrument ID contained in the given command.

        Parameters
        ----------
        command : CancelAllOrders
            The command to execute.

        """
        ...
    def batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        """
        Batch cancel orders for the instrument ID contained in the given command.

        Parameters
        ----------
        command : BatchCancelOrders
            The command to execute.

        """
        ...
    def query_order(self, command: QueryOrder) -> None:
        """
        Initiate a reconciliation for the queried order which will generate an
        `OrderStatusReport`.

        Parameters
        ----------
        command : QueryOrder
            The command to execute.

        """
        ...
    def generate_account_state(
        self,
        balances: list[AccountBalance],
        margins: list[MarginBalance],
        reported: bool,
        ts_event: int,
        info: dict[str, Any] | None = None,
    ) -> None:
        """
        Generate an `AccountState` event and publish on the message bus.

        Parameters
        ----------
        balances : list[AccountBalance]
            The account balances.
        margins : list[MarginBalance]
            The margin balances.
        reported : bool
            If the balances are reported directly from the exchange.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the account state event occurred.
        info : dict [str, object]
            The additional implementation specific account information.

        """
        ...
    def generate_order_submitted(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        ts_event: int,
    ) -> None:
        """
        Generate an `OrderSubmitted` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order submitted event occurred.

        """
        ...
    def generate_order_rejected(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: str,
        ts_event: int,
    ) -> None:
        """
        Generate an `OrderRejected` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        reason : datetime
            The order rejected reason.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order rejected event occurred.

        """
        ...
    def generate_order_accepted(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> None:
        """
        Generate an `OrderAccepted` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order accepted event occurred.

        """
        ...
    def generate_order_modify_rejected(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: str,
        ts_event: int,
    ) -> None:
        """
        Generate an `OrderModifyRejected` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        reason : str
            The order update rejected reason.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order update rejection event occurred.

        """
        ...
    def generate_order_cancel_rejected(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: str,
        ts_event: int,
    ) -> None:
        """
        Generate an `OrderCancelRejected` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        reason : str
            The order cancel rejected reason.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order cancel rejected event occurred.

        """
        ...
    def generate_order_updated(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Price,
        trigger_price: Price | None,
        ts_event: int,
        venue_order_id_modified: bool = False,
    ) -> None:
        """
        Generate an `OrderUpdated` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        quantity : Quantity
            The orders current quantity.
        price : Price
            The orders current price.
        trigger_price : Price or ``None``
            The orders current trigger price.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order update event occurred.
        venue_order_id_modified : bool
            If the ID was modified for this event.

        """
        ...
    def generate_order_canceled(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> None:
        """
        Generate an `OrderCanceled` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when order canceled event occurred.

        """
        ...
    def generate_order_triggered(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> None:
        """
        Generate an `OrderTriggered` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order triggered event occurred.

        """
        ...
    def generate_order_expired(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> None:
        """
        Generate an `OrderExpired` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order expired event occurred.

        """
        ...
    def generate_order_filled(
        self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        venue_position_id: PositionId | None,
        trade_id: TradeId,
        order_side: OrderSide,
        order_type: OrderType,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Money,
        liquidity_side: LiquiditySide,
        ts_event: int,
        info: dict[str, Any] | None = None,
    ) -> None:
        """
        Generate an `OrderFilled` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        trade_id : TradeId
            The trade ID.
        venue_position_id : PositionId or ``None``
            The venue position ID associated with the order. If the trading
            venue has assigned a position ID / ticket then pass that here,
            otherwise pass ``None`` and the execution engine OMS will handle
            position ID resolution.
        order_side : OrderSide {``BUY``, ``SELL``}
            The execution order side.
        order_type : OrderType
            The execution order type.
        last_qty : Quantity
            The fill quantity for this execution.
        last_px : Price
            The fill price for this execution (not average price).
        quote_currency : Currency
            The currency of the price.
        commission : Money
            The fill commission.
        liquidity_side : LiquiditySide {``NO_LIQUIDITY_SIDE``, ``MAKER``, ``TAKER``}
            The execution liquidity side.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order filled event occurred.
        info : dict[str, object], optional
            The additional fill information.

        """
        ...
    def _send_account_state(self, account_state: AccountState) -> None: ...
    def _send_order_event(self, event: OrderEvent) -> None: ...
    def _send_mass_status_report(self, report: ExecutionMassStatus) -> None: ...
    def _send_order_status_report(self, report: OrderStatusReport) -> None: ...
    def _send_fill_report(self, report: FillReport) -> None: ...

