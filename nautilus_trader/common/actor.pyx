# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Any
from typing import Callable

import cython

from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.common.executor import ActorExecutor
from nautilus_trader.common.executor import TaskId
from nautilus_trader.common.signal import generate_signal_class
from nautilus_trader.core import nautilus_pyo3

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport CMD
from nautilus_trader.common.component cimport REQ
from nautilus_trader.common.component cimport SENT
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport is_logging_initialized
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport RequestBars
from nautilus_trader.data.messages cimport RequestData
from nautilus_trader.data.messages cimport RequestInstrument
from nautilus_trader.data.messages cimport RequestInstruments
from nautilus_trader.data.messages cimport RequestOrderBookDepth
from nautilus_trader.data.messages cimport RequestOrderBookSnapshot
from nautilus_trader.data.messages cimport RequestQuoteTicks
from nautilus_trader.data.messages cimport RequestTradeTicks
from nautilus_trader.data.messages cimport SubscribeBars
from nautilus_trader.data.messages cimport SubscribeData
from nautilus_trader.data.messages cimport SubscribeFundingRates
from nautilus_trader.data.messages cimport SubscribeIndexPrices
from nautilus_trader.data.messages cimport SubscribeInstrument
from nautilus_trader.data.messages cimport SubscribeInstrumentClose
from nautilus_trader.data.messages cimport SubscribeInstruments
from nautilus_trader.data.messages cimport SubscribeInstrumentStatus
from nautilus_trader.data.messages cimport SubscribeMarkPrices
from nautilus_trader.data.messages cimport SubscribeOrderBook
from nautilus_trader.data.messages cimport SubscribeQuoteTicks
from nautilus_trader.data.messages cimport SubscribeTradeTicks
from nautilus_trader.data.messages cimport UnsubscribeBars
from nautilus_trader.data.messages cimport UnsubscribeData
from nautilus_trader.data.messages cimport UnsubscribeFundingRates
from nautilus_trader.data.messages cimport UnsubscribeIndexPrices
from nautilus_trader.data.messages cimport UnsubscribeInstrument
from nautilus_trader.data.messages cimport UnsubscribeInstrumentClose
from nautilus_trader.data.messages cimport UnsubscribeInstruments
from nautilus_trader.data.messages cimport UnsubscribeInstrumentStatus
from nautilus_trader.data.messages cimport UnsubscribeMarkPrices
from nautilus_trader.data.messages cimport UnsubscribeOrderBook
from nautilus_trader.data.messages cimport UnsubscribeQuoteTicks
from nautilus_trader.data.messages cimport UnsubscribeTradeTicks
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport FundingRateUpdate
from nautilus_trader.model.data cimport IndexPriceUpdate
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport MarkPriceUpdate
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport OrderBookDepth10
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.greeks cimport GreeksCalculator
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

    def __init__(self, config: ActorConfig | None = None) -> None:
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
        self._pending_requests: dict[UUID4, Callable[[UUID4], None] | None] = {}
        self._pyo3_conversion_types = set()
        self._signal_classes: dict[str, type] = {}

        # Indicators
        self._indicators: list[Indicator] = []
        self._indicators_for_quotes: dict[InstrumentId, list[Indicator]] = {}
        self._indicators_for_trades: dict[InstrumentId, list[Indicator]] = {}
        self._indicators_for_bars: dict[BarType, list[Indicator]] = {}

        # Configuration
        self._log_events = config.log_events
        self._log_commands = config.log_commands
        self.config = config

        self.trader_id = None  # Initialized when registered
        self.msgbus = None     # Initialized when registered
        self.cache = None      # Initialized when registered
        self.clock = None      # Initialized when registered
        self.greeks = None     # Initialized when registered
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

    cpdef dict[str, bytes] on_save(self):
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
        return {}  # Optionally override in subclass

    cpdef void on_load(self, dict[str, bytes] state):
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
            "occur here, such as subscribing/requesting data",
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
            "following a stop occur here"
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
            "occur here, such as resetting indicators and other state"
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

    cpdef void on_order_book_depth(self, depth):
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

    cpdef void on_mark_price(self, MarkPriceUpdate mark_price):
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
        # Optionally override in subclass

    cpdef void on_index_price(self, IndexPriceUpdate index_price):
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
        # Optionally override in subclass

    cpdef void on_funding_rate(self, FundingRateUpdate funding_rate):
        """
        Actions to be performed when running and receives a funding rate update.

        Parameters
        ----------
        funding_rate : FundingRateUpdate
            The funding rate update received.

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

    cpdef void on_signal(self, signal):
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

    cpdef void on_order_filled(self, OrderFilled event):
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

        self.greeks = GreeksCalculator(msgbus, cache, self.clock)

    cpdef void register_executor(
        self,
        loop: asyncio.AbstractEventLoop,
        executor: Executor,
    ):
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
        Condition.type(executor, Executor, "executor")

        self._executor = ActorExecutor(loop, executor, logger=self._log)
        if self._log is not None:
            self._log.debug(f"Registered {executor}")

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
        if self._log is not None:
            self._log.debug(f"Deregistered `{event.__name__}` from warning log levels")

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
            self.log.info(f"Registered Indicator {indicator} for {instrument_id} quotes")
        else:
            self.log.error(f"Indicator {indicator} already registered for {instrument_id} quotes")

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
            self.log.info(f"Registered Indicator {indicator} for {instrument_id} trades")
        else:
            self.log.error(f"Indicator {indicator} already registered for {instrument_id} trades")

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

        cdef BarType standard_bar_type = bar_type.standard()

        if standard_bar_type not in self._indicators_for_bars:
            self._indicators_for_bars[standard_bar_type] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_bars[standard_bar_type]:
            self._indicators_for_bars[standard_bar_type].append(indicator)
            self.log.info(f"Registered Indicator {indicator} for {standard_bar_type} bars")
        else:
            self.log.error(f"Indicator {indicator} already registered for {standard_bar_type} bars")

# -- ACTOR COMMANDS -------------------------------------------------------------------------------

    cpdef dict[str, bytes] save(self):
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
        if not self.is_initialized:
            self.log.error(
                "Cannot save: actor/strategy has not been registered with a trader",
            )
            return
        try:
            self.log.debug("Saving state")
            user_state = self.on_save()

            if len(user_state) > 0:
                self.log.info(f"Saved state: {list(user_state.keys())}", color=LogColor.BLUE)
            else:
                self.log.info("No user state to save", color=LogColor.BLUE)

            return user_state
        except Exception as e:
            self.log.exception("Error on save", e)
            raise  # Otherwise invalid state information could be saved

    cpdef void load(self, dict[str, bytes] state):
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
        Condition.not_none(state, "state")

        if not state:
            self.log.info("No user state to load", color=LogColor.BLUE)
            return
        try:
            self.log.debug(f"Loading state")
            self.on_load(state)
            self.log.info(f"Loaded state {list(state.keys())}", color=LogColor.BLUE)
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
        Condition.is_true(self.cache.synthetic(synthetic.id) is None, f"`synthetic` {synthetic.id} already exists")

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
        Condition.is_true(self.cache.synthetic(synthetic.id) is not None, f"`synthetic` {synthetic.id} does not exist")

        # This will replace the previous synthetic
        self.cache.add_synthetic(synthetic)

    cpdef queue_for_executor(
        self,
        func: Callable[..., Any],
        tuple args = None,
        dict kwargs = None,
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

        if self._log is not None:
            self._log.debug(
                f"Executor: Queued {task_id}: {func.__name__}({args=}, {kwargs=})", LogColor.BLUE,
            )

        return task_id

    cpdef run_in_executor(
        self,
        func: Callable[..., Any],
        tuple args = None,
        dict kwargs = None,
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

        if self._log is not None:
            self._log.debug(
                f"Executor: Submitted {task_id}: {func.__name__}({args=}, {kwargs=})", LogColor.BLUE,
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
        Return a value indicating whether there are any queued OR active tasks.

        Returns
        -------
        bool

        """
        if self._executor is None:
            return False

        return self._executor.has_queued_tasks() or self._executor.has_active_tasks()

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
            if self._log is not None:
                self._log.warning(f"Executor: {task_id} not found")
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
            if self._log is not None:
                self._log.info(f"Canceled Timer(name={name})")

        if self._executor is not None:
            if self._log is not None:
                self._log.info(f"Canceling executor tasks")
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
        Component._dispose(self)  # Call base cleanup (cancels timers)
        self.on_dispose()

    cpdef void _degrade(self):
        self.on_degrade()

    cpdef void _fault(self):
        self.on_fault()

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe_data(
        self,
        DataType data_type,
        ClientId client_id = None,
        InstrumentId instrument_id = None,
        bint update_catalog = False,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(data_type, "data_type")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        topic = f"data.{data_type.topic}"

        if instrument_id and not data_type.metadata:
            topic = f"data.{data_type.type.__name__}.{instrument_id.venue}.{instrument_id.symbol.topic()}"

        self._msgbus.subscribe(
            topic=topic,
            handler=self.handle_data,
        )

        # TODO during a backtest, use any ClientId for subscribing to custom data from a catalog when not using instrument_id
        if client_id is None and instrument_id is None:
            return

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef SubscribeData command = SubscribeData(
            data_type=data_type,
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue if instrument_id else None,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_instruments(
        self,
        Venue venue,
        ClientId client_id = None,
        bint update_catalog = False,
        dict[str, object] params = None,
    ):
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
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
            Only useful when downloading data during a backtest.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.not_none(venue, "venue")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.instrument.{venue}.*",
            handler=self.handle_instrument,
        )

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef SubscribeInstruments command = SubscribeInstruments(
            client_id=client_id,
            venue=venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_instrument(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        bint update_catalog = False,
        dict[str, object] params = None,
    ):
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
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
            Only useful when downloading data during a backtest.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.instrument"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_instrument,
        )

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef SubscribeInstrument command = SubscribeInstrument(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_order_book_deltas(
        self,
        InstrumentId instrument_id,
        BookType book_type=BookType.L2_MBP,
        int depth = 0,
        ClientId client_id = None,
        bint managed = True,
        bint pyo3_conversion = False,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        if pyo3_conversion:
            self._pyo3_conversion_types.add(OrderBookDeltas)

        self._msgbus.subscribe(
            topic=f"data.book.deltas"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_order_book_deltas,
        )
        cdef SubscribeOrderBook command = SubscribeOrderBook(
            instrument_id=instrument_id,
            book_data_type=OrderBookDelta,
            book_type=book_type,
            depth=depth,
            managed=managed,
            interval_ms=0,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_order_book_depth(
        self,
        InstrumentId instrument_id,
        BookType book_type=BookType.L2_MBP,
        int depth = 0,
        ClientId client_id = None,
        bint managed = True,
        bint pyo3_conversion = False,
        bint update_catalog = False,
        dict[str, object] params = None,
    ):
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
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
            Only useful when downloading data during a backtest.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        if pyo3_conversion:
            self._pyo3_conversion_types.add(OrderBookDepth10)

        self._msgbus.subscribe(
            topic=f"data.book.depth"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_order_book_depth,
        )

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef SubscribeOrderBook command = SubscribeOrderBook(
            instrument_id=instrument_id,
            book_data_type=OrderBookDepth10,
            book_type=book_type,
            depth=depth,
            managed=managed,
            interval_ms=0,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_order_book_at_interval(
        self,
        InstrumentId instrument_id,
        BookType book_type=BookType.L2_MBP,
        int depth = 0,
        int interval_ms = 1000,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_negative(depth, "depth")
        Condition.positive_int(interval_ms, "interval_ms")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        if book_type == BookType.L1_MBP and depth > 1:
            self._log.error(
                "Cannot subscribe to order book snapshots: "
                f"L1 MBP book subscription depth > 1, was {depth}",
            )
            return

        self._msgbus.subscribe(
            topic=f"data.book.snapshots"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}"
                  f".{interval_ms}",
            handler=self.handle_order_book,
        )
        cdef SubscribeOrderBook command = SubscribeOrderBook(
            instrument_id=instrument_id,
            book_data_type=OrderBookDelta,
            book_type=book_type,
            depth=depth,
            managed=True,  # Must be managed by DataEngine to provide snapshots at interval
            interval_ms=interval_ms,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_quote_ticks(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        bint update_catalog = False,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.quotes"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_quote_tick,
        )

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef SubscribeQuoteTicks command = SubscribeQuoteTicks(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_trade_ticks(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        bint update_catalog = False,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.trades"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_trade_tick,
        )

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef SubscribeTradeTicks command = SubscribeTradeTicks(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_mark_prices(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.mark_prices"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_mark_price,
        )
        cdef SubscribeMarkPrices command = SubscribeMarkPrices(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_index_prices(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.index_prices"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_index_price,
        )
        cdef SubscribeIndexPrices command = SubscribeIndexPrices(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_funding_rates(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Subscribe to streaming `FundingRateUpdate` data for the given instrument ID.

        Once subscribed, any matching funding rate updates published on the message bus are forwarded
        to the `on_funding_rate` handler.

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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.funding_rates"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_funding_rate,
        )
        cdef SubscribeFundingRates command = SubscribeFundingRates(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_bars(
        self,
        BarType bar_type,
        ClientId client_id = None,
        bint update_catalog = False,
        dict[str, object] params = None,
    ):
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
        update_catalog : bool, optional
            Whether to update a catalog with the received data.
            Only useful when downloading data during a backtest.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.bars.{bar_type.standard()}",
            handler=self.handle_bar,
        )

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef SubscribeBars command = SubscribeBars(
            bar_type=bar_type,
            client_id=client_id,
            venue=bar_type.instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_instrument_status(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.status.{instrument_id.venue}.{instrument_id.symbol.topic()}",
            handler=self.handle_instrument_status,
        )
        cdef SubscribeInstrumentStatus command = SubscribeInstrumentStatus(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)
        self._log.info(f"Subscribed to {instrument_id} InstrumentStatus")

    cpdef void subscribe_instrument_close(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"data.venue.close_price.{instrument_id.to_str()}",
            handler=self.handle_instrument_close,
        )
        cdef SubscribeInstrumentClose command = SubscribeInstrumentClose(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void subscribe_order_fills(self, InstrumentId instrument_id):
        """
        Subscribe to all order fills for the given instrument ID.

        Once subscribed, any matching order fills published on the message bus are forwarded
        to the `on_order_filled` handler.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to fills for.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.subscribe(
            topic=f"events.fills.{instrument_id}",
            handler=self._handle_order_filled,
        )

    cpdef void unsubscribe_data(
        self,
        DataType data_type,
        ClientId client_id = None,
        InstrumentId instrument_id = None,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(data_type, "data_type")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        topic = f"data.{data_type.topic}"

        if instrument_id and not data_type.metadata:
            topic = f"data.{data_type.type.__name__}.{instrument_id.venue}.{instrument_id.symbol.topic()}"

        self._msgbus.unsubscribe(
            topic=topic,
            handler=self.handle_data,
        )

        # TODO during a backtest, use any ClientId for subscribing to custom data from a catalog when not using instrument_id
        if client_id is None and instrument_id is None:
            return

        cdef UnsubscribeData command = UnsubscribeData(
            data_type=data_type,
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue if instrument_id else None,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_instruments(
        self,
        Venue venue,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(venue, "venue")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.instrument.{venue}.*",
            handler=self.handle_instrument,
        )
        cdef UnsubscribeInstruments command = UnsubscribeInstruments(
            client_id=client_id,
            venue=venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_instrument(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.instrument"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_instrument,
        )
        cdef UnsubscribeInstrument command = UnsubscribeInstrument(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_order_book_deltas(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.book.deltas"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_order_book_deltas,
        )
        cdef UnsubscribeOrderBook command = UnsubscribeOrderBook(
            instrument_id=instrument_id,
            book_data_type=OrderBookDelta,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_order_book_depth(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Unsubscribe the order book depth stream for the given instrument ID.

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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.book.depth"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_order_book_depth,
        )
        cdef UnsubscribeOrderBook command = UnsubscribeOrderBook(
            instrument_id=instrument_id,
            book_data_type=OrderBookDepth10,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_order_book_at_interval(
        self,
        InstrumentId instrument_id,
        int interval_ms = 1000,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.book.snapshots"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}"
                  f".{interval_ms}",
            handler=self.handle_order_book,
        )
        cdef UnsubscribeOrderBook command = UnsubscribeOrderBook(
            instrument_id=instrument_id,
            book_data_type=OrderBookDelta,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_quote_ticks(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Unsubscribe from streaming `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.quotes"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_quote_tick,
        )
        cdef UnsubscribeQuoteTicks command = UnsubscribeQuoteTicks(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_trade_ticks(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Unsubscribe from streaming `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.trades"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_trade_tick,
        )
        cdef UnsubscribeTradeTicks command = UnsubscribeTradeTicks(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_mark_prices(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Unsubscribe from streaming `MarkPriceUpdate` data for the given instrument ID.

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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.mark_prices"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_mark_price,
        )
        cdef UnsubscribeMarkPrices command = UnsubscribeMarkPrices(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_index_prices(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Unsubscribe from streaming `IndexPriceUpdate` data for the given instrument ID.

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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.index_prices"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_index_price,
        )
        cdef UnsubscribeIndexPrices command = UnsubscribeIndexPrices(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_funding_rates(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Unsubscribe from streaming `FundingRateUpdate` data for the given instrument ID.

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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.funding_rates"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol.topic()}",
            handler=self.handle_funding_rate,
        )
        cdef UnsubscribeFundingRates command = UnsubscribeFundingRates(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)

    cpdef void unsubscribe_bars(
        self,
        BarType bar_type,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Unsubscribe from streaming `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        standard_bar_type = bar_type.standard()

        self._msgbus.unsubscribe(
            topic=f"data.bars.{standard_bar_type}",
            handler=self.handle_bar,
        )
        cdef UnsubscribeBars command = UnsubscribeBars(
            bar_type=standard_bar_type,
            client_id=client_id,
            venue=bar_type.instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)
        self._log.info(f"Unsubscribed from {standard_bar_type} bar data")

    cpdef void unsubscribe_instrument_status(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Unsubscribe from status updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from status updates for.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.status.{instrument_id.venue}.{instrument_id.symbol.topic()}",
            handler=self.handle_instrument_status,
        )
        cdef UnsubscribeInstrumentStatus command = UnsubscribeInstrumentStatus(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)
        self._log.info(f"Unsubscribed from {instrument_id} InstrumentStatus")

    cpdef void unsubscribe_instrument_close(
        self,
        InstrumentId instrument_id,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Unsubscribe from close updates for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from close updates for.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"data.venue.close_price.{instrument_id.to_str()}",
            handler=self.handle_instrument_close,
        )
        cdef UnsubscribeInstrumentClose command = UnsubscribeInstrumentClose(
            instrument_id=instrument_id,
            client_id=client_id,
            venue=instrument_id.venue,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._send_data_cmd(command)
        self._log.info(f"Unsubscribed from {instrument_id} InstrumentClose")

    cpdef void unsubscribe_order_fills(self, InstrumentId instrument_id):
        """
        Unsubscribe from all order fills for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from fills for.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.unsubscribe(
            topic=f"events.fills.{instrument_id}",
            handler=self._handle_order_filled,
        )

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
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

        self._msgbus.publish_c(topic=f"data.{data_type.topic}", msg=data)

    cpdef void publish_signal(self, str name, value, uint64_t ts_event = 0):
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
        Condition.not_none(name, "name")
        Condition.not_none(value, "value")
        Condition.is_in(type(value), (int, float, str), "value", "int, float, str")
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")

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

    cpdef void subscribe_signal(self, str name = ""):
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
        Condition.not_none(name, "name")

        cdef str topic = f"Signal{name.title()}*"
        self._msgbus.subscribe(
            topic=f"data.{topic}",
            handler=self.handle_signal,
        )

# -- VALIDATIONS -------------------------------------------------------------------------

    cdef tuple _validate_datetime_range(
        self,
        datetime start,
        datetime end,
    ):
        """
        Validate datetime range parameters.

        Parameters
        ----------
        start : datetime
            The start datetime (UTC) of request time range.
        end : datetime, optional
            The end datetime (UTC) of request time range.

        Returns
        -------
        tuple[datetime, datetime]
            The validated start and end datetimes. If `end` was None,
            it will be replaced with the current UTC time.

        Raises
        ------
        TypeError
            If `start` is None.
        ValueError
            If `start` is > current timestamp (now).
        ValueError
            If `end` is > current timestamp (now).
        ValueError
            If `start` is > `end`.

        """
        cdef datetime now = self.clock.utc_now()
        if end is None:
            end = now

        Condition.not_none(start, "start")
        Condition.is_true(start <= now, "start was > now")
        Condition.is_true(end <= now, "end was > now")
        Condition.is_true(start <= end, "start was > end")

        return (start, end)

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef UUID4 request_data(
        self,
        DataType data_type,
        ClientId client_id,
        InstrumentId instrument_id = None,
        datetime start = None,
        datetime end = None,
        int limit = 0,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        dict[str, object] params = None,
    ):
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
        start : datetime
            The start datetime (UTC) of request time range.
            Cannot be `None`.
            Should be left-inclusive (start <= value), but inclusiveness is not currently guaranteed.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            If `None` then will be replaced with the current UTC time.
            Should be right-inclusive (value <= end), but inclusiveness is not currently guaranteed.
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
            If `start` is `None`.
        ValueError
            If `start` is > current timestamp (now).
        ValueError
            If `end` is > current timestamp (now).
        ValueError
            If `start` is > `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        # TODO: Default start value assignment based on requested type of data
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(client_id, "client_id")
        Condition.not_none(data_type, "data_type")
        Condition.callable_or_none(callback, "callback")

        start, end = self._validate_datetime_range(start, end)

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef UUID4 request_id = UUID4()
        cdef RequestData request = RequestData(
            data_type=data_type,
            instrument_id=instrument_id,
            start=start,
            end=end,
            limit=limit,
            client_id=client_id,
            venue=None,
            callback=self._handle_data_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
            params=params,
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
        update_catalog: bool = False,
        dict[str, object] params = None,
    ):
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
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable_or_none(callback, "callback")

        cdef datetime now = self.clock.utc_now()

        if start is not None:
            Condition.is_true(start <= now, "start was > now")

        if end is not None:
            Condition.is_true(end <= now, "end was > now")

        if start is not None and end is not None:
            Condition.is_true(start <= end, "start was > end")

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef UUID4 request_id = UUID4()
        cdef RequestInstrument request = RequestInstrument(
            instrument_id=instrument_id,
            start=start,
            end=end,
            client_id=client_id,
            venue=instrument_id.venue,
            callback=self._handle_instruments_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
            params=params,
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
        update_catalog: bool = False,
        dict[str, object] params = None,
    ):
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
            Additional parameters potentially used by a specific client:
            - `only_last` (default `True`) retains only the latest instrument record per instrument_id, based on the most recent ts_init.

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
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(venue, "venue")
        Condition.callable_or_none(callback, "callback")

        cdef datetime now = self.clock.utc_now()

        if start is not None:
            Condition.is_true(start <= now, "start was > now")

        if end is not None:
            Condition.is_true(end <= now, "end was > now")

        if start is not None and end is not None:
            Condition.is_true(start <= end, "start was > end")

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef UUID4 request_id = UUID4()
        cdef RequestInstruments request = RequestInstruments(
            start=start,
            end=end,
            client_id=client_id,
            venue=venue,
            callback=self._handle_instruments_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_order_book_snapshot(
        self,
        InstrumentId instrument_id,
        int limit = 0,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
        dict[str, object] params = None,
    ):
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
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has completed processing.
        params : dict[str, Any], optional
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
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable_or_none(callback, "callback")

        cdef UUID4 request_id = UUID4()
        cdef RequestOrderBookSnapshot request = RequestOrderBookSnapshot(
            instrument_id=instrument_id,
            limit=limit,
            client_id=client_id,
            venue=instrument_id.venue,
            callback=self._handle_data_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_order_book_depth(
        self,
        InstrumentId instrument_id,
        datetime start,
        datetime end = None,
        int limit = 0,
        int depth = 10,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
        bint update_catalog: bool = True,
        dict[str, object] params = None,
    ):
        """
        Request historical `OrderBookDepth10` snapshots.

        Once the response is received, the order book depth data is forwarded from the message bus
        to the `on_historical_data` handler.

        If the request fails, then an error is logged.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the order book depths request.
        start : datetime
            The start datetime (UTC) of request time range (inclusive).
        end : datetime, optional
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        limit : int, optional
            The limit on the amount of depth snapshots received.
        depth : int, optional
            The maximum depth for the returned order book data (default is 10).
        client_id : ClientId, optional
            The specific client ID for the command.
            If None, it will be inferred from the venue in the instrument ID.
        callback : Callable[[UUID4], None], optional
            The registered callback, to be called with the request ID when the response has completed processing.
        update_catalog : bool, default True
            If the data catalog should be updated with the received data.
        params : dict[str, Any], optional
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
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable_or_none(callback, "callback")

        start, end = self._validate_datetime_range(start, end)

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef UUID4 request_id = UUID4()
        cdef RequestOrderBookDepth request = RequestOrderBookDepth(
            instrument_id=instrument_id,
            start=start,
            end=end,
            limit=limit,
            depth=depth,
            client_id=client_id,
            venue=instrument_id.venue,
            callback=self._handle_data_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_quote_ticks(
        self,
        InstrumentId instrument_id,
        datetime start,
        datetime end = None,
        int limit = 0,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        dict[str, object] params = None,
    ):
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
        start : datetime
            The start datetime (UTC) of request time range.
            Should be left-inclusive (start <= value), but inclusiveness is not currently guaranteed.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            If `None` then will be replaced with the current UTC time.
            Should be right-inclusive (value <= end), but inclusiveness is not currently guaranteed.
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
        TypeError
            If `start` is `None`.
        ValueError
            If `start` is > current timestamp (now).
        ValueError
            If `end` is > current timestamp (now).
        ValueError
            If `start` is > `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable_or_none(callback, "callback")

        start, end = self._validate_datetime_range(start, end)

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef UUID4 request_id = UUID4()
        cdef RequestQuoteTicks request = RequestQuoteTicks(
            instrument_id=instrument_id,
            start=start,
            end=end,
            limit=limit,
            client_id=client_id,
            venue=instrument_id.venue,
            callback=self._handle_quote_ticks_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_trade_ticks(
        self,
        InstrumentId instrument_id,
        datetime start,
        datetime end = None,
        int limit = 0,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        dict[str, object] params = None,
    ):
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
        start : datetime
            The start datetime (UTC) of request time range.
            Should be left-inclusive (start <= value), but inclusiveness is not currently guaranteed.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            If `None` then will be replaced with the current UTC time.
            Should be right-inclusive (value <= end), but inclusiveness is not currently guaranteed.
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
        TypeError
            If `start` is `None`.
        ValueError
            If `start` is > current timestamp (now).
        ValueError
            If `end` is > current timestamp (now).
        ValueError
            If `start` is > `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.callable_or_none(callback, "callback")

        start, end = self._validate_datetime_range(start, end)

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef UUID4 request_id = UUID4()
        cdef RequestTradeTicks request = RequestTradeTicks(
            instrument_id=instrument_id,
            start=start,
            end=end,
            limit=limit,
            client_id=client_id,
            venue=instrument_id.venue,
            callback=self._handle_trade_ticks_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_bars(
        self,
        BarType bar_type,
        datetime start,
        datetime end = None,
        int limit = 0,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
        update_catalog: bool = False,
        dict[str, object] params = None,
    ):
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
        start : datetime
            The start datetime (UTC) of request time range.
            Should be left-inclusive (start <= value), but inclusiveness is not currently guaranteed.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            If `None` then will be replaced with the current UTC time.
            Should be right-inclusive (value <= end), but inclusiveness is not currently guaranteed.
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
        TypeError
            If `start` is `None`.
        ValueError
            If `start` is > current timestamp (now).
        ValueError
            If `end` is > current timestamp (now).
        ValueError
            If `start` is > `end`.
        TypeError
            If `callback` is not `None` and not of type `Callable`.

        """
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_none(bar_type, "bar_type")
        Condition.callable_or_none(callback, "callback")

        start, end = self._validate_datetime_range(start, end)

        params = params or {}
        params["update_catalog"] = update_catalog

        cdef UUID4 request_id = UUID4()
        cdef RequestBars request = RequestBars(
            bar_type=bar_type,
            start=start,
            end=end,
            limit=limit,
            client_id=client_id,
            venue=bar_type.instrument_id.venue,
            callback=self._handle_bars_response,
            request_id=request_id,
            ts_init=self._clock.timestamp_ns(),
            params=params,
        )
        self._pending_requests[request_id] = callback
        self._send_data_req(request)

        return request_id

    cpdef UUID4 request_aggregated_bars(
        self,
        list bar_types,
        datetime start,
        datetime end = None,
        int limit = 0,
        ClientId client_id = None,
        callback: Callable[[UUID4], None] | None = None,
        bint include_external_data = False,
        bint update_subscriptions = False,
        update_catalog: bool = False,
        dict[str, object] params = None,
    ):
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
        start : datetime
            The start datetime (UTC) of request time range.
            Should be left-inclusive (start <= value), but inclusiveness is not currently guaranteed.
        end : datetime, optional
            The end datetime (UTC) of request time range.
            If `None` then will be replaced with the current UTC time.
            Should be right-inclusive (value <= end), but inclusiveness is not currently guaranteed.
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
        TypeError
            If `start` is `None`.
        ValueError
            If `start` is > current timestamp (now).
        ValueError
            If `end` is > current timestamp (now).
        ValueError
            If `start` is > `end`.
        ValueError
            If `bar_types` is empty.
        TypeError
            If `callback` is not `None` and not of type `Callable`.
        TypeError
            If `bar_types` is empty or contains elements not of type `BarType`.

        """
        Condition.is_true(self.trader_id is not None, "The actor has not been registered")
        Condition.not_empty(bar_types, "bar_types")
        Condition.list_type(bar_types, BarType, "bar_types")
        Condition.callable_or_none(callback, "callback")

        start, end = self._validate_datetime_range(start, end)

        for bar_type in bar_types:
            if not bar_type.is_internally_aggregated():
                self._log.error(f"request_aggregated_bars: {bar_type} must be internally aggregated")
                return

        cdef UUID4 request_id = UUID4()
        cdef BarType first_bar_type = bar_types[0]

        params = params or {}
        params["bar_type"] = first_bar_type.composite()
        params["bar_types"] = tuple(bar_types)
        params["include_external_data"] = include_external_data
        params["update_subscriptions"] = update_subscriptions
        params["update_catalog"] = update_catalog

        if first_bar_type.is_composite():
            params["bars_market_data_type"] = "bars"
            request = RequestBars(
                bar_type=first_bar_type.composite(),
                start=start,
                end=end,
                limit=limit,
                client_id=client_id,
                venue=first_bar_type.instrument_id.venue,
                callback=self._handle_aggregated_bars_response,
                request_id=request_id,
                ts_init=self._clock.timestamp_ns(),
                params=params,
            )
        elif first_bar_type.spec.price_type == PriceType.LAST:
            params["bars_market_data_type"] = "trade_ticks"
            request = RequestTradeTicks(
                instrument_id=first_bar_type.instrument_id,
                start=start,
                end=end,
                limit=limit,
                client_id=client_id,
                venue=first_bar_type.instrument_id.venue,
                callback=self._handle_aggregated_bars_response,
                request_id=request_id,
                ts_init=self._clock.timestamp_ns(),
                params=params,
            )
        else:
            params["bars_market_data_type"] = "quote_ticks"
            request = RequestQuoteTicks(
                instrument_id=first_bar_type.instrument_id,
                start=start,
                end=end,
                limit=limit,
                client_id=client_id,
                venue=first_bar_type.instrument_id.venue,
                callback=self._handle_aggregated_bars_response,
                request_id=request_id,
                ts_init=self._clock.timestamp_ns(),
                params=params,
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

        if self._fsm.state in (ComponentState.STARTING, ComponentState.RUNNING):
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
            self._log.info(f"Received <Instrument[{length}]> data for {instrument_id.venue}")
        else:
            self._log.warning("Received <Instrument[]> data with no instruments")

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

    cpdef void handle_order_book_depth(self, OrderBookDepth10 depth):
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
        Condition.not_none(depth, "depth")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_order_book_depth(depth)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(depth)}", e)
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
            self._log.info(f"Received <QuoteTick[{length}]> data for {instrument_id}")
        else:
            self._log.warning("Received <QuoteTick[]> data with no ticks")
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

    cpdef void handle_mark_price(self, MarkPriceUpdate mark_price):
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
        Condition.not_none(mark_price, "mark_price")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_mark_price(mark_price)
            except Exception as e:
                self.log.exception(f"Error on handling {repr(mark_price)}", e)
                raise

    cpdef void handle_index_price(self, IndexPriceUpdate index_price):
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
        Condition.not_none(index_price, "index_price")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_index_price(index_price)
            except Exception as e:
                self.log.exception(f"Error on handling {repr(index_price)}", e)
                raise

    cpdef void handle_funding_rate(self, FundingRateUpdate funding_rate):
        """
        Handle the given funding rate update.

        If state is ``RUNNING`` then passes to `on_funding_rate`.

        Parameters
        ----------
        funding_rate : FundingRateUpdate
            The funding rate update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(funding_rate, "funding_rate")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_funding_rate(funding_rate)
            except Exception as e:
                self.log.exception(f"Error on handling {repr(funding_rate)}", e)
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
            self._log.info(f"Received <TradeTick[{length}]> data for {instrument_id}")
        else:
            self._log.warning("Received <TradeTick[]> data with no ticks")
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

        Raises
        ------
        RuntimeError
            If bar data has incorrectly sorted timestamps (not monotonically increasing).

        """
        Condition.not_none(bars, "bars")  # Can be empty

        cdef int length = len(bars)
        cdef Bar first = bars[0] if length > 0 else None
        cdef Bar last = bars[length - 1] if length > 0 else None

        if length > 0:
            self._log.info(f"Received <Bar[{length}]> data for {first.bar_type}")
        else:
            self._log.warning("Received empty bars response (no data returned)")
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

    cpdef void handle_signal(self, Data signal):
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
        Condition.not_none(signal, "signal")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_signal(signal)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(signal)}", e)
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

    cpdef void _handle_aggregated_bars_response(self, DataResponse response):
        if "bars" in response.data:
            for bars in response.data["bars"].values():
                self.handle_bars(bars)

        if "quote_ticks" in response.data:
            self.handle_quote_ticks(response.data["quote_ticks"])

        if "trade_ticks" in response.data:
            self.handle_trade_ticks(response.data["trade_ticks"])

        self._finish_response(response.correlation_id)

    cpdef void _handle_order_filled(self, OrderFilled event):
        if str(event.strategy_id) == str(self.id):
            # This represents a strategies automatic subscription to it's own
            # order events, so we don't need to pass this event to the handler twice
            return

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_order_filled(event)
            except Exception as e:
                self._log.exception(f"Error on handling {repr(event)}", e)
                raise

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
        if self._log_commands and is_logging_initialized():
            self._log.info(f"{CMD}{SENT} {command}")

        self._msgbus.send(endpoint="DataEngine.execute", msg=command)

    cdef void _send_data_req(self, RequestData request):
        if is_logging_initialized():
            self._log.info(f"{REQ}{SENT} {request}")

        self._msgbus.request(endpoint="DataEngine.request", request=request)
