# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

"""
The `Actor` class allows traders to implement their own customized components.

A user can inherit from `Actor` and optionally override any of the
"on" named event handler methods. The class is not entirely initialized in a stand-alone
way, the intended usage is to pass actors to a `Trader` so that they can be
fully "wired" into the platform. Exceptions will be raised if an `Actor`
attempts to operate without a managing `Trader` instance.

"""

import asyncio
from concurrent.futures import Executor

import cython

from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.common.executor import ActorExecutor
from nautilus_trader.common.executor import TaskId
from nautilus_trader.persistence.writer import generate_signal_class

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport CMD
from nautilus_trader.common.component cimport REQ
from nautilus_trader.common.component cimport SENT
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport LiveClock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.common cimport logging_is_initialized
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.messages cimport DataRequest
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport Subscribe
from nautilus_trader.data.messages cimport Unsubscribe
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.data cimport VenueStatus
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class Actor(Component):
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

    def __init__(self, config: ActorConfig | None = None):
        if config is None:
            config = ActorConfig()
        Condition.type(config, ActorConfig, "config")

        if isinstance(config.component_id, str):
            component_id = ComponentId(config.component_id)
        else:
            component_id = config.component_id

        super().__init__(
            clock=Clock(),  # Use placeholder until registered
            component_id=component_id,
            config=config,
        )

        self._warning_events: set[type] = set()
        self._signal_classes: dict[str, type] = {}
        self._pending_requests: dict[UUID4, Callable[[UUID4], None] | None] = {}

        # Indicators
        self._indicators: list[Indicator] = []
        self._indicators_for_quotes: dict[InstrumentId, list[Indicator]] = {}
        self._indicators_for_trades: dict[InstrumentId, list[Indicator]] = {}
        self._indicators_for_bars: dict[BarType, list[Indicator]] = {}

        self._pyo3_conversion_types = set()

        # Configuration
        self.config = config

        self.trader_id = None  # Initialized when registered
        self.msgbus = None     # Initialized when registered
        self.cache = None      # Initialized when registered
        self.clock = None      # Initialized when registered
        self.log = self._log

    def to_importable_config(self) -> ImportableActorConfig:
        """
        Returns an importable configuration for this actor.

        Returns
        -------
        ImportableActorConfig

        """
        return ImportableActorConfig(
            actor_path=self.fully_qualified_name(),
            config_path=self.config.fully_qualified_name(),
            config=self.config.dict(),
        )

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef dict on_save(self):
        """
        Actions to be performed when the actor state is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        return {}  # Optionally override in subclass

    cpdef void on_load(self, dict state):
        """
        Actions to be performed when the actor state is loaded.

        Saved state values will be contained in the give state dictionary.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_start(self):
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
        # Should override in subclass
        self.log.warning(
            "The `Actor.on_start` handler was called when not overridden. "
            "It's expected that any actions required when starting the actor "
            "occur here, such as subscribing/requesting data.",
        )

    cpdef void on_stop(self):
        """
        Actions to be performed on stop.

        The intent is that this method is called to pause, or when done for day.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in a user implementation.

        """
        # Should override in subclass
        self.log.warning(
            "The `Actor.on_stop` handler was called when not overridden. "
            "It's expected that any actions required when stopping the actor "
            "occur here, such as unsubscribing from data.",
        )

    cpdef void on_resume(self):
        """
        Actions to be performed on resume.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Should override in subclass
        self.log.warning(
            "The `Actor.on_resume` handler was called when not overridden. "
            "It's expected that any actions required when resuming the actor "
            "following a stop occur here."
        )

    cpdef void on_reset(self):
        """
        Actions to be performed on reset.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in a user implementation.

        """
        # Should override in subclass
        self.log.warning(
            "The `Actor.on_reset` handler was called when not overridden. "
            "It's expected that any actions required when resetting the actor "
            "occur here, such as resetting indicators and other state."
        )

    cpdef void on_dispose(self):
        """
        Actions to be performed on dispose.

        Cleanup/release any resources used here.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_degrade(self):
        """
        Actions to be performed on degrade.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the actor implementation.

        """
        # Optionally override in subclass

    cpdef void on_fault(self):
        """
        Actions to be performed on fault.

        Cleanup any resources used by the actor here.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the actor implementation.

        """
        # Optionally override in subclass

    cpdef void on_venue_status(self, VenueStatus data):
        """
        Actions to be performed when running and receives a venue status update.

        Parameters
        ----------
        data : VenueStatus
            The venue status update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_instrument_status(self, InstrumentStatus data):
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
        # Optionally override in subclass

    cpdef void on_instrument_close(self, InstrumentClose update):
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
        # Optionally override in subclass

    cpdef void on_instrument(self, Instrument instrument):
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
        # Optionally override in subclass

    cpdef void on_order_book(self, OrderBook order_book):
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
        # Optionally override in subclass

    cpdef void on_order_book_deltas(self, deltas):
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
        # Optionally override in subclass

    cpdef void on_quote_tick(self, QuoteTick tick):
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
        # Optionally override in subclass

    cpdef void on_trade_tick(self, TradeTick tick):
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
        # Optionally override in subclass

    cpdef void on_bar(self, Bar bar):
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
        # Optionally override in subclass

    cpdef void on_data(self, data):
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
        # Optionally override in subclass

    cpdef void on_historical_data(self, data):
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
        # Optionally override in subclass

    cpdef void on_event(self, Event event):
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
        # Optionally override in subclass

    @property
    def registered_indicators(self):
        """
        Return the registered indicators for the strategy.

        Returns
        -------
        list[Indicator]

        """
        return self._indicators.copy()

    cpdef bint indicators_initialized(self):
        """
        Return a value indicating whether all indicators are initialized.

        Returns
        -------
        bool
            True if all initialized, else False

        """
        if not self._indicators:
            return False

        cdef Indicator indicator
        for indicator in self._indicators:
            if not indicator.initialized:
                return False
        return True

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_base(
        self,
        PortfolioFacade portfolio,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
    ):
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
        Condition.not_none(portfolio, "portfolio")
        Condition.not_none(msgbus, "msgbus")
        Condition.not_none(cache, "cache")
        Condition.not_none(clock, "clock")

        clock.register_default_handler(self.handle_event)
        self._change_clock(clock)
        self._change_msgbus(msgbus)  # The trader ID is assigned here

        self.portfolio = portfolio  # Assigned as PortfolioFacade
        self.msgbus = msgbus
        self.cache = cache
        self.clock = self._clock
        self.log = self._log

    cpdef void register_executor(
        self,
        loop: asyncio.AbstractEventLoop,
        executor: Executor,
    ):
        """
        Register the given `Executor` for the actor.

        Parameters
        ----------
        loop : asyncio.AsbtractEventLoop
            The event loop of the application.
        executor : concurrent.futures.Executor
            The executor to register.

        Raises
        ------
        TypeError
            If `executor` is not of type `concurrent.futures.Executor`

        """
        Condition.type(executor, Executor, "executor")

        self._executor = ActorExecutor(loop, executor, logger=self._log)

        self._log.debug(f"Registered {executor}.")

    cpdef void register_warning_event(self, type event):
        """
        Register the given event type for warning log levels.

        Parameters
        ----------
        event : type
            The event class to register.

        """
        Condition.not_none(event, "event")

        self._warning_events.add(event)

    cpdef void deregister_warning_event(self, type event):
        """
        Deregister the given event type from warning log levels.

        Parameters
        ----------
        event : type
            The event class to deregister.

        """
        Condition.not_none(event, "event")

        self._warning_events.discard(event)

        self._log.debug(f"Deregistered `{event.__name__}` from warning log levels.")

    cpdef void register_indicator_for_quote_ticks(self, InstrumentId instrument_id, Indicator indicator):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if instrument_id not in self._indicators_for_quotes:
            self._indicators_for_quotes[instrument_id] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_quotes[instrument_id]:
            self._indicators_for_quotes[instrument_id].append(indicator)
            self.log.info(f"Registered Indicator {indicator} for {instrument_id} quote ticks.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {instrument_id} quote ticks.")

    cpdef void register_indicator_for_trade_ticks(self, InstrumentId instrument_id, Indicator indicator):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if instrument_id not in self._indicators_for_trades:
            self._indicators_for_trades[instrument_id] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_trades[instrument_id]:
            self._indicators_for_trades[instrument_id].append(indicator)
            self.log.info(f"Registered Indicator {indicator} for {instrument_id} trade ticks.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {instrument_id} trade ticks.")

    cpdef void register_indicator_for_bars(self, BarType bar_type, Indicator indicator):
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
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if bar_type not in self._indicators_for_bars:
            self._indicators_for_bars[bar_type] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_bars[bar_type]:
            self._indicators_for_bars[bar_type].append(indicator)
            self.log.info(f"Registered Indicator {indicator} for {bar_type} bars.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {bar_type} bars.")

# -- ACTOR COMMANDS -------------------------------------------------------------------------------

    cpdef dict save(self):
        """
        Return the actor/strategy state dictionary to be saved.

        Calls `on_save`.

        Raises
        ------
        RuntimeError
            If `actor/strategy` is not registered with a trader.

        Warnings
        --------
        Exceptions raised will be caught, logged, and reraised.

        """
        if not self.is_initialized:
            self.log.error(
                "Cannot save: actor/strategy has not been registered with a trader.",
            )
            return
        try:
            self.log.debug("Saving state...")
            user_state = self.on_save()
            if len(user_state) > 0:
                self.log.info(f"Saved state: {list(user_state.keys())}.", color=LogColor.BLUE)
            else:
                self.log.info("No user state to save.", color=LogColor.BLUE)
            return user_state
        except Exception as e:
            self.log.exception("Error on save", e)
            raise  # Otherwise invalid state information could be saved

    cpdef void load(self, dict state):
        """
        Load the actor/strategy state from the give state dictionary.

        Calls `on_load` and passes the state.

        Parameters
        ----------
        state : dict[str, object]
            The state dictionary.

        Raises
        ------
        RuntimeError
            If `actor/strategy` is not registered with a trader.

        Warnings
        --------
        Exceptions raised will be caught, logged, and reraised.

        """
        Condition.not_none(state, "state")

        if not state:
            self.log.info("No user state to load.", color=LogColor.BLUE)
            return

        try:
            self.log.debug(f"Loading state...")
            self.on_load(state)
            self.log.info(f"Loaded state {list(state.keys())}.", color=LogColor.BLUE)
        except Exception as e:
            self.log.exception(f"Error on load {repr(state)}", e)
            raise

    cpdef void add_synthetic(self, SyntheticInstrument synthetic):
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
        Condition.not_none(synthetic, "synthetic")
        Condition.true(self.cache.synthetic(synthetic.id) is None, f"`synthetic` {synthetic.id} already exists")

        self.cache.add_synthetic(synthetic)

    cpdef void update_synthetic(self, SyntheticInstrument synthetic):
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
        Condition.not_none(synthetic, "synthetic")
        Condition.true(self.cache.synthetic(synthetic.id) is not None, f"`synthetic` {synthetic.id} does not exist")

        # This will replace the previous synthetic
        self.cache.add_synthetic(synthetic)

    cpdef queue_for_executor(
        self,
        func: Callable[..., Any],
        tuple args=None,
        dict kwargs=None,
    ):
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
        Condition.callable(func, "func")

        if args is None:
            args = ()
        if kwargs is None:
            kwargs = {}

        if self._executor is None:
            func(*args, **kwargs)
            task_id = TaskId.create()
        else:
            task_id = self._executor.queue_for_executor(
                func,
                *args,
                **kwargs,
            )

        self._log.info(
                f"Executor: Queued {task_id}: {func.__name__}({args=}, {kwargs=}).", LogColor.BLUE,
        )
        return task_id

    cpdef run_in_executor(
        self,
        func: Callable[..., Any],
        tuple args=None,
        dict kwargs=None,
    ):
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
        Condition.callable(func, "func")

        if args is None:
            args = ()
        if kwargs is None:
            kwargs = {}

        if self._executor is None:
            func(*args, **kwargs)
            task_id = TaskId.create()
        else:
            task_id = self._executor.run_in_executor(
                func,
                *args,
                **kwargs,
            )

        self._log.info(
            f"Executor: Submitted {task_id}: {func.__name__}({args=}, {kwargs=}).", LogColor.BLUE,
        )
        return task_id

    cpdef list queued_task_ids(self):
        """
        Return the queued task identifiers.

        Returns
        -------
        list[TaskId]

        """
        if self._executor is None:
            return []  # Tasks are immediately executed

        return self._executor.queued_task_ids()

    cpdef list active_task_ids(self):
        """
        Return the active task identifiers.

        Returns
        -------
        list[TaskId]

        """
        if self._executor is None:
            return []  # Tasks are immediately executed

        return self._executor.active_task_ids()

    cpdef bint has_queued_tasks(self):
        """
        Return a value indicating whether there are any queued tasks.

        Returns
        -------
        bool

        """
        if self._executor is None:
            return False

        return self._executor.has_queued_tasks()

    cpdef bint has_active_tasks(self):
        """
        Return a value indicating whether there are any active tasks.

        Returns
        -------
        bool

        """
        if self._executor is None:
            return False

        return self._executor.has_active_tasks()

    cpdef bint has_any_tasks(self):
        """
        Return a value indicating whether there are any queued or active tasks.

        Returns
        -------
        bool

        """
        if self._executor is None:
            return False

        return self._executor.has_queued_tasks() and self._executor.has_active_tasks()

    cpdef void cancel_task(self, task_id: TaskId):
        """
        Cancel the task with the given `task_id` (if queued or active).

        If the task is not found then a warning is logged.

        Parameters
        ----------
        task_id : TaskId
            The task identifier.

        """
        if self._executor is None:
            self._log.warning(f"Executor: {task_id} not found.")
            return

        self._executor.cancel_task(task_id)

    cpdef void cancel_all_tasks(self):
        """
        Cancel all queued and active tasks.
        """
        if self._executor is None:
            return

        self._executor.cancel_all_tasks()

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _start(self):
        self.on_start()

    cpdef void _stop(self):
        self.on_stop()

        # Clean up clock
        cdef list timer_names = self._clock.timer_names
        self._clock.cancel_timers()

        cdef str name
        for name in timer_names:
            self._log.info(f"Canceled Timer(name={name}).")

        if self._executor is not None:
            self._log.info(f"Canceling executor tasks...")
            self._executor.cancel_all_tasks()

    cpdef void _resume(self):
        self.on_resume()

    cpdef void _reset(self):
        self.on_reset()

        self._pending_requests.clear()

        self._indicators.clear()
        self._indicators_for_quotes.clear()
        self._indicators_for_trades.clear()
        self._indicators_for_bars.clear()

    cpdef void _dispose(self):
        self.on_dispose()

    cpdef void _degrade(self):
        self.on_degrade()

    cpdef void _fault(self):
        self.on_fault()

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe_data(self, DataType data_type, ClientId client_id = None):
        """
        Subscribe to data of the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type to subscribe to.
        client_id : ClientId, optional
            The data client ID. If supplied then a `Subscribe` command will be
            sent to the corresponding data client.

        """
        Condition.not_none(data_type, "data_type")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.{data_type.topic}",
            handler=self.handle_data,
        )

        if client_id is None:
            return

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=None,
            data_type=data_type,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_instruments(self, Venue venue, ClientId client_id = None):
        """
        Subscribe to update `Instrument` data for the given venue.

        Parameters
        ----------
        venue : Venue
            The venue for the subscription.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue.

        """
        Condition.not_none(venue, "venue")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.instrument.{venue}.*",
            handler=self.handle_instrument,
        )

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=venue,
            data_type=DataType(Instrument),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_instrument(self, InstrumentId instrument_id, ClientId client_id = None):
        """
        Subscribe to update `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.instrument"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_instrument,
        )

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(Instrument, metadata={"instrument_id": instrument_id}),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_order_book_deltas(
        self,
        InstrumentId instrument_id,
        BookType book_type=BookType.L2_MBP,
        int depth = 0,
        dict kwargs = None,
        ClientId client_id = None,
        bint managed = True,
        bint pyo3_conversion = False,
    ):
        """
        Subscribe to the order book data stream, being a snapshot then deltas
        for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument ID to subscribe to.
        book_type : BookType {``L1_MBP``, ``L2_MBP``, ``L3_MBO``}
            The order book type.
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        managed : bool, default True
            If an order book should be managed by the data engine based on the subscribed feed.
        pyo3_conversion : bool, default False
            If received deltas should be converted to `nautilus_pyo3.OrderBookDeltas`
            prior to being passed to the `on_order_book_deltas` handler.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        if pyo3_conversion:
            self._pyo3_conversion_types.add(OrderBookDeltas)

        self._msgbus.subscribe(
            topic=f"data.book.deltas"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_order_book_deltas,
        )

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(OrderBookDelta, metadata={
                "instrument_id": instrument_id,
                "book_type": book_type,
                "depth": depth,
                "kwargs": kwargs,
                "managed": managed,
            }),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_order_book_snapshots(
        self,
        InstrumentId instrument_id,
        BookType book_type=BookType.L2_MBP,
        int depth = 0,
        int interval_ms = 1000,
        dict kwargs = None,
        ClientId client_id = None,
        bint managed = True,
    ):
        """
        Subscribe to `OrderBook` snapshots at a specified interval, for the given instrument ID.

        The `DataEngine` will only maintain one order book for each instrument.
        Because of this - the level, depth and kwargs for the stream will be set
        as per the last subscription request (this will also affect all subscribers).

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument ID to subscribe to.
        book_type : BookType {``L1_MBP``, ``L2_MBP``, ``L3_MBO``}
            The order book type.
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        interval_ms : int
            The order book snapshot interval in milliseconds (must be positive).
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        managed : bool, default True
            If an order book should be managed by the data engine based on the subscribed feed.

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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_negative(depth, "depth")
        Condition.positive_int(interval_ms, "interval_ms")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        if book_type == BookType.L1_MBP and depth > 1:
            self._log.error(
                "Cannot subscribe to order book snapshots: "
                f"L1 TBBO book subscription depth > 1, was {depth}",
            )
            return

        self._msgbus.subscribe(
            topic=f"data.book.snapshots"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}"
                  f".{interval_ms}",
            handler=self.handle_order_book,
        )

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(OrderBook, metadata={
                "instrument_id": instrument_id,
                "book_type": book_type,
                "depth": depth,
                "interval_ms": interval_ms,
                "kwargs": kwargs,
                "managed": managed,
            }),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id, ClientId client_id = None):
        """
        Subscribe to streaming `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.quotes"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_quote_tick,
        )

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(QuoteTick, metadata={"instrument_id": instrument_id}),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id, ClientId client_id = None):
        """
        Subscribe to streaming `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.trades"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_trade_tick,
        )

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(TradeTick, metadata={"instrument_id": instrument_id}),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_bars(
        self,
        BarType bar_type,
        ClientId client_id = None,
        bint await_partial = False,
    ):
        """
        Subscribe to streaming `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        await_partial : bool, default False
            If the bar aggregator should await the arrival of a historical partial bar prior
            to activaely aggregating new bars.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.bars.{bar_type}",
            handler=self.handle_bar,
        )

        cdef dict metadata = {
            "bar_type": bar_type,
            "await_partial": await_partial,
        }

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=bar_type.instrument_id.venue,
            data_type=DataType(Bar, metadata=metadata),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_venue_status(self, Venue venue, ClientId client_id = None):
        """
        Subscribe to status updates for the given venue.

        Parameters
        ----------
        venue : Venue
            The venue to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue.

        """
        Condition.not_none(venue, "venue")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.status.{venue.to_str()}",
            handler=self.handle_venue_status,
        )

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=venue,
            data_type=DataType(VenueStatus),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_instrument_status(self, InstrumentId instrument_id, ClientId client_id = None):
        """
        Subscribe to status updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to status updates for.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.status.{instrument_id.venue}.{instrument_id.symbol}",
            handler=self.handle_instrument_status,
        )

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(InstrumentStatus, metadata={"instrument_id": instrument_id}),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)
        self._log.info(f"Subscribed to {instrument_id} InstrumentStatus.")

    cpdef void subscribe_instrument_close(self, InstrumentId instrument_id, ClientId client_id = None):
        """
        Subscribe to close updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to status updates for.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.venue.close_price.{instrument_id.to_str()}",
            handler=self.handle_instrument_close,
        )

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(InstrumentClose, metadata={"instrument_id": instrument_id}),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_data(self, DataType data_type, ClientId client_id = None):
        """
        Unsubscribe from data of the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type to unsubscribe from.
        client_id : ClientId, optional
            The data client ID. If supplied then an `Unsubscribe` command will
            be sent to the data client.

        """
        Condition.not_none(data_type, "data_type")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.{data_type.topic}",
            handler=self.handle_data,
        )

        if client_id is None:
            return

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            venue=None,
            data_type=data_type,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_instruments(self, Venue venue, ClientId client_id = None):
        """
        Unsubscribe from update `Instrument` data for the given venue.

        Parameters
        ----------
        venue : Venue
            The venue for the subscription.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue.

        """
        Condition.not_none(venue, "venue")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.instrument.{venue}.*",
            handler=self.handle_instrument,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            venue=venue,
            data_type=DataType(Instrument),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id, ClientId client_id = None):
        """
        Unsubscribe from update `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.instrument"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_instrument,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(Instrument, metadata={"instrument_id": instrument_id}),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id, ClientId client_id = None):
        """
        Unsubscribe the order book deltas stream for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.book.deltas"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_order_book_deltas,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(OrderBookDelta, metadata={"instrument_id": instrument_id}),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_order_book_snapshots(
        self,
        InstrumentId instrument_id,
        int interval_ms = 1000,
        ClientId client_id = None,
    ):
        """
        Unsubscribe from `OrderBook` snapshots, for the given instrument ID.

        The interval must match the previously subscribed interval.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        interval_ms : int
            The order book snapshot interval in milliseconds.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.book.snapshots"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}"
                  f".{interval_ms}",
            handler=self.handle_order_book,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(OrderBook, metadata={
                "instrument_id": instrument_id,
                "interval_ms": interval_ms,
            }),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id, ClientId client_id = None):
        """
        Unsubscribe from streaming `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.quotes"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_quote_tick,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(QuoteTick, metadata={"instrument_id": instrument_id}),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id, ClientId client_id = None):
        """
        Unsubscribe from streaming `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.trades"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_trade_tick,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(TradeTick, metadata={"instrument_id": instrument_id}),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_bars(self, BarType bar_type, ClientId client_id = None):
        """
        Unsubscribe from streaming `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.bars.{bar_type}",
            handler=self.handle_bar,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            venue=bar_type.instrument_id.venue,
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)
        self._log.info(f"Unsubscribed from {bar_type} bar data.")

    cpdef void unsubscribe_venue_status(self, Venue venue, ClientId client_id = None):
        """
        Unsubscribe to status updates for the given venue.

        Parameters
        ----------
        venue : Venue
            The venue to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue.

        """
        Condition.not_none(venue, "venue")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.status.{venue.to_str()}",
            handler=self.handle_venue_status,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            venue=venue,
            data_type=DataType(VenueStatus),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_instrument_status(self, InstrumentId instrument_id, ClientId client_id = None):
        """
        Unsubscribe to status updates of the given venue.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe to status updates for.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.status.{instrument_id.venue}.{instrument_id.symbol}",
            handler=self.handle_venue_status,
        )
        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(InstrumentStatus),
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)
        self._log.info(f"Unsubscribed from {instrument_id} InstrumentStatus.")


    cpdef void publish_data(self, DataType data_type, Data data):
        """
        Publish the given data to the message bus.

        Parameters
        ----------
        data_type : DataType
            The data type being published.
        data : Data
            The data to publish.

        """
        Condition.not_none(data_type, "data_type")
        Condition.not_none(data, "data")
        Condition.type(data, data_type.type, "data", "data.type")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.publish_c(topic=f"data.{data_type.topic}", msg=data)

    cpdef void publish_signal(self, str name, value, uint64_t ts_event = 0):
        """
        Publish the given value as a signal to the message bus. Optionally setup persistence for this `signal`.

        Parameters
        ----------
        name : str
            The name of the signal being published.
        value : object
            The signal data to publish.
        ts_event : uint64_t, optional
            The UNIX timestamp (nanoseconds) when the signal event occurred.
            If ``None`` then will timestamp current time.

        """
        Condition.not_none(name, "name")
        Condition.not_none(value, "value")
        Condition.is_in(type(value), (int, float, str), "value", "int, float, str")
        Condition.true(self.trader_id is not None, "The actor has not been registered")

        cdef type cls = self._signal_classes.get(name)
        if cls is None:
            cls = generate_signal_class(name=name, value_type=type(value))
            self._signal_classes[name] = cls

        cdef uint64_t now = self.clock.timestamp_ns()
        cdef Data data = cls(
            value=value,
            ts_event=ts_event or now,
            ts_init=now,
        )
        self.publish_data(data_type=DataType(cls), data=data)

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef UUID4 request_data(
        self,
        DataType data_type,
        ClientId client_id,
        callback: Callable[[UUID4], None] | None = None,
    ):
        """
        Request custom data for the given data type from the given data client.

        Parameters
        ----------
        data_type : DataType
            The data type for the request.
        client_id : ClientId
            The data client ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        Condition.true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(client_id, "client_id")
        Condition.not_none(data_type, "data_type")
        Condition.callable_or_none(callback, "callback")

        cdef UUID4 request_id = UUID4()
        cdef DataRequest request = DataRequest(
            client_id=client_id,
            venue=None,
            data_type=data_type,
            callback=self._handle_data_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
        )

        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_instrument(
        self,
        InstrumentId instrument_id,
        datetime start = None,
        datetime end = None,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
    ):
        """
        Request `Instrument` data for the given instrument ID.

        If `end` is ``None`` then will request up to the most recent data.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the request.
        start : datetime, optional
            The start datetime (UTC) of request time range (inclusive).
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        Condition.true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(instrument_id, "instrument_id")
        if start is not None and end is not None:
            Condition.true(start < end, "start was >= end")
        Condition.callable_or_none(callback, "callback")

        cdef UUID4 request_id = UUID4()
        cdef DataRequest request = DataRequest(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(Instrument, metadata={
                "instrument_id": instrument_id,
                "start": start,
                "end": end,
            }),
            callback=self._handle_instrument_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
        )

        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_instruments(
        self,
        Venue venue,
        datetime start = None,
        datetime end = None,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
    ):
        """
        Request all `Instrument` data for the given venue.

        If `end` is ``None`` then will request up to the most recent data.

        Parameters
        ----------
        venue : Venue
            The venue for the request.
        start : datetime, optional
            The start datetime (UTC) of request time range (inclusive).
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        Condition.true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(venue, "venue")
        if start is not None and end is not None:
            Condition.true(start < end, "start was >= end")
        Condition.callable_or_none(callback, "callback")

        cdef UUID4 request_id = UUID4()
        cdef DataRequest request = DataRequest(
            client_id=client_id,
            venue=venue,
            data_type=DataType(Instrument, metadata={
                "venue": venue,
                "start": start,
                "end": end,
            }),
            callback=self._handle_instruments_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
        )

        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_quote_ticks(
        self,
        InstrumentId instrument_id,
        datetime start = None,
        datetime end = None,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
    ):
        """
        Request historical `QuoteTick` data.

        If `end` is ``None`` then will request up to the most recent data.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        start : datetime, optional
            The start datetime (UTC) of request time range (inclusive).
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        Condition.true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(instrument_id, "instrument_id")
        if start is not None and end is not None:
            Condition.true(start < end, "start was >= end")
        Condition.callable_or_none(callback, "callback")

        cdef UUID4 request_id = UUID4()
        cdef DataRequest request = DataRequest(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(QuoteTick, metadata={
                "instrument_id": instrument_id,
                "start": start,
                "end": end,
            }),
            callback=self._handle_quote_ticks_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
        )

        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_trade_ticks(
        self,
        InstrumentId instrument_id,
        datetime start = None,
        datetime end = None,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
    ):
        """
        Request historical `TradeTick` data.

        If `end` is ``None`` then will request up to the most recent data.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        start : datetime, optional
            The start datetime (UTC) of request time range (inclusive).
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        Condition.true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(instrument_id, "instrument_id")
        if start is not None and end is not None:
            Condition.true(start < end, "start was >= end")
        Condition.callable_or_none(callback, "callback")

        cdef UUID4 request_id = UUID4()
        cdef DataRequest request = DataRequest(
            client_id=client_id,
            venue=instrument_id.venue,
            data_type=DataType(TradeTick, metadata={
                "instrument_id": instrument_id,
                "start": start,
                "end": end,
            }),
            callback=self._handle_trade_ticks_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
        )

        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_bars(
        self,
        BarType bar_type,
        datetime start = None,
        datetime end = None,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
    ):
        """
        Request historical `Bar` data.

        If `end` is ``None`` then will request up to the most recent data.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.
        start : datetime, optional
            The start datetime (UTC) of request time range (inclusive).
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has
            completed processing.

        Returns
        -------
        UUID4
            The `request_id` for the request.

        Raises
        ------
        ValueError
            If `start` and `end` are not `None` and `start` is >= `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        Condition.true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(bar_type, "bar_type")
        if start is not None and end is not None:
            Condition.true(start < end, "start was >= end")
        Condition.callable_or_none(callback, "callback")

        cdef UUID4 request_id = UUID4()
        cdef DataRequest request = DataRequest(
            client_id=client_id,
            venue=bar_type.instrument_id.venue,
            data_type=DataType(Bar, metadata={
                "bar_type": bar_type,
                "start": start,
                "end": end,
            }),
            callback=self._handle_bars_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
        )

        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef bint is_pending_request(self, UUID4 request_id):
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
        return request_id in self._pending_requests

    cpdef bint has_pending_requests(self):
        """
        Return whether the actor is pending processing for any requests.

        Returns
        -------
        bool
            True if any requests are pending, else False.

        """
        return len(self._pending_requests) > 0

    cpdef set pending_requests(self):
        """
        Return the request IDs which are currently pending processing.

        Returns
        -------
        set[UUID4]

        """
        return set(self._pending_requests.keys())

# -- HANDLERS -------------------------------------------------------------------------------------

    cpdef void handle_instrument(self, Instrument instrument):
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
        Condition.not_none(instrument, "instrument")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_instrument(instrument)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(instrument)}", e)
                raise

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_instruments(self, list instruments):
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
        Condition.not_none(instruments, "instruments")  # Could be empty

        cdef int length = len(instruments)
        cdef Instrument first = instruments[0] if length > 0 else None
        cdef InstrumentId instrument_id = first.id if first is not None else None

        if length > 0:
            self._log.info(f"Received <Instrument[{length}]> data for {instrument_id.venue}.")
        else:
            self._log.warning("Received <Instrument[]> data with no instruments.")

        cdef int i
        for i in range(length):
            self.handle_instrument(instruments[i])

    cpdef void handle_order_book_deltas(self, deltas):
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
        Condition.not_none(deltas, "deltas")

        if OrderBookDeltas in self._pyo3_conversion_types:
            deltas = deltas.to_pyo3()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_order_book_deltas(deltas)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(deltas)}", e)
                raise

    cpdef void handle_order_book(self, OrderBook order_book):
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
        Condition.not_none(order_book, "order_book")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_order_book(order_book)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(order_book)}", e)
                raise

    cpdef void handle_quote_tick(self, QuoteTick tick):
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
        Condition.not_none(tick, "tick")

        # Update indicators
        cdef list indicators = self._indicators_for_quotes.get(tick.instrument_id)
        if indicators:
            self._handle_indicators_for_quote(indicators, tick)

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_quote_tick(tick)
            except Exception as e:
                self.log.exception(f"Error on handling {repr(tick)}", e)
                raise

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_quote_ticks(self, list ticks):
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
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef QuoteTick first = ticks[0] if length > 0 else None
        cdef InstrumentId instrument_id = first.instrument_id if first is not None else None

        if length > 0:
            self._log.info(f"Received <QuoteTick[{length}]> data for {instrument_id}.")
        else:
            self._log.warning("Received <QuoteTick[]> data with no ticks.")
            return

        # Update indicators
        cdef list indicators = self._indicators_for_quotes.get(first.instrument_id)

        cdef:
            int i
            QuoteTick tick
        for i in range(length):
            tick = ticks[i]
            if indicators:
                self._handle_indicators_for_quote(indicators, tick)
            self.handle_historical_data(tick)

    cpdef void handle_trade_tick(self, TradeTick tick):
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
        Condition.not_none(tick, "tick")

        # Update indicators
        cdef list indicators = self._indicators_for_trades.get(tick.instrument_id)
        if indicators:
            self._handle_indicators_for_trade(indicators, tick)

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_trade_tick(tick)
            except Exception as e:
                self.log.exception(f"Error on handling {repr(tick)}", e)
                raise

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_trade_ticks(self, list ticks):
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
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef TradeTick first = ticks[0] if length > 0 else None
        cdef InstrumentId instrument_id = first.instrument_id if first is not None else None

        if length > 0:
            self._log.info(f"Received <TradeTick[{length}]> data for {instrument_id}.")
        else:
            self._log.warning("Received <TradeTick[]> data with no ticks.")
            return

        # Update indicators
        cdef list indicators = self._indicators_for_trades.get(first.instrument_id)

        cdef:
            int i
            TradeTick tick
        for i in range(length):
            tick = ticks[i]
            if indicators:
                self._handle_indicators_for_trade(indicators, tick)
            self.handle_historical_data(tick)

    cpdef void handle_bar(self, Bar bar):
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
        Condition.not_none(bar, "bar")

        # Update indicators
        cdef list indicators = self._indicators_for_bars.get(bar.bar_type)
        if indicators:
            self._handle_indicators_for_bar(indicators, bar)

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_bar(bar)
            except Exception as e:
                self.log.exception(f"Error on handling {repr(bar)}", e)
                raise

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_bars(self, list bars):
        """
        Handle the given historical bar data by handling each bar individually.

        Parameters
        ----------
        bars : list[Bar]
            The bars to handle.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(bars, "bars")  # Can be empty

        cdef int length = len(bars)
        cdef Bar first = bars[0] if length > 0 else None
        cdef Bar last = bars[length - 1] if length > 0 else None

        if length > 0:
            self._log.info(f"Received <Bar[{length}]> data for {first.bar_type}.")
        else:
            self._log.error(f"Received <Bar[{length}]> data for unknown bar type.")
            return

        if length > 0 and first.ts_init > last.ts_init:
            raise RuntimeError(f"cannot handle <Bar[{length}]> data: incorrectly sorted")

        # Update indicators
        cdef list indicators = self._indicators_for_bars.get(first.bar_type)

        cdef:
            int i
            Bar bar
        for i in range(length):
            bar = bars[i]
            if indicators:
                self._handle_indicators_for_bar(indicators, bar)
            self.handle_historical_data(bar)

    cpdef void handle_venue_status(self, VenueStatus data):
        """
        Handle the given venue status update.

        If state is ``RUNNING`` then passes to `on_venue_status`.

        Parameters
        ----------
        data : VenueStatus
            The status update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(data, "data")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_venue_status(data)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(data)}", e)
                raise

    cpdef void handle_instrument_status(self, InstrumentStatus data):
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
        Condition.not_none(data, "data")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_instrument_status(data)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(data)}", e)
                raise

    cpdef void handle_instrument_close(self, InstrumentClose update):
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
        Condition.not_none(update, "update")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_instrument_close(update)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(update)}", e)
                raise

    cpdef void handle_data(self, Data data):
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
        Condition.not_none(data, "data")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_data(data)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(data)}", e)
                raise

    cpdef void handle_historical_data(self, data):
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
        Condition.not_none(data, "data")

        try:
            self.on_historical_data(data)
        except Exception as e:
            self._log.exception(f"Error on handling {repr(data)}", e)
            raise

    cpdef void handle_event(self, Event event):
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
        Condition.not_none(event, "event")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_event(event)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(event)}", e)
                raise

    cpdef void _handle_data_response(self, DataResponse response):
        if isinstance(response.data, list):
            for data in response.data:
                self.handle_historical_data(data)
        else:
            self.handle_historical_data(response.data)
        self._finish_response(response.correlation_id)

    cpdef void _handle_instrument_response(self, DataResponse response):
        self.handle_instrument(response.data)
        self._finish_response(response.correlation_id)

    cpdef void _handle_instruments_response(self, DataResponse response):
        self.handle_instruments(response.data)
        self._finish_response(response.correlation_id)

    cpdef void _handle_quote_ticks_response(self, DataResponse response):
        self.handle_quote_ticks(response.data)
        self._finish_response(response.correlation_id)

    cpdef void _handle_trade_ticks_response(self, DataResponse response):
        self.handle_trade_ticks(response.data)
        self._finish_response(response.correlation_id)

    cpdef void _handle_bars_response(self, DataResponse response):
        self.handle_bars(response.data)
        self._finish_response(response.correlation_id)

    cpdef void _finish_response(self, UUID4 request_id):
        callback: Callable | None = self._pending_requests.pop(request_id, None)
        if callback is not None:
            callback(request_id)

    cpdef void _handle_indicators_for_quote(self, list indicators, QuoteTick tick):
        cdef Indicator indicator
        for indicator in indicators:
            indicator.handle_quote_tick(tick)

    cpdef void _handle_indicators_for_trade(self, list indicators, TradeTick tick):
        cdef Indicator indicator
        for indicator in indicators:
            indicator.handle_trade_tick(tick)

    cpdef void _handle_indicators_for_bar(self, list indicators, Bar bar):
        cdef Indicator indicator
        for indicator in indicators:
            indicator.handle_bar(bar)

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _send_data_cmd(self, DataCommand command):
        if logging_is_initialized():
            self._log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="DataEngine.execute", msg=command)

    cdef void _send_data_req(self, DataRequest request):
        if logging_is_initialized():
            self._log.info(f"{REQ}{SENT} {request}.")
        self._msgbus.request(endpoint="DataEngine.request", request=request)
