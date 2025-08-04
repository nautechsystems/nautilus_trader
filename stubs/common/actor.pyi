import asyncio
import datetime as dt
from collections.abc import Callable
from concurrent.futures import Executor
from typing import Any

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.common.component import Clock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.common.executor import ActorExecutor
from nautilus_trader.common.executor import TaskId
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.model.greeks import GreeksCalculator
from nautilus_trader.portfolio.base import PortfolioFacade
from stubs.common.component import Component
from stubs.core.uuid import UUID4
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import BarType
from stubs.model.identifiers import InstrumentId

class Actor(Component):
    """
    The base class for all actor components.

    Parameters
    ----------
    config : ActorConfig, optional
        The actor configuration.

    Raises
    ------
    TypeError
        If `config` is not of type `ActorConfig`.

    Warnings
    --------
    - This class should not be used directly, but through a concrete subclass.
    - Do not call components such as `clock` and `logger` in the `__init__` prior to registration.
    """

    portfolio: PortfolioFacade | None
    msgbus: MessageBus | None
    cache: CacheFacade | None
    clock: Clock | None
    greeks: GreeksCalculator | None
    log: Any # Logger type from Component
    config: ActorConfig
    trader_id: UUID4 | None
    _log_events: bool
    _log_commands: bool
    _warning_events: set[type]
    _pending_requests: dict[UUID4, Callable[[UUID4], None] | None]
    _pyo3_conversion_types: set[type]
    _signal_classes: dict[str, type]
    _indicators: list[Indicator]
    _indicators_for_quotes: dict[InstrumentId, list[Indicator]]
    _indicators_for_trades: dict[InstrumentId, list[Indicator]]
    _indicators_for_bars: dict[BarType, list[Indicator]]
    _executor: ActorExecutor | None

    def __init__(self, config: ActorConfig | None = None) -> None: ...
    def to_importable_config(self) -> ImportableActorConfig:
        """
        Returns an importable configuration for this actor.

        Returns
        -------
        ImportableActorConfig

        """
        ...
    def on_save(self) -> dict[str, bytes]:
        """
        Actions to be performed when the actor state is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state to save.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_load(self, state: dict[str, bytes]) -> None:
        """
        Actions to be performed when the actor state is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict[str, bytes]
            The strategy state to load.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_start(self) -> None:
        """
        Actions to be performed on start.

        The intent is that this method is called once per trading 'run', when
        initially starting.

        It is recommended to subscribe/request for data here.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in a user implementation.

        """
        ...
    def on_stop(self) -> None:
        """
        Actions to be performed on stop.

        The intent is that this method is called to pause, or when done for day.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in a user implementation.

        """
        ...
    def on_resume(self) -> None:
        """
        Actions to be performed on resume.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_reset(self) -> None:
        """
        Actions to be performed on reset.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in a user implementation.

        """
        ...
    def on_dispose(self) -> None:
        """
        Actions to be performed on dispose.

        Cleanup/release any resources used here.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_degrade(self) -> None:
        """
        Actions to be performed on degrade.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the actor implementation.

        """
        ...
    def on_fault(self) -> None:
        """
        Actions to be performed on fault.

        Cleanup any resources used by the actor here.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the actor implementation.

        """
        ...
    def on_instrument_status(self, data: InstrumentStatus) -> None:
        """
        Actions to be performed when running and receives an instrument status
        update.

        Parameters
        ----------
        data : InstrumentStatus
            The instrument status update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_instrument_close(self, update: InstrumentClose) -> None:
        """
        Actions to be performed when running and receives an instrument close
        update.

        Parameters
        ----------
        update : InstrumentClose
            The instrument close received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_instrument(self, instrument: Instrument) -> None:
        """
        Actions to be performed when running and receives an instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_book(self, order_book: OrderBook) -> None:
        """
        Actions to be performed when running and receives an order book.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_book_deltas(self, deltas: OrderBookDeltas | Any) -> None: # pyo3 type hint
        """
        Actions to be performed when running and receives order book deltas.

        Parameters
        ----------
        deltas : OrderBookDeltas or nautilus_pyo3.OrderBookDeltas
            The order book deltas received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_order_book_depth(self, depth: OrderBookDepth10) -> None:
        """
        Actions to be performed when running and receives an order book depth.

        Parameters
        ----------
        depth : OrderBookDepth10
            The order book depth received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_trade_tick(self, tick: TradeTick) -> None:
        """
        Actions to be performed when running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_mark_price(self, mark_price: MarkPriceUpdate) -> None:
        """
        Actions to be performed when running and receives a mark price update.

        Parameters
        ----------
        mark_price : MarkPriceUpdate
            The mark price update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_index_price(self, index_price: IndexPriceUpdate) -> None:
        """
        Actions to be performed when running and receives an index price update.

        Parameters
        ----------
        index_price : IndexPriceUpdate
            The index price update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_bar(self, bar: Bar) -> None:
        """
        Actions to be performed when running and receives a bar.

        Parameters
        ----------
        bar : Bar
            The bar received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_data(self, data: Data) -> None:
        """
        Actions to be performed when running and receives data.

        Parameters
        ----------
        data : Data
            The data received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_signal(self, signal: Data) -> None:
        """
        Actions to be performed when running and receives signal data.

        Parameters
        ----------
        signal : Data
            The signal received.

        Warnings
        --------
        System method (not intended to be called by user code).

        Notes
        -----
        This refers to a data signal, not an operating system signal (such as SIGTERM, SIGKILL, etc.).

        """
        ...
    def on_historical_data(self, data: Data) -> None:
        """
        Actions to be performed when running and receives historical data.

        Parameters
        ----------
        data : Data
            The historical data received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def on_event(self, event: Event) -> None:
        """
        Actions to be performed running and receives an event.

        Parameters
        ----------
        event : Event
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    @property
    def registered_indicators(self) -> list[Indicator]:
        """
        Return the registered indicators for the strategy.

        Returns
        -------
        list[Indicator]

        """
        ...
    def indicators_initialized(self) -> bool:
        """
        Return a value indicating whether all indicators are initialized.

        Returns
        -------
        bool
            True if all initialized, else False

        """
        ...
    def register_base(
        self,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: CacheFacade,
        clock: Clock,
    ) -> None:
        """
        Register with a trader.

        Parameters
        ----------
        portfolio : PortfolioFacade
            The read-only portfolio for the actor.
        msgbus : MessageBus
            The message bus for the actor.
        cache : CacheFacade
            The read-only cache for the actor.
        clock : Clock
            The clock for the actor.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def register_executor(
        self,
        loop: asyncio.AbstractEventLoop,
        executor: Executor,
    ) -> None:
        """
        Register the given `Executor` for the actor.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop of the application.
        executor : concurrent.futures.Executor
            The executor to register.

        Raises
        ------
        TypeError
            If `executor` is not of type `concurrent.futures.Executor`

        """
        ...
    def register_warning_event(self, event: type) -> None:
        """
        Register the given event type for warning log levels.

        Parameters
        ----------
        event : type
            The event class to register.

        """
        ...
    def deregister_warning_event(self, event: type) -> None:
        """
        Deregister the given event type from warning log levels.

        Parameters
        ----------
        event : type
            The event class to deregister.

        """
        ...
    def register_indicator_for_quote_ticks(self, instrument_id: InstrumentId, indicator: Indicator) -> None:
        """
        Register the given indicator with the actor/strategy to receive quote tick
        data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for tick updates.
        indicator : Indicator
            The indicator to register.

        """
        ...
    def register_indicator_for_trade_ticks(self, instrument_id: InstrumentId, indicator: Indicator) -> None:
        """
        Register the given indicator with the actor/strategy to receive trade tick
        data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for tick updates.
        indicator : indicator
            The indicator to register.

        """
        ...
    def register_indicator_for_bars(self, bar_type: BarType, indicator: Indicator) -> None:
        """
        Register the given indicator with the actor/strategy to receive bar data for the
        given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for bar updates.
        indicator : Indicator
            The indicator to register.

        """
        ...
    def save(self) -> dict[str, bytes]:
        """
        Return the actor/strategy state dictionary to be saved.

        Calls `on_save`.

        Returns
        -------
        dict[str, bytes]
            The strategy state to save.

        Warnings
        --------
        Exceptions raised will be caught, logged, and reraised.

        """
        ...
    def load(self, state: dict[str, bytes]) -> None:
        """
        Load the actor/strategy state from the give state dictionary.

        Calls `on_load` and passes the state.

        Parameters
        ----------
        state : dict[str, bytes]
            The strategy state to load.

        Warnings
        --------
        Exceptions raised will be caught, logged, and reraised.

        """
        ...
    def add_synthetic(self, synthetic: SyntheticInstrument) -> None:
        """
        Add the created synthetic instrument to the cache.

        Parameters
        ----------
        synthetic : SyntheticInstrument
            The synthetic instrument to add to the cache.

        Raises
        ------
        KeyError
            If `synthetic` is already in the cache.

        Notes
        -----
        If you are updating the synthetic instrument then you should use the `update_synthetic` method.

        """
        ...
    def update_synthetic(self, synthetic: SyntheticInstrument) -> None:
        """
        Update the synthetic instrument in the cache.

        Parameters
        ----------
        synthetic : SyntheticInstrument
            The synthetic instrument to update in the cache.

        Raises
        ------
        KeyError
            If `synthetic` does not already exist in the cache.

        Notes
        -----
        If you are adding a new synthetic instrument then you should use the `add_synthetic` method.

        """
        ...
    def queue_for_executor(
        self,
        func: Callable[..., Any],
        args: tuple | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> TaskId:
        """
        Queues the callable `func` to be executed as `fn(*args, **kwargs)` sequentially.

        Parameters
        ----------
        func : Callable
            The function to be executed.
        args : positional arguments
            The positional arguments for the call to `func`.
        kwargs : arbitrary keyword arguments
            The keyword arguments for the call to `func`.

        Raises
        ------
        TypeError
            If `func` is not of type `Callable`.

        Notes
        -----
        For backtesting the `func` is immediately executed, as there's no need for a `Future`
        object that can be awaited. In a backtesting scenario, the execution is not in real time,
        and so the results of `func` are 'immediately' available after it's called.

        """
        ...
    def run_in_executor(
        self,
        func: Callable[..., Any],
        args: tuple | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> TaskId:
        """
        Schedules the callable `func` to be executed as `fn(*args, **kwargs)`.

        Parameters
        ----------
        func : Callable
            The function to be executed.
        args : positional arguments
            The positional arguments for the call to `func`.
        kwargs : arbitrary keyword arguments
            The keyword arguments for the call to `func`.

        Returns
        -------
        TaskId
            The unique task identifier for the execution.
            This also corresponds to any future objects memory address.

        Raises
        ------
        TypeError
            If `func` is not of type `Callable`.

        Notes
        -----
        For backtesting the `func` is immediately executed, as there's no need for a `Future`
        object that can be awaited. In a backtesting scenario, the execution is not in real time,
        and so the results of `func` are 'immediately' available after it's called.

        """
        ...
    def queued_task_ids(self) -> list[TaskId]:
        """
        Return the queued task identifiers.

        Returns
        -------
        list[TaskId]

        """
        ...
    def active_task_ids(self) -> list[TaskId]:
        """
        Return the active task identifiers.

        Returns
        -------
        list[TaskId]

        """
        ...
    def has_queued_tasks(self) -> bool:
        """
        Return a value indicating whether there are any queued tasks.

        Returns
        -------
        bool

        """
        ...
    def has_active_tasks(self) -> bool:
        """
        Return a value indicating whether there are any active tasks.

        Returns
        -------
        bool

        """
        ...
    def has_any_tasks(self) -> bool:
        """
        Return a value indicating whether there are any queued OR active tasks.

        Returns
        -------
        bool

        """
        ...
    def cancel_task(self, task_id: TaskId) -> None:
        """
        Cancel the task with the given `task_id` (if queued or active).

        If the task is not found then a warning is logged.

        Parameters
        ----------
        task_id : TaskId
            The task identifier.

        """
        ...
    def cancel_all_tasks(self) -> None:
        """
        Cancel all queued and active tasks.
        """
        ...
    def _start(self) -> None: ...
    def _stop(self) -> None: ...
    def _resume(self) -> None: ...
    def _reset(self) -> None: ...
    def _dispose(self) -> None: ...
    def _degrade(self) -> None: ...
    def _fault(self) -> None: ...
    def subscribe_data(
        self,
        data_type: DataType,
        client_id: ClientId | None = None,
        instrument_id: InstrumentId | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to data of the given data type.

        Once subscribed, any matching data published on the message bus is forwarded
        to the `on_data` handler.

        Parameters
        ----------
        data_type : DataType
            The data type to subscribe to.
        client_id : ClientId, optional
            The data client ID. If supplied then a `Subscribe` command will be
            sent to the corresponding data client.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
            Only useful when downloading data during a backtest.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_instruments(
        self,
        venue: Venue,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to update `Instrument` data for the given venue.

        Once subscribed, any matching instrument data published on the message bus is forwarded
        the `on_instrument` handler.

        Parameters
        ----------
        venue : Venue
            The venue for the subscription.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_instrument(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to update `Instrument` data for the given instrument ID.

        Once subscribed, any matching instrument data published on the message bus is forwarded
        to the `on_instrument` handler.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType = BookType.L2_MBP,
        depth: int = 0,
        client_id: ClientId | None = None,
        managed: bool = True,
        pyo3_conversion: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to the order book data stream, being a snapshot then deltas
        for the given instrument ID.

        Once subscribed, any matching order book data published on the message bus is forwarded
        to the `on_order_book_deltas` handler.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument ID to subscribe to.
        book_type : BookType {``L1_MBP``, ``L2_MBP``, ``L3_MBO``}
            The order book type.
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        managed : bool, default True
            If an order book should be managed by the data engine based on the subscribed feed.
        pyo3_conversion : bool, default False
            If received deltas should be converted to `nautilus_pyo3.OrderBookDeltas`
            prior to being passed to the `on_order_book_deltas` handler.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_order_book_depth(
        self,
        instrument_id: InstrumentId,
        book_type: BookType = BookType.L2_MBP,
        depth: int = 0,
        client_id: ClientId | None = None,
        managed: bool = True,
        pyo3_conversion: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to the order book depth stream for the given instrument ID.

        Once subscribed, any matching order book data published on the message bus is forwarded
        to the `on_order_book_depth` handler.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument ID to subscribe to.
        book_type : BookType {``L1_MBP``, ``L2_MBP``, ``L3_MBO``}
            The order book type.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        managed : bool, default True
            If an order book should be managed by the data engine based on the subscribed feed.
        pyo3_conversion : bool, default False
            If received deltas should be converted to `nautilus_pyo3.OrderBookDepth`
            prior to being passed to the `on_order_book_depth` handler.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_order_book_at_interval(
        self,
        instrument_id: InstrumentId,
        book_type: BookType = BookType.L2_MBP,
        depth: int = 0,
        interval_ms: int = 1000,
        client_id: ClientId | None = None,
        managed: bool = True,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to an `OrderBook` at a specified interval for the given instrument ID.

        Once subscribed, any matching order book updates published on the message bus are forwarded
        to the `on_order_book` handler.

        The `DataEngine` will only maintain one order book for each instrument.
        Because of this - the level, depth and params for the stream will be set
        as per the last subscription request (this will also affect all subscribers).

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument ID to subscribe to.
        book_type : BookType {``L1_MBP``, ``L2_MBP``, ``L3_MBO``}
            The order book type.
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        interval_ms : int, default 1000
            The order book snapshot interval (milliseconds).
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        managed : bool, default True
            If an order book should be managed by the data engine based on the subscribed feed.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Raises
        ------
        ValueError
            If `depth` is negative (< 0).
        ValueError
            If `interval_ms` is not positive (> 0).

        Warnings
        --------
        Consider subscribing to order book deltas if you need intervals less than 100 milliseconds.

        """
        ...
    def subscribe_quote_ticks(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to streaming `QuoteTick` data for the given instrument ID.

        Once subscribed, any matching quote tick data published on the message bus is forwarded
        to the `on_quote_tick` handler.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
            Only useful when downloading data during a backtest.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_trade_ticks(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to streaming `TradeTick` data for the given instrument ID.

        Once subscribed, any matching trade tick data published on the message bus is forwarded
        to the `on_trade_tick` handler.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
            Only useful when downloading data during a backtest.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_mark_prices(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to streaming `MarkPriceUpdate` data for the given instrument ID.

        Once subscribed, any matching mark price updates published on the message bus are forwarded
        to the `on_mark_price` handler.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_index_prices(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to streaming `IndexPriceUpdate` data for the given instrument ID.

        Once subscribed, any matching index price updates published on the message bus are forwarded
        to the `on_index_price` handler.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_bars(
        self,
        bar_type: BarType,
        client_id: ClientId | None = None,
        await_partial: bool = False,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to streaming `Bar` data for the given bar type.

        Once subscribed, any matching bar data published on the message bus is forwarded
        to the `on_bar` handler.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        await_partial : bool, default False
            If the bar aggregator should await the arrival of a historical partial bar prior
            to actively aggregating new bars.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
            Only useful when downloading data during a backtest.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_instrument_status(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to status updates for the given instrument ID.

        Once subscribed, any matching instrument status data published on the message bus is forwarded
        to the `on_instrument_status` handler.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to status updates for.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def subscribe_instrument_close(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to close updates for the given instrument ID.

        Once subscribed, any matching instrument close data published on the message bus is forwarded
        to the `on_instrument_close` handler.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to status updates for.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_data(
        self,
        data_type: DataType,
        client_id: ClientId | None = None,
        instrument_id: InstrumentId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from data of the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type to unsubscribe from.
        client_id : ClientId, optional
            The data client ID. If supplied then an `Unsubscribe` command will
            be sent to the data client.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_instruments(
        self,
        venue: Venue,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from update `Instrument` data for the given venue.

        Parameters
        ----------
        venue : Venue
            The venue for the subscription.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_instrument(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from update `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe the order book deltas stream for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_order_book_depth(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe the order book depth stream for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any] | None = None,
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_order_book_at_interval(
        self,
        instrument_id: InstrumentId,
        interval_ms: int = 1000,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from an `OrderBook` at a specified interval for the given instrument ID.

        The interval must match the previously subscribed interval.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        interval_ms : int, default 1000
            The order book snapshot interval (milliseconds).
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_quote_ticks(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from streaming `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any] | None = None,
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_trade_ticks(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from streaming `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any] | None = None,
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_mark_prices(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from streaming `MarkPriceUpdate` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any] | None = None,
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_index_prices(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from streaming `IndexPriceUpdate` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any] | None = None,
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_bars(
        self,
        bar_type: BarType,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from streaming `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any] | None = None,
            Additional parameters potentially used by a specific client.

        """
        ...
    def unsubscribe_instrument_status(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe to status updates of the given venue.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe to status updates for.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue.
        params : dict[str, Any] | None = None,
            Additional parameters potentially used by a specific client.

        """
        ...
    def publish_data(self, data_type: DataType, data: Data) -> None:
        """
        Publish the given data to the message bus.

        Parameters
        ----------
        data_type : DataType
            The data type being published.
        data : Data
            The data to publish.

        """
        ...
    def publish_signal(self, name: str, value: int | float | str, ts_event: int = 0) -> None:
        """
        Publish the given value as a signal to the message bus.

        Parameters
        ----------
        name : str
            The name of the signal being published.
            The signal name will be converted to title case, with each word capitalized
            (e.g., 'example' becomes 'SignalExample').
        value : object
            The signal data to publish.
        ts_event : uint64_t, optional
            UNIX timestamp (nanoseconds) when the signal event occurred.
            If ``None`` then will timestamp current time.

        """
        ...
    def subscribe_signal(self, name: str = "") -> None:
        """
        Subscribe to a specific signal by name, or to all signals if no name is provided.

        Once subscribed, any matching signal data published on the message bus is forwarded
        to the `on_signal` handler.

        Parameters
        ----------
        name : str, optional
            The name of the signal to subscribe to. If not provided or an empty
            string is passed, the subscription will include all signals.
            The signal name is case-insensitive and will be capitalized
            (e.g., 'example' becomes 'SignalExample*').

        """
        ...
    def request_data(
        self,
        data_type: DataType,
        client_id: ClientId,
        instrument_id: InstrumentId | None = None,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        limit: int = 0,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4:
        """
        Request custom data for the given data type from the given data client.

        Once the response is received, the data is forwarded from the message bus
        to the `on_historical_data` handler.

        If the request fails, then an error is logged.

        Parameters
        ----------
        data_type : DataType
            The data type for the request.
        client_id : ClientId
            The data client ID.
        start : datetime, optional
            The start datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        limit : int, optional
            The limit on the amount of data points received.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        ...
    def request_instrument(
        self,
        instrument_id: InstrumentId,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4:
        """
        Request `Instrument` data for the given instrument ID.

        If `end` is ``None`` then will request up to the most recent data.

        Once the response is received, the instrument data is forwarded from the message bus
        to the `on_instrument` handler.

        If the request fails, then an error is logged.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the request.
        start : datetime, optional
            The start datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` is not `None` and > current timestamp (now).
        ValueError
            If `end` is not `None` and > current timestamp (now).
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        ...
    def request_instruments(
        self,
        venue: Venue,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4:
        """
        Request all `Instrument` data for the given venue.

        If `end` is ``None`` then will request up to the most recent data.

        Once the response is received, the instrument data is forwarded from the message bus
        to the `on_instrument` handler.

        If the request fails, then an error is logged.

        Parameters
        ----------
        venue : Venue
            The venue for the request.
        start : datetime, optional
            The start datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` is not `None` and > current timestamp (now).
        ValueError
            If `end` is not `None` and > current timestamp (now).
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        ...
    def request_order_book_snapshot(
        self,
        instrument_id: InstrumentId,
        limit: int = 0,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        params: dict[str, Any] | None = None,
    ) -> UUID4:
        """
        Request an order book snapshot.

        Once the response is received, the order book data is forwarded from the message bus
        to the `on_historical_data` handler.

        If the request fails, then an error is logged.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the order book snapshot request.
        limit : int, optional
            The limit on the depth of the order book snapshot.
        client_id : ClientId, optional
            The specific client ID for the command.
            If None, it will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None] | None = None,
            The registered callback, to be called with the request ID when the response has completed processing.
        params : dict[str, Any] | None = None,
            Additional parameters potentially used by a specific client.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If the instrument_id is None.
        TypeError
            If callback is not None and not of type Callable.

        """
        ...
    def request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        limit: int = 0,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4:
        """
        Request historical `QuoteTick` data.

        If `end` is ``None`` then will request up to the most recent data.

        Once the response is received, the quote tick data is forwarded from the message bus
        to the `on_historical_data` handler.

        If the request fails, then an error is logged.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        start : datetime, optional
            The start datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        limit : int, optional
            The limit on the amount of quote ticks received.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` is not `None` and > current timestamp (now).
        ValueError
            If `end` is not `None` and > current timestamp (now).
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        ...
    def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        limit: int = 0,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4:
        """
        Request historical `TradeTick` data.

        If `end` is ``None`` then will request up to the most recent data.

        Once the response is received, the trade tick data is forwarded from the message bus
        to the `on_historical_data` handler.

        If the request fails, then an error is logged.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        start : datetime, optional
            The start datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        limit : int, optional
            The limit on the amount of trade ticks received.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` is not `None` and > current timestamp (now).
        ValueError
            If `end` is not `None` and > current timestamp (now).
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        ...
    def request_bars(
        self,
        bar_type: BarType,
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        limit: int = 0,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4:
        """
        Request historical `Bar` data.

        If `end` is ``None`` then will request up to the most recent data.

        Once the response is received, the bar data is forwarded from the message bus
        to the `on_historical_data` handler.

        If the request fails, then an error is logged.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.
        start : datetime, optional
            The start datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        limit : int, optional
            The limit on the amount of bars received.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` is not `None` and > current timestamp (now).
        ValueError
            If `end` is not `None` and > current timestamp (now).
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        ...
    def request_aggregated_bars(
        self,
        bar_types: list[BarType],
        start: dt.datetime | None = None,
        end: dt.datetime | None = None,
        limit: int = 0,
        client_id: ClientId | None = None,
        callback: Callable[[UUID4], None] | None = None,
        include_external_data: bool = False,
        update_subscriptions: bool = False,
        update_catalog: bool = False,
        params: dict[str, Any] | None = None,
    ) -> UUID4:
        """
        Request historical aggregated `Bar` data for multiple bar types.
        The first bar is used to determine which market data type will be queried.
        This can either be quotes, trades or bars. If bars are queried,
        the first bar type needs to have a composite bar that is external (i.e. not internal/aggregated).
        This external bar type will be queried.

        If `end` is ``None`` then will request up to the most recent data.

        Once the response is received, the bar data is forwarded from the message bus
        to the `on_historical_data` handler. Any tick data used for aggregation is also
        forwarded to the `on_historical_data` handler.

        If the request fails, then an error is logged.

        Parameters
        ----------
        bar_types : list[BarType]
            The list of bar types for the request. Composite bars can also be used and need to
            figure in the list after a BarType on which it depends.
        start : datetime, optional
            The start datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        limit : int, optional
            The limit on the amount of data received (quote ticks, trade ticks or bars).
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.
        include_external_data : bool, default False
            If True, includes the queried external data in the response.
        update_subscriptions : bool, default False
            If True, updates the aggregators of any existing or future subscription with the queried external data.
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` is not `None` and > current timestamp (now).
        ValueError
            If `end` is not `None` and > current timestamp (now).
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        ValueError
            If `bar_types` is empty.
        TypeError
            If `callback` is not `None` and not of type `Callable`.
        TypeError
            If `bar_types` is empty or contains elements not of type `BarType`.

        """
        ...
    def is_pending_request(self, request_id: UUID4) -> bool:
        """
        Return whether the request for the given identifier is pending processing.

        Parameters
        ----------
        request_id : UUID4
            The request ID to check.

        Returns
        -------
        bool
            True if request is pending, else False.

        """
        ...
    def has_pending_requests(self) -> bool:
        """
        Return whether the actor is pending processing for any requests.

        Returns
        -------
        bool
            True if any requests are pending, else False.

        """
        ...
    @property
    def pending_requests(self) -> set[UUID4]:
        """
        Return the request IDs which are currently pending processing.

        Returns
        -------
        set[UUID4]

        """
        ...
    def handle_instrument(self, instrument: Instrument) -> None:
        """
        Handle the given instrument.

        Passes to `on_instrument` if state is ``RUNNING``.

        Parameters
        ----------
        instrument : Instrument
            The instrument received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_instruments(self, instruments: list[Instrument]) -> None:
        """
        Handle the given instruments data by handling each instrument individually.

        Parameters
        ----------
        instruments : list[Instrument]
            The instruments received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_order_book_deltas(self, deltas: OrderBookDeltas | Any) -> None:
        """
        Handle the given order book deltas.

        Passes to `on_order_book_deltas` if state is ``RUNNING``.
        The `deltas` will be `nautilus_pyo3.OrderBookDeltas` if the
        pyo3_conversion flag was set for the subscription.

        Parameters
        ----------
        deltas : OrderBookDeltas or nautilus_pyo3.OrderBookDeltas
            The order book deltas received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_order_book_depth(self, depth: OrderBookDepth10) -> None:
        """
        Handle the given order book depth

        Passes to `on_order_book_depth` if state is ``RUNNING``.

        Parameters
        ----------
        depth : OrderBookDepth10
            The order book depth received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_order_book(self, order_book: OrderBook) -> None:
        """
        Handle the given order book.

        Passes to `on_order_book` if state is ``RUNNING``.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_quote_tick(self, tick: QuoteTick) -> None:
        """
        Handle the given quote tick.

        If state is ``RUNNING`` then passes to `on_quote_tick`.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_quote_ticks(self, ticks: list[QuoteTick]) -> None:
        """
        Handle the given historical quote tick data by handling each tick individually.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The ticks received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_trade_tick(self, tick: TradeTick) -> None:
        """
        Handle the given trade tick.

        If state is ``RUNNING`` then passes to `on_trade_tick`.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_mark_price(self, mark_price: MarkPriceUpdate) -> None:
        """
        Handle the given mark price update.

        If state is ``RUNNING`` then passes to `on_mark_price`.

        Parameters
        ----------
        mark_price : MarkPriceUpdate
            The mark price update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_index_price(self, index_price: IndexPriceUpdate) -> None:
        """
        Handle the given index price update.

        If state is ``RUNNING`` then passes to `on_index_price`.

        Parameters
        ----------
        index_price : IndexPriceUpdate
            The index price update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_trade_ticks(self, ticks: list[TradeTick]) -> None:
        """
        Handle the given historical trade tick data by handling each tick individually.

        Parameters
        ----------
        ticks : list[TradeTick]
            The ticks received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Handle the given bar data.

        If state is ``RUNNING`` then passes to `on_bar`.

        Parameters
        ----------
        bar : Bar
            The bar received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_bars(self, bars: list[Bar]) -> None:
        """
        Handle the given historical bar data by handling each bar individually.

        Parameters
        ----------
        bars : list[Bar]
            The bars to handle.

        Warnings
        --------
        System method (not intended to be called by user code).

        Raises
        ------
        RuntimeError
            If bar data has incorrectly sorted timestamps (not monotonically increasing).

        """
        ...
    def handle_instrument_status(self, data: InstrumentStatus) -> None:
        """
        Handle the given instrument status update.

        If state is ``RUNNING`` then passes to `on_instrument_status`.

        Parameters
        ----------
        data : InstrumentStatus
            The status update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_instrument_close(self, update: InstrumentClose) -> None:
        """
        Handle the given instrument close update.

        If state is ``RUNNING`` then passes to `on_instrument_close`.

        Parameters
        ----------
        update : InstrumentClose
            The update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_data(self, data: Data) -> None:
        """
        Handle the given data.

        If state is ``RUNNING`` then passes to `on_data`.

        Parameters
        ----------
        data : Data
            The data received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_signal(self, signal: Data) -> None:
        """
        Handle the given signal.

        If state is ``RUNNING`` then passes to `on_signal`.

        Parameters
        ----------
        signal : Data
            The signal received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def handle_historical_data(self, data: Data) -> None:
        """
        Handle the given historical data.

        Parameters
        ----------
        data : Data
            The historical data received.

        Warnings
        --------
        System method (not intended to be called by user code).

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
    def _handle_data_response(self, response: DataResponse) -> None: ...
    def _handle_instrument_response(self, response: DataResponse) -> None: ...
    def _handle_instruments_response(self, response: DataResponse) -> None: ...
    def _handle_quote_ticks_response(self, response: DataResponse) -> None: ...
    def _handle_trade_ticks_response(self, response: DataResponse) -> None: ...
    def _handle_bars_response(self, response: DataResponse) -> None: ...
    def _handle_aggregated_bars_response(self, response: DataResponse) -> None: ...
    def _finish_response(self, request_id: UUID4) -> None: ...
    def _handle_indicators_for_quote(self, indicators: list[Indicator], tick: QuoteTick) -> None: ...
    def _handle_indicators_for_trade(self, indicators: list[Indicator], tick: TradeTick) -> None: ...
    def _handle_indicators_for_bar(self, indicators: list[Indicator], bar: Bar) -> None: ...

