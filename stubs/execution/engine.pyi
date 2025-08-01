from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import ExecutionReport
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.execution.messages import TradingCommand, SubmitOrder, SubmitOrderList, ModifyOrder, CancelOrder, CancelAllOrders, BatchCancelOrders, QueryOrder
from nautilus_trader.model.events.order import OrderEvent
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.orders.base import OrderSide
from nautilus_trader.model.position.base import Position
from nautilus_trader.common.component import TimeEvent
from nautilus_trader.model.identifiers import PositionId
from stubs.cache.cache import Cache
from stubs.common.component import Clock, Component
from stubs.common.generators import PositionIdGenerator
from stubs.execution.client import ExecutionClient
from stubs.model.events.position import PositionEvent
from nautilus_trader.model.instruments.base import Instrument


class ExecutionEngine(Component):
    """
    Provides a high-performance execution engine for the management of many
    `ExecutionClient` instances, and the asynchronous ingest and distribution of
    trading commands and events.

    Parameters
    ----------
    msgbus : MessageBus
        The message bus for the engine.
    cache : Cache
        The cache for the engine.
    clock : Clock
        The clock for the engine.
    config : ExecEngineConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `ExecEngineConfig`.
    """

    _cache: Cache
    _clients: dict[ClientId, ExecutionClient]
    _routing_map: dict[Venue, ExecutionClient]
    _default_client: ExecutionClient | None
    _external_clients: set[ClientId]
    _oms_overrides: dict[StrategyId, OmsType]
    _external_order_claims: dict[InstrumentId, StrategyId]
    _pos_id_generator: PositionIdGenerator
    _pending_position_events: list[PositionEvent]
    debug: bool
    manage_own_order_books: bool
    snapshot_orders: bool
    snapshot_positions: bool
    snapshot_positions_interval_secs: int
    snapshot_positions_timer_name: str
    command_count: int
    event_count: int
    report_count: int

    def __init__(
        self,
        msgbus: MessageBus,
        cache: Cache,
        clock: Clock,
        config: ExecEngineConfig | None = None,
    ) -> None: ...
    @property
    def reconciliation(self) -> bool:
        """
        Return whether the reconciliation process will be run on start.

        Returns
        -------
        bool

        """
        ...
    @property
    def registered_clients(self) -> list[ClientId]:
        """
        Return the execution clients registered with the engine.

        Returns
        -------
        list[ClientId]

        """
        ...
    @property
    def default_client(self) -> ClientId | None:
        """
        Return the default execution client registered with the engine.

        Returns
        -------
        ClientId or ``None``

        """
        ...
    def connect(self) -> None:
        """
        Connect the engine by calling connect on all registered clients.
        """
        ...
    def disconnect(self) -> None:
        """
        Disconnect the engine by calling disconnect on all registered clients.
        """
        ...
    def position_id_count(self, strategy_id: StrategyId) -> int:
        """
        The position ID count for the given strategy ID.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID for the position count.

        Returns
        -------
        int

        """
        ...
    def check_integrity(self) -> bool:
        """
        Check integrity of data within the cache and clients.

        Returns
        -------
        bool
            True if checks pass, else False.
        """
        ...
    def check_connected(self) -> bool:
        """
        Check all of the engines clients are connected.

        Returns
        -------
        bool
            True if all clients connected, else False.

        """
        ...
    def check_disconnected(self) -> bool:
        """
        Check all of the engines clients are disconnected.

        Returns
        -------
        bool
            True if all clients disconnected, else False.

        """
        ...
    def check_residuals(self) -> bool:
        """
        Check for any residual open state and log warnings if found.

        'Open state' is considered to be open orders and open positions.

        Returns
        -------
        bool
            True if residuals exist, else False.

        """
        ...
    def get_external_order_claim(self, instrument_id: InstrumentId) -> StrategyId | None:
        """
        Get any external order claim for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the claim.

        Returns
        -------
        StrategyId or ``None``

        """
        ...
    def get_external_order_claims_instruments(self) -> set[InstrumentId]:
        """
        Get all instrument IDs registered for external order claims.

        Returns
        -------
        set[InstrumentId]

        """
        ...
    def get_clients_for_orders(self, orders: list[Order]) -> set[ExecutionClient]:
        """
        Get all execution clients corresponding to the given orders.

        Parameters
        ----------
        orders : list[Order]
            The orders to locate associated execution clients for.

        Returns
        -------
        set[ExecutionClient]

        """
        ...
    def set_manage_own_order_books(self, value: bool) -> None:
        """
        Set the `manage_own_order_books` setting with the given `value`.

        Parameters
        ----------
        value : bool
            The value to set.

        """
        ...
    def register_client(self, client: ExecutionClient) -> None:
        """
        Register the given execution client with the execution engine.

        If the `client.venue` is ``None`` and a default routing client has not
        been previously registered then will be registered as such.

        Parameters
        ----------
        client : ExecutionClient
            The execution client to register.

        Raises
        ------
        ValueError
            If `client` is already registered with the execution engine.

        """
        ...
    def register_default_client(self, client: ExecutionClient) -> None:
        """
        Register the given client as the default routing client (when a specific
        venue routing cannot be found).

        Any existing default routing client will be overwritten.

        Parameters
        ----------
        client : ExecutionClient
            The client to register.

        """
        ...
    def register_venue_routing(self, client: ExecutionClient, venue: Venue) -> None:
        """
        Register the given client to route orders to the given venue.

        Any existing client in the routing map for the given venue will be
        overwritten.

        Parameters
        ----------
        venue : Venue
            The venue to route orders to.
        client : ExecutionClient
            The client for the venue routing.

        """
        ...
    def register_oms_type(self, strategy: Strategy) -> None:
        """
        Register the given trading strategies OMS (Order Management System) type.

        Parameters
        ----------
        strategy : Strategy
            The strategy for the registration.

        """
        ...
    def register_external_order_claims(self, strategy: Strategy) -> None:
        """
        Register the given strategies external order claim instrument IDs (if any)

        Parameters
        ----------
        strategy : Strategy
            The strategy for the registration.

        Raises
        ------
        InvalidConfiguration
            If a strategy is already registered to claim external orders for an instrument ID.

        """
        ...
    def deregister_client(self, client: ExecutionClient) -> None:
        """
        Deregister the given execution client from the execution engine.

        Parameters
        ----------
        client : ExecutionClient
            The execution client to deregister.

        Raises
        ------
        ValueError
            If `client` is not registered with the execution engine.

        """
        ...
    async def reconcile_state(self, timeout_secs: float = 10.0) -> bool: # skip-validate
        """
        Reconcile the internal execution state with all execution clients (external state).

        Parameters
        ----------
        timeout_secs : double, default 10.0
            The timeout (seconds) for reconciliation to complete.

        Returns
        -------
        bool
            True if states reconcile within timeout, else False.

        Raises
        ------
        ValueError
            If `timeout_secs` is not positive (> 0).

        """
        ...
    def reconcile_report(self, report: ExecutionReport) -> bool:
        """
        Check the given execution report.

        Parameters
        ----------
        report : ExecutionReport
            The execution report to check.

        Returns
        -------
        bool
            True if reconciliation successful, else False.

        """
        ...
    def reconcile_mass_status(self, report: ExecutionMassStatus) -> None:
        """
        Reconcile the given execution mass status report.

        Parameters
        ----------
        report : ExecutionMassStatus
            The execution mass status report to reconcile.

        """
        ...
    def _on_start(self) -> None: ...
    def _on_stop(self) -> None: ...
    def _start(self) -> None: ...
    def _stop(self) -> None: ...
    def _reset(self) -> None: ...
    def _dispose(self) -> None: ...
    def stop_clients(self) -> None:
        """
        Stop the registered clients.
        """
        ...
    def load_cache(self) -> None:
        """
        Load the cache up from the execution database.
        """
        ...
    def execute(self, command: TradingCommand) -> None:
        """
        Execute the given command.

        Parameters
        ----------
        command : TradingCommand
            The command to execute.

        """
        ...
    def process(self, event: OrderEvent) -> None:
        """
        Process the given order event.

        Parameters
        ----------
        event : OrderEvent
            The order event to process.

        """
        ...
    def flush_db(self) -> None:
        """
        Flush the execution database which permanently removes all persisted data.

        Warnings
        --------
        Permanent data loss.

        """
        ...
    def _set_position_id_counts(self) -> None: ...
    def _last_px_for_conversion(self, instrument_id: InstrumentId, order_side: OrderSide) -> Price | None: ...
    def _set_order_base_qty(self, order: Order, base_qty: Quantity) -> None: ...
    def _deny_order(self, order: Order, reason: str) -> None: ...
    def _get_or_init_own_order_book(self, instrument_id: InstrumentId) -> object: ...
    def _add_own_book_order(self, order: Order) -> None: ...
    def _execute_command(self, command: TradingCommand) -> None: ...
    def _handle_submit_order(self, client: ExecutionClient, command: SubmitOrder) -> None: ...
    def _handle_submit_order_list(self, client: ExecutionClient, command: SubmitOrderList) -> None: ...
    def _handle_modify_order(self, client: ExecutionClient, command: ModifyOrder) -> None: ...
    def _handle_cancel_order(self, client: ExecutionClient, command: CancelOrder) -> None: ...
    def _handle_cancel_all_orders(self, client: ExecutionClient, command: CancelAllOrders) -> None: ...
    def _handle_batch_cancel_orders(self, client: ExecutionClient, command: BatchCancelOrders) -> None: ...
    def _handle_query_order(self, client: ExecutionClient, command: QueryOrder) -> None: ...
    def _handle_event(self, event: OrderEvent) -> None: ...
    def _determine_oms_type(self, fill: OrderFilled) -> OmsType: ...
    def _determine_position_id(self, fill: OrderFilled, oms_type: OmsType) -> None: ...
    def _determine_hedging_position_id(self, fill: OrderFilled) -> PositionId: ...
    def _determine_netting_position_id(self, fill: OrderFilled) -> PositionId: ...
    def _apply_event_to_order(self, order: Order, event: OrderEvent) -> None: ...
    def _handle_order_fill(self, order: Order, fill: OrderFilled, oms_type: OmsType) -> None: ...
    def _open_position(self, instrument: Instrument, position: Position | None, fill: OrderFilled, oms_type: OmsType) -> Position: ...
    def _update_position(self, instrument: Instrument, position: Position, fill: OrderFilled, oms_type: OmsType) -> None: ...
    def _will_flip_position(self, position: Position, fill: OrderFilled) -> bool: ...
    def _flip_position(self, instrument: Instrument, position: Position, fill: OrderFilled, oms_type: OmsType) -> None: ...
    def _create_order_state_snapshot(self, order: Order) -> None: ...
    def _create_position_state_snapshot(self, position: Position, open_only: bool) -> None: ...
    def _snapshot_open_position_states(self, event: TimeEvent) -> None: ...

