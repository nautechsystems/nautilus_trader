# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

# cython: boundscheck=False
# cython: wraparound=False

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport ValidString, Label
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_purpose cimport OrderPurpose
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.events cimport Event, OrderRejected, OrderCancelReject
from nautilus_trader.model.identifiers cimport Symbol, TraderId, StrategyId, OrderId, PositionId
from nautilus_trader.model.commands cimport AccountInquiry, SubmitOrder, SubmitBracketOrder
from nautilus_trader.model.commands cimport ModifyOrder, CancelOrder
from nautilus_trader.model.generators cimport PositionIdGenerator
from nautilus_trader.model.objects cimport Quantity, Price
from nautilus_trader.model.tick cimport QuoteTick, TradeTick
from nautilus_trader.model.bar cimport BarType, Bar
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.order cimport Order, BracketOrder
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.common.logging cimport Logger, LoggerAdapter, EVT, CMD, SENT, RECV
from nautilus_trader.common.execution cimport ExecutionEngine
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.common.market cimport IndicatorUpdater
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.live.clock cimport LiveClock
from nautilus_trader.live.factories cimport LiveUUIDFactory


cdef class TradingStrategy:
    """
    The base class for all trading strategies.
    """

    def __init__(self,
                 str order_id_tag not None="000",
                 bint flatten_on_stop=True,
                 bint flatten_on_sl_reject=True,
                 bint cancel_all_orders_on_stop=True,
                 Clock clock=None,
                 UUIDFactory uuid_factory=None,
                 Logger logger=None,
                 bint reraise_exceptions=True):
        """
        Initialize a new instance of the TradingStrategy class.

        :param order_id_tag: The order_id tag for the strategy (must be unique at trader level).
        :param flatten_on_stop: If all strategy positions should be flattened on stop.
        :param flatten_on_sl_reject: If open positions should be flattened on SL reject.
        :param cancel_all_orders_on_stop: If all residual orders should be cancelled on stop.
        :param clock: The clock for the strategy (can be None, default=LiveClock).
        :param uuid_factory: The UUID factory for the strategy (can be None, default=LiveUUIDFactory).
        :param logger: The logger for the strategy (can be None).
        :param reraise_exceptions: If exceptions raised in handling methods should be re-raised.
        :raises ValueError: If order_id_tag is not a valid string.
        """
        Condition.valid_string(order_id_tag, "order_id_tag")

        # Identification
        self.id = StrategyId(self.__class__.__name__, order_id_tag)
        self.trader_id = None  # Initialized when registered with a trader

        # Components
        if clock is None:
            clock = LiveClock()
        self.clock = clock
        if uuid_factory is None:
            uuid_factory = LiveUUIDFactory()
        self.uuid_factory = uuid_factory
        self.log = LoggerAdapter(self.id.value, logger)

        self.clock.register_default_handler(self.handle_event)

        # Management flags
        self.flatten_on_stop = flatten_on_stop
        self.flatten_on_sl_reject = flatten_on_sl_reject
        self.cancel_all_orders_on_stop = cancel_all_orders_on_stop
        self.reraise_exceptions = reraise_exceptions

        # Order / Position components
        self.order_factory = None          # Initialized when registered with a trader
        self.position_id_generator = None  # Initialized when registered with a trader

        # Indicators
        self._indicators = []          # type: [Indicator]
        self._indicator_updaters = {}  # type: {Indicator, [IndicatorUpdater]}

        # Registerable modules
        self._data = None  # Initialized when registered with the data client
        self._exec = None  # Initialized when registered with the execution engine

        self.is_running = False

    cpdef bint equals(self, TradingStrategy other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.id.equals(other.id)

    def __eq__(self, TradingStrategy other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, TradingStrategy other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.id.value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}({self.id.value})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"


# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void on_start(self) except *:
        """
        Actions to be performed on strategy start.
        """
        pass  # Optionally override in implementation

    cpdef void on_quote_tick(self, QuoteTick tick) except *:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        :param tick: The tick received.
        """
        pass  # Optionally override in implementation

    cpdef void on_trade_tick(self, TradeTick tick) except *:
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        :param tick: The tick received.
        """
        pass  # Optionally override in implementation

    cpdef void on_bar(self, BarType bar_type, Bar bar) except *:
        """
        Actions to be performed when the strategy is running and receives a bar.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        pass  # Optionally override in implementation

    cpdef void on_data(self, object data) except *:
        """
        Actions to be performed when the strategy is running and receives a data object.

        :param data: The data object received.
        """
        pass  # Optionally override in implementation

    cpdef void on_event(self, Event event) except *:
        """
        Actions to be performed when the strategy is running and receives an event.

        :param event: The event received.
        """
        pass  # Optionally override in implementation

    cpdef void on_stop(self) except *:
        """
        Actions to be performed when the strategy is stopped.
        """
        pass  # Optionally override in implementation

    cpdef void on_reset(self) except *:
        """
        Actions to be performed when the strategy is reset.
        """
        pass  # Optionally override in implementation

    cpdef dict on_save(self):
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Note: 'OrderIdCount' and 'PositionIdCount' are reserved keys for
        the returned state dictionary.
        """
        return {}  # Optionally override in implementation

    cpdef void on_load(self, dict state) except *:
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.
        """
        pass  # Optionally override in implementation

    cpdef void on_dispose(self) except *:
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.
        """
        pass  # Optionally override in implementation


# -- REGISTRATION METHODS --------------------------------------------------------------------------

    cpdef void register_trader(self, TraderId trader_id) except *:
        """
        Change the trader for the strategy.

        :param trader_id: The trader_id to change to.
        """
        Condition.not_none(trader_id, "trader_id")

        self.trader_id = trader_id

        # Create OrderFactory now that order_id_tag is known
        self.order_factory = OrderFactory(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=self.clock,
            uuid_factory=self.uuid_factory)

        # Create PositionIdGenerator now that order_id_tag is known
        self.position_id_generator = PositionIdGenerator(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=self.clock)

        self.log.debug(f"Registered trader {trader_id.value}.")

    cpdef void register_data_client(self, DataClient client) except *:
        """
        Register the strategy with the given data client.

        :param client: The data client to register.
        """
        Condition.not_none(client, "client")

        self._data = client
        self.log.debug("Registered data client.")

    cpdef void register_execution_engine(self, ExecutionEngine engine) except *:
        """
        Register the strategy with the given execution engine.

        :param engine: The execution engine to register.
        """
        Condition.not_none(engine, "engine")

        self._exec = engine
        self.log.debug("Registered execution engine.")

    cpdef void register_indicator(
            self,
            data_source,
            Indicator indicator,
            update_method: callable=None) except *:
        """
        Register the given indicator with the strategy to receive data of the
        given data_source (can be a <Symbol> for <QuoteTick> data, or a
        <BarType> for <Bar> data).

        :param data_source: The data source for updates.
        :param indicator: The indicator to register.
        :param update_method: The update method for the indicator.
        :raises ValueError: If update_method is not of type callable.
        """
        Condition.not_none(data_source, "data_source")
        Condition.not_none(indicator, "indicator")
        if update_method is None:
            update_method = indicator.update
        Condition.callable(update_method, "update_method")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if data_source not in self._indicator_updaters:
            self._indicator_updaters[data_source] = []  # type: [IndicatorUpdater]

        if indicator not in self._indicator_updaters[data_source]:
            self._indicator_updaters[data_source].append(IndicatorUpdater(indicator, update_method))
        else:
            self.log.error(f"Indicator {indicator} already registered for {data_source}.")


# -- HANDLER METHODS -------------------------------------------------------------------------------

    cpdef void handle_quote_tick(self, QuoteTick tick, bint is_historical=False) except *:
        """"
        System method. Handle the given tick.

        :param tick: The tick received.
        :param is_historical: The flag indicating whether the tick is historical
        (won't be passed to on_quote_tick()).
        """
        Condition.not_none(tick, "tick")

        # Update indicators
        cdef list updaters = self._indicator_updaters.get(tick.symbol)  # Can be None
        cdef IndicatorUpdater updater
        if updaters is not None:
            for updater in updaters:
                updater.update_tick(tick)

        if is_historical:
            return  # Don't pass to on_tick()

        if self.is_running:
            try:
                self.on_quote_tick(tick)
            except Exception as ex:
                self.log.exception(ex)
                if self.reraise_exceptions:
                    raise ex  # Re-raise

    cpdef void handle_quote_ticks(self, list ticks) except *:
        """
        System method. Handle the given list of ticks by handling each tick
        individually.
        """
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef Symbol symbol = ticks[0].symbol if length > 0 else None

        if length > 0:
            self.log.info(f"Received <QuoteTick[{length}]> data for {symbol}.")
        else:
            self.log.warning("Received <QuoteTick[]> data with no ticks.")

        cdef int i
        for i in range(length):
            self.handle_quote_tick(ticks[i], is_historical=True)

    cpdef void handle_trade_tick(self, TradeTick tick, bint is_historical=False) except *:
        """"
        System method. Handle the given tick.

        :param tick: The trade tick received.
        :param is_historical: The flag indicating whether the tick is historical
        (won't be passed to on_trade_tick()).
        """
        Condition.not_none(tick, "tick")

        if is_historical:
            return  # Don't pass to on_tick()

        if self.is_running:
            try:
                self.on_trade_tick(tick)
            except Exception as ex:
                self.log.exception(ex)
                if self.reraise_exceptions:
                    raise ex  # Re-raise

    cpdef void handle_trade_ticks(self, list ticks) except *:
        """
        System method. Handle the given list of ticks by handling each tick
        individually.
        """
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef Symbol symbol = ticks[0].symbol if length > 0 else None

        if length > 0:
            self.log.info(f"Received <TradeTick[{length}]> data for {symbol}.")
        else:
            self.log.warning("Received <TradeTick[]> data with no ticks.")

        cdef int i
        for i in range(length):
            self.handle_trade_tick(ticks[i], is_historical=True)

    cpdef void handle_bar(self, BarType bar_type, Bar bar, bint is_historical=False) except *:
        """"
        System method. Handle the given bar type and bar.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        :param is_historical: The flag indicating whether the bar is historical
        (won't be passed to on_bar).
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bar, "bar")

        # Update indicators
        cdef list updaters = self._indicator_updaters.get(bar_type)  # Can be None
        cdef IndicatorUpdater updater
        if updaters is not None:
            for updater in updaters:
                updater.update_bar(bar)

        if is_historical:
            return  # Don't pass to on_bar()

        if self.is_running:
            try:
                self.on_bar(bar_type, bar)
            except Exception as ex:
                self.log.exception(ex)
                if self.reraise_exceptions:
                    raise ex  # Re-raise

    cpdef void handle_bars(self, BarType bar_type, list bars) except *:
        """
        System method. Handle the given bar type and bars by handling each bar
        individually.
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bars, "bars")  # Can be empty

        cdef int length = len(bars)

        self.log.info(f"Received <Bar[{length}]> data for {bar_type}.")

        if length > 0 and bars[0].timestamp > bars[length - 1].timestamp:
            raise RuntimeError("Cannot handle <Bar[]> data (incorrectly sorted).")

        cdef int i
        for i in range(length):
            self.handle_bar(bar_type, bars[i], is_historical=True)

    cpdef void handle_data(self, object data) except *:
        """
        System method. Handle the given data object.
        """
        Condition.not_none(data, "data")

        if self.is_running:
            try:
                self.on_data(data)
            except Exception as ex:
                self.log.exception(ex)
                if self.reraise_exceptions:
                    raise ex  # Re-raise

    cpdef void handle_event(self, Event event) except *:
        """
        System method. Hand the given event.

        :param event: The event received.
        """
        Condition.not_none(event, "event")

        if isinstance(event, OrderRejected):
            self.log.warning(f"{RECV}{EVT} {event}.")
            if self.flatten_on_sl_reject:
                self._flatten_on_sl_reject(event)
        elif isinstance(event, OrderCancelReject):
            self.log.warning(f"{RECV}{EVT} {event}.")
        else:
            self.log.info(f"{RECV}{EVT} {event}.")

        if self.is_running:
            try:
                self.on_event(event)
            except Exception as ex:
                self.log.exception(ex)
                if self.reraise_exceptions:
                    raise ex  # Re-raise


# -- DATA METHODS ----------------------------------------------------------------------------------

    cpdef list instrument_symbols(self):
        """
        Return a list of all instrument symbols held by the data client.

        :return List[Instrument].
        :raises ValueError: If the strategy has not been registered with a data client.
        """
        Condition.not_none(self._data, "data")

        return self._data.instrument_symbols()

    cpdef void get_quote_ticks(self, Symbol symbol) except *:
        """
        Request the historical quote ticks for the given parameters from the data service.

        :param symbol: The tick symbol for the request.
        """
        Condition.not_none(symbol, "symbol")

        if self._data is None:
            self.log.error("Cannot request quote ticks (data client not registered).")
            return

        self._data.request_quote_ticks(
            symbol=symbol,
            from_datetime=None,
            to_datetime=None,
            limit=self._data.tick_capacity,
            callback=self.handle_quote_ticks)

    cpdef void get_trade_ticks(self, Symbol symbol) except *:
        """
        Request the historical trade ticks for the given parameters from the data service.

        :param symbol: The tick symbol for the request.
        """
        Condition.not_none(symbol, "symbol")

        if self._data is None:
            self.log.error("Cannot request trade ticks (data client not registered).")
            return

        self._data.request_trade_ticks(
            symbol=symbol,
            from_datetime=None,
            to_datetime=None,
            limit=self._data.tick_capacity,
            callback=self.handle_trade_ticks)

    cpdef void get_bars(self, BarType bar_type) except *:
        """
        Request the historical bars for the given parameters from the data service.

        :param bar_type: The bar type for the request.
        """
        Condition.not_none(bar_type, "bar_type")

        if self._data is None:
            self.log.error("Cannot request bars (data client not registered).")
            return

        self._data.request_bars(
            bar_type=bar_type,
            from_datetime=None,
            to_datetime=None,
            limit=self._data.bar_capacity,
            callback=self.handle_bars)

    cpdef Instrument get_instrument(self, Symbol symbol):
        """
        Return the instrument corresponding to the given symbol (if found).

        :param symbol: The symbol of the instrument to return.
        :return Instrument or None.
        :raises ValueError: If the strategy has not been registered with a data client.
        """
        if self._data is None:
            self.log.error("Cannot get instrument (data client not registered).")
            return

        return self._data.get_instrument(symbol)

    cpdef dict get_instruments(self):
        """
        Return a dictionary of all instruments for the given venue (if found).

        :return Dict[Symbol, Instrument].
        """
        if self._data is None:
            self.log.error("Cannot get instruments (data client not registered).")
            return

        return self._data.get_instruments()

    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Subscribe to <QuoteTick> data for the given symbol.

        :param symbol: The tick symbol to subscribe to.
        """
        Condition.not_none(symbol, "symbol")

        if self._data is None:
            self.log.error(f"Cannot subscribe to {symbol} <QuoteTick> data "
                           "(data client not registered).")
            return

        self._data.subscribe_quote_ticks(symbol, self.handle_quote_tick)
        self.log.info(f"Subscribed to {symbol} <QuoteTick> data.")

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Subscribe to <TradeTick> data for the given symbol.

        :param symbol: The tick symbol to subscribe to.
        """
        Condition.not_none(symbol, "symbol")

        if self._data is None:
            self.log.error(f"Cannot subscribe to {symbol} <TradeTick> data "
                           "(data client not registered).")
            return

        self._data.subscribe_trade_ticks(symbol, self.handle_trade_tick)
        self.log.info(f"Subscribed to {symbol} <TradeTick> data.")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to <Bar> data for the given bar type.

        :param bar_type: The bar type to subscribe to.
        """
        Condition.not_none(bar_type, "bar_type")

        if self._data is None:
            self.log.error(f"Cannot subscribe to {bar_type} <Bar> data "
                           f"(data client not registered).")
            return

        self._data.subscribe_bars(bar_type, self.handle_bar)
        self.log.info(f"Subscribed to {bar_type} <Bar> data.")

    cpdef void subscribe_instrument(self, Symbol symbol) except *:
        """
        Subscribe to <Instrument> data for the given symbol.

        :param symbol: The instrument symbol to subscribe to.
        """
        Condition.not_none(symbol, "symbol")

        if self._data is None:
            self.log.error(f"Cannot subscribe to {symbol} <Instrument> data "
                           f"(data client not registered).")
            return

        self._data.subscribe_instrument(symbol, self.handle_data)
        self.log.info(f"Subscribed to {symbol} <Instrument> data.")

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from <QuoteTick> data for the given symbol.

        :param symbol: The tick symbol to unsubscribe from.
        """
        Condition.not_none(symbol, "symbol")

        if self._data is None:
            self.log.error(f"Cannot unsubscribe from {symbol} <QuoteTick> data "
                           "(data client not registered).")
            return

        self._data.unsubscribe_quote_ticks(symbol, self.handle_quote_tick)
        self.log.info(f"Unsubscribed from {symbol} <QuoteTick> data.")

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from <TradeTick> data for the given symbol.

        :param symbol: The tick symbol to unsubscribe from.
        """
        Condition.not_none(symbol, "symbol")

        if self._data is None:
            self.log.error(f"Cannot unsubscribe from {symbol} <TradeTick> data "
                           "(data client not registered).")
            return

        self._data.unsubscribe_trade_ticks(symbol, self.handle_trade_tick)
        self.log.info(f"Unsubscribed from {symbol} <TradeTick> data.")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from <Bar> data for the given bar type.

        :param bar_type: The bar type to unsubscribe from.
        """
        Condition.not_none(bar_type, "bar_type")

        if self._data is None:
            self.log.error(f"Cannot unsubscribe from {bar_type} <Bar> data "
                           f"(data client not registered).")
            return

        self._data.unsubscribe_bars(bar_type, self.handle_bar)
        self.log.info(f"Unsubscribed from {bar_type} <Bar> data.")

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        """
        Unsubscribe from instrument data for the given symbol.

        :param symbol: The instrument symbol to unsubscribe from.
        """
        Condition.not_none(symbol, "symbol")

        if self._data is None:
            self.log.error(f"Cannot unsubscribe from {symbol} <Instrument> data "
                           f"(data client not registered).")
            return

        self._data.unsubscribe_instrument(symbol, self.handle_data)
        self.log.info(f"Unsubscribed from {symbol} <Instrument> data.")

    cpdef bint has_quote_ticks(self, Symbol symbol):
        """
        Return a value indicating whether the strategy has quote ticks for the given symbol.

        :param symbol: The symbol for the ticks.
        :return bool.
        :raises ValueError: If the data client is not registered.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data, "data client")

        return self._data.has_quote_ticks(symbol)

    cpdef bint has_trade_ticks(self, Symbol symbol):
        """
        Return a value indicating whether the strategy has trade ticks for the given symbol.

        :param symbol: The symbol for the ticks.
        :return bool.
        :raises ValueError: If the data client is not registered.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data, "data client")

        return self._data.has_trade_ticks(symbol)

    cpdef bint has_bars(self, BarType bar_type):
        """
        Return a value indicating whether the strategy has bars for the given bar type.

        :param bar_type: The bar type for the bars.
        :return bool.
        :raises ValueError: If the data client is not registered.
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data, "data client")

        return self._data.has_bars(bar_type)

    cpdef int quote_tick_count(self, Symbol symbol):
        """
        Return the count of quote ticks held by the strategy for the given symbol.

        :param symbol: The symbol for the ticks.
        :return int.
        :raises ValueError: If the data client is not registered.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data, "data client")

        return self._data.quote_tick_count(symbol)

    cpdef int trade_tick_count(self, Symbol symbol):
        """
        Return the count of trade ticks held by the strategy for the given symbol.

        :param symbol: The symbol for the ticks.
        :return int.
        :raises ValueError: If the data client is not registered.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data, "data client")

        return self._data.trade_tick_count(symbol)

    cpdef int bar_count(self, BarType bar_type):
        """
        Return the count of bars held by the strategy for the given bar type.

        :param bar_type: The bar type to count.
        :return int.
        :raises ValueError: If the data client is not registered.
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data, "data client")

        return self._data.bar_count(bar_type)

    cpdef list quote_ticks(self, Symbol symbol):
        """
        Return the quote ticks for the given symbol (returns a copy of the internal deque).

        :param symbol: The symbol for the ticks to get.
        :return List[QuoteTick].
        :raises ValueError: If the data client is not registered.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data, "data client")

        return self._data.quote_ticks(symbol)

    cpdef list trade_ticks(self, Symbol symbol):
        """
        Return the trade ticks for the given symbol (returns a copy of the internal deque).

        :param symbol: The symbol for the ticks to get.
        :return List[TradeTick].
        :raises ValueError: If the data client is not registered.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data, "data client")

        return self._data.trade_ticks(symbol)

    cpdef list bars(self, BarType bar_type):
        """
        Return the bars for the given bar type (returns a copy of the internal deque).

        :param bar_type: The bar type to get.
        :return List[Bar].
        :raises ValueError: If the data client is not registered.
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data, "data client")

        return self._data.bars(bar_type)

    cpdef QuoteTick quote_tick(self, Symbol symbol, int index=0):
        """
        Return the quote tick for the given symbol at the given index or last if no index specified.

        :param symbol: The symbol for the tick to get.
        :param index: The optional index for the tick to get.
        :return QuoteTick.
        :raises ValueError: If the data client is not registered.
        :raises IndexError: If the tick index is out of range.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data, "data client")

        return self._data.quote_tick(symbol, index)

    cpdef TradeTick trade_tick(self, Symbol symbol, int index=0):
        """
        Return the trade tick for the given symbol at the given index or last if no index specified.

        :param symbol: The symbol for the tick to get.
        :param index: The optional index for the tick to get.
        :return TradeTick.
        :raises ValueError: If the data client is not registered.
        :raises IndexError: If the tick index is out of range.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data, "data client")

        return self._data.trade_tick(symbol, index)

    cpdef Bar bar(self, BarType bar_type, int index=0):
        """
        Return the bar for the given bar type at the given index or last if no index specified.

        :param bar_type: The bar type to get.
        :param index: The optional index for the bar to get.
        :return Bar.
        :raises ValueError: If the data client is not registered.
        :raises IndexError: If the bar index is out of range.
        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data, "data client")

        return self._data.bar(bar_type, index)


# -- INDICATOR METHODS -----------------------------------------------------------------------------

    cpdef list registered_indicators(self):
        """
        Return the registered indicators for the strategy (returns copy).

        :return List[Indicator].
        """
        return self._indicators.copy()

    cpdef bint indicators_initialized(self):
        """
        Return a value indicating whether all indicators are initialized.

        :return bool.
        """
        cdef int i
        for i in range(len(self._indicators)):
            if self._indicators[i].initialized is False:
                return False
        return True


# -- MANAGEMENT METHODS ----------------------------------------------------------------------------

    cpdef Account account(self):
        """
        Return the account for the strategy.

        :return: Account.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.account

    cpdef Portfolio portfolio(self):
        """
        Return the portfolio for the strategy.

        :return: Portfolio.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.portfolio

    cpdef OrderSide get_opposite_side(self, OrderSide side):
        """
        Return the opposite order side from the given side.

        :param side: The original order side.
        :return OrderSide.
        """
        return OrderSide.BUY if side == OrderSide.SELL else OrderSide.SELL

    cpdef OrderSide get_flatten_side(self, MarketPosition market_position):
        """
        Return the order side needed to flatten a position from the given market position.

        :param market_position: The market position to flatten.
        :return OrderSide.
        :raises ValueError: If market_position is FLAT.
        """
        if market_position == MarketPosition.LONG:
            return OrderSide.SELL
        elif market_position == MarketPosition.SHORT:
            return OrderSide.BUY
        else:
            raise ValueError("Cannot flatten a FLAT position.")

    cpdef double get_exchange_rate(
            self,
            Currency from_currency,
            Currency to_currency,
            PriceType price_type=PriceType.MID):
        """
        Return the calculated exchange rate for the given currencies.

        :param from_currency: The currency to convert from.
        :param to_currency: The currency to convert to.
        :param price_type: The price type for the exchange rate (default=MID).
        :return float.
        :raises ValueError: If the data client is not registered.
        :raises ValueError: If price_type is UNDEFINED or LAST.
        """
        Condition.not_none(self._data, "data client")

        return self._data.get_exchange_rate(
            from_currency=from_currency,
            to_currency=to_currency,
            price_type=price_type)

    cpdef double get_exchange_rate_for_account(
            self,
            Currency quote_currency,
            PriceType price_type=PriceType.MID):
        """
        Return the calculated exchange rate for the give trading instrument quote
        currency to the account currency.

        :param quote_currency: The quote currency to convert from.
        :param price_type: The price type for the exchange rate (default=MID).
        :return float.
        :raises ValueError: If the data client is not registered.
        :raises ValueError: If price_type is UNDEFINED or LAST.
        """
        Condition.not_none(self._data, "data client")

        cdef Account account = self.account()
        if account is None:
            self.log.error("Cannot get exchange rate (account is not initialized).")
            return 0.0

        return self._data.get_exchange_rate(
            from_currency=quote_currency,
            to_currency=self.account().currency,
            price_type=price_type)

    cpdef Order order(self, OrderId order_id):
        """
        Return the order with the given identifier (if found).

        :param order_id: The order_id.
        :return Order or None.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.get_order(order_id)

    cpdef dict orders(self):
        """
        Return a all orders associated with this strategy.

        :return Dict[OrderId, Order].
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.get_orders(self.id)

    cpdef dict orders_working(self):
        """
        Return all working orders associated with this strategy.

        :return Dict[OrderId, Order].
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.get_orders_working(self.id)

    cpdef dict orders_completed(self):
        """
        Return all completed orders associated with this strategy.

        :return Dict[OrderId, Order].
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.get_orders_completed(self.id)

    cpdef Position position(self, PositionId position_id):
        """
        Return the position associated with the given position_id (if found).

        :param position_id: The positions identifier.
        :return Position or None.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.get_position(position_id)

    cpdef Position position_for_order(self, OrderId order_id):
        """
        Return the position associated with the given order_id (if found).

        :param order_id: The order_id.
        :return Position or None.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.get_position_for_order(order_id)

    cpdef dict positions(self):
        """
        Return a dictionary of all positions associated with this strategy.

        :return Dict[PositionId, Position].
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.get_positions(self.id)

    cpdef dict positions_open(self):
        """
        Return a dictionary of all active positions associated with this strategy.

        :return Dict[PositionId, Position].
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.get_positions_open(self.id)

    cpdef dict positions_closed(self):
        """
        Return a dictionary of all closed positions associated with this strategy.

        :return Dict[PositionId, Position].
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.get_positions_closed(self.id)

    cpdef bint position_exists(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists.

        :param position_id: The position_id.
        :return bool.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.position_exists(position_id)

    cpdef bint order_exists(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier exists.

        :param order_id: The order_id.
        :return bool.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.order_exists(order_id)

    cpdef bint is_order_working(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is working.

        :param order_id: The order_id.
        :return bool.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.is_order_working(order_id)

    cpdef bint is_order_completed(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is complete.

        :param order_id: The order_id.
        :return bool.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.is_order_completed(order_id)

    cpdef bint is_position_open(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier is open.

        :param position_id: The position_id.
        :return bool.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.is_position_open(position_id)

    cpdef bint is_position_closed(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier is closed.

        :param position_id: The position_id.
        :return bool.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.is_position_closed(position_id)

    cpdef bint is_flat(self):
        """
        Return a value indicating whether the strategy is completely flat (i.e no market positions
        other than FLAT across all instruments).

        :return bool.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.is_strategy_flat(self.id)

    cpdef int count_orders_working(self):
        """
        Return the count of working orders held by the execution database.

        :return int.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.count_orders_working(self.id)

    cpdef int count_orders_completed(self):
        """
        Return the count of completed orders held by the execution database.

        :return int.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.count_orders_completed(self.id)

    cpdef int count_orders_total(self):
        """
        Return the total count of orders held by the execution database.

        :return int.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.count_orders_total(self.id)

    cpdef int count_positions_open(self):
        """
        Return the count of open positions held by the execution database.

        :return int.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.count_positions_open(self.id)

    cpdef int count_positions_closed(self):
        """
        Return the count of closed positions held by the execution database.

        :return int.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.count_positions_closed(self.id)

    cpdef int count_positions_total(self):
        """
        Return the total count of positions held by the execution database.

        :return int.
        :raises ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec, "exec")

        return self._exec.database.count_positions_total(self.id)


# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void start(self) except *:
        """
        Start the trade strategy and call on_start().
        """
        self.log.debug(f"Starting...")

        if self._data is None:
            self.log.error("Cannot start strategy (the data client is not registered).")
            return

        if self._exec is None:
            self.log.error("Cannot start strategy (the execution engine is not registered).")
            return

        try:
            self.on_start()
        except Exception as ex:
            self.log.exception(ex)

        self.is_running = True
        self.log.info(f"Running...")

    cpdef void stop(self) except *:
        """
        Stop the trade strategy and call on_stop().
        """
        self.log.debug(f"Stopping...")

        # Clean up clock
        cdef list timer_names = self.clock.get_timer_names()
        self.clock.cancel_all_timers()

        cdef str name
        for name in timer_names:
            self.log.info(f"Cancelled Timer(name={name}).")

        # Flatten open positions
        if self.flatten_on_stop:
            if not self.is_flat():
                self.flatten_all_positions()

        # Cancel working orders
        if self.cancel_all_orders_on_stop:
            self.cancel_all_orders("STOPPING STRATEGY")

        try:
            self.on_stop()
        except Exception as ex:
            self.log.exception(ex)

        self.is_running = False
        self.log.info(f"Stopped.")

    cpdef void reset(self) except *:
        """
        Reset the strategy.

        All stateful values are reset to their initial value, on_reset() is then
        called.
        Note: The strategy cannot be running otherwise an error is logged.
        """
        if self.is_running:
            self.log.error(f"Cannot reset (cannot reset a running strategy).")
            return

        self.log.debug(f"Resetting...")

        if self.order_factory is not None:
            self.order_factory.reset()
        if self.position_id_generator is not None:
            self.position_id_generator.reset()

        self._indicators.clear()
        self._indicator_updaters.clear()

        for indicator in self._indicators:
            indicator.reset()

        try:
            self.on_reset()
        except Exception as ex:
            self.log.exception(ex)

        self.log.info(f"Reset.")

    cpdef dict save(self):
        """
        Return the strategy state dictionary to be saved.
        """
        cpdef dict state = {
            "OrderIdCount": self.order_factory.count(),
            "PositionIdCount": self.position_id_generator.count
        }

        try:
            user_state = self.on_save()
        except Exception as ex:
            self.log.exception(ex)

        return {**state, **user_state}

    cpdef void load(self, dict state) except *:
        """
        Load the strategy state from the give state dictionary.

        :param state: The state dictionary to load.
        """
        Condition.not_empty(state, "state")

        order_id_count = state.get(b'OrderIdCount')
        if order_id_count:
            order_id_count = int(order_id_count.decode("utf8"))
            self.order_factory.set_count(order_id_count)
            self.log.info(f"Setting OrderIdGenerator count to {order_id_count}.")

        position_id_count = state.get(b'PositionIdCount')
        if position_id_count:
            position_id_count = int(position_id_count.decode("utf8"))
            self.position_id_generator.set_count(position_id_count)
            self.log.info(f"Setting PositionIdGenerator count to {position_id_count}.")

        try:
            self.on_load(state)
        except Exception as ex:
            self.log.exception(ex)

    cpdef void dispose(self) except *:
        """
        Dispose of the strategy to release system resources.
        """
        self.log.debug(f"Disposing...")

        try:
            self.on_dispose()
        except Exception as ex:
            self.log.exception(ex)

        self.log.info(f"Disposed.")

    cpdef void account_inquiry(self) except *:
        """
        Send an account inquiry command to the execution service.
        """
        if self._exec is None:
            self.log.error("Cannot send command AccountInquiry (execution engine not registered).")
            return

        cdef AccountInquiry command = AccountInquiry(
            self.trader_id,
            self._exec.account_id,
            self.uuid_factory.generate(),
            self.clock.time_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec.execute_command(command)

    cpdef void submit_order(self, Order order, PositionId position_id) except *:
        """
        Send a submit order command with the given order and position_id to the execution
        service.

        :param order: The order to submit.
        :param position_id: The position_id to associate with this order.
        """
        Condition.not_none(order, "order")
        Condition.not_none(position_id, "position_id")

        if self.trader_id is None:
            self.log.error("Cannot send command SubmitOrder (strategy not registered with a trader).")
            return

        if self._exec is None:
            self.log.error("Cannot send command SubmitOrder (execution engine not registered).")
            return

        cdef SubmitOrder command = SubmitOrder(
            self.trader_id,
            self._exec.account_id,
            self.id,
            position_id,
            order,
            self.uuid_factory.generate(),
            self.clock.time_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec.execute_command(command)

    cpdef void submit_bracket_order(self, BracketOrder bracket_order, PositionId position_id) except *:
        """
        Send a submit bracket order command with the given order and position_id to the
        execution service.

        :param bracket_order: The bracket order to submit.
        :param position_id: The position_id to associate with this order.
        """
        Condition.not_none(bracket_order, "bracket_order")
        Condition.not_none(position_id, "position_id")

        if self.trader_id is None:
            self.log.error("Cannot send command SubmitBracketOrder (strategy not registered with a trader).")
            return

        if self._exec is None:
            self.log.error("Cannot send command SubmitBracketOrder (execution engine not registered).")
            return

        cdef SubmitBracketOrder command = SubmitBracketOrder(
            self.trader_id,
            self._exec.account_id,
            self.id,
            position_id,
            bracket_order,
            self.uuid_factory.generate(),
            self.clock.time_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec.execute_command(command)

    cpdef void modify_order(self, Order order, Quantity new_quantity=None, Price new_price=None) except *:
        """
        Send a modify order command for the given order with the given new price
        to the execution service.

        :param order: The order to modify.
        :param new_quantity: The new quantity for the given order.
        :param new_price: The new price for the given order.
        """
        Condition.not_none(order, "order")

        if self.trader_id is None:
            self.log.error("Cannot send command ModifyOrder (strategy not registered with a trader).")
            return

        if self._exec is None:
            self.log.error("Cannot send command ModifyOrder (execution engine not registered).")
            return

        if new_quantity is None and new_price is None:
            self.log.error("Cannot send command ModifyOrder (both new_quantity and new_price were None).")
            return

        if new_quantity is None:
            new_quantity = order.quantity

        if new_price is None:
            new_price = order.price

        cdef ModifyOrder command = ModifyOrder(
            self.trader_id,
            self._exec.account_id,
            order.id,
            new_quantity,
            new_price,
            self.uuid_factory.generate(),
            self.clock.time_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec.execute_command(command)

    cpdef void cancel_order(self, Order order, str cancel_reason="NONE") except *:
        """
        Send a cancel order command for the given order and cancel_reason to the
        execution service.

        :param order: The order to cancel.
        :param cancel_reason: The optional reason for cancellation (default='NONE').
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the cancel_reason is not a valid string.
        """
        Condition.not_none(order, "order")
        Condition.valid_string(cancel_reason, "cancel_reason")

        if self.trader_id is None:
            self.log.error("Cannot send command CancelOrder (strategy not registered with a trader).")
            return

        if self._exec is None:
            self.log.error("Cannot send command CancelOrder (execution client not registered).")
            return

        cdef CancelOrder command = CancelOrder(
            self.trader_id,
            self._exec.account_id,
            order.id,
            ValidString(cancel_reason),
            self.uuid_factory.generate(),
            self.clock.time_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec.execute_command(command)

    cpdef void cancel_all_orders(self, str cancel_reason="CANCEL_ALL_ORDERS") except *:
        """
        Send a cancel order command for orders which are not completed in the
        order book with the given cancel_reason - to the execution engine.

        :param cancel_reason: The optional reason for cancellation (default='CANCEL_ALL_ORDERS').
        :raises ValueError: If the cancel_reason is not a valid string.
        """
        Condition.not_none(cancel_reason, "reason")  # Can be empty string

        if self._exec is None:
            self.log.error("Cannot execute cancel_all_orders(), execution client not registered.")
            return

        cdef dict working_orders = self._exec.database.get_orders_working(self.id)
        cdef int working_orders_count = len(working_orders)
        if working_orders_count == 0:
            self.log.info("cancel_all_orders(): No working orders to cancel.")
            return

        self.log.info(f"cancel_all_orders(): Cancelling {working_orders_count} working order(s)...")
        cdef OrderId order_id
        cdef Order order
        cdef CancelOrder command
        for order_id, order in working_orders.items():
            command = CancelOrder(
                self.trader_id,
                self._exec.account_id,
                order_id,
                ValidString(cancel_reason),
                self.uuid_factory.generate(),
                self.clock.time_now())

            self.log.info(f"{CMD}{SENT} {command}.")
            self._exec.execute_command(command)

    cpdef void flatten_position(self, PositionId position_id, str order_label="FLATTEN") except *:
        """
        Flatten the position corresponding to the given identifier by generating
        the required market order, and sending it to the execution service.
        If the position is None or already FLAT will log a warning.

        :param position_id: The position_id to flatten.
        :param order_label: The order label for the flattening order.
        :raises ValueError: If the position_id is not found in the position book.
        """
        Condition.not_none(position_id, "position_id")
        Condition.valid_string(order_label, "order_label")

        if self._exec is None:
            self.log.error("Cannot flatten position (execution client not registered).")
            return

        cdef Position position = self._exec.database.get_position(position_id)
        if position is None:
            self.log.error(f"Cannot flatten position (cannot find {position_id} in cached positions.")
            return

        if position.is_closed:
            self.log.warning(f"Cannot flatten position (the position {position_id} was already closed).")
            return

        cdef Order order = self.order_factory.market(
            position.symbol,
            self.get_flatten_side(position.market_position),
            position.quantity,
            Label(order_label),
            OrderPurpose.EXIT)

        self.log.info(f"Flattening {position}...")
        self.submit_order(order, position_id)

    cpdef void flatten_all_positions(self, str order_label="FLATTEN") except *:
        """
        Flatten all positions by generating the required market orders and sending
        them to the execution service. If no positions found or a position is None
        then will log a warning.

        :param order_label: The order label for the flattening order(s).
        """
        Condition.valid_string(order_label, "order_label")

        if self._exec is None:
            self.log.error("Cannot flatten all positions (execution client not registered).")
            return

        cdef dict positions = self._exec.database.get_positions_open(self.id)
        cdef int open_positions_count = len(positions)
        if open_positions_count == 0:
            self.log.info("FLATTEN_ALL_POSITIONS: No open positions to flatten.")
            return

        self.log.info(f"FLATTEN_ALL_POSITIONS: Flattening {open_positions_count} open position(s)...")

        cdef PositionId position_id
        cdef Position position
        cdef Order order
        for position_id, position in positions.items():
            if position.is_closed:
                self.log.warning(f"Cannot flatten position (the position {position_id} was already FLAT.")
                continue

            order = self.order_factory.market(
                position.symbol,
                self.get_flatten_side(position.market_position),
                position.quantity,
                Label(order_label),
                OrderPurpose.EXIT)

            self.log.info(f"Flattening {position}...")
            self.submit_order(order, position_id)

    cdef void _flatten_on_sl_reject(self, OrderRejected event) except *:
        cdef Order order = self._exec.database.get_order(event.order_id)
        cdef PositionId position_id = self._exec.database.get_position_id(event.order_id)

        if order is None:
            self.log.error(f"Cannot find {event.order_id} in cached orders.")
            return

        if position_id is None:
            self.log.error(f"Cannot find PositionId for {event.order_id}.")
            return

        if order.purpose == OrderPurpose.STOP_LOSS:
            if self._exec.database.is_position_open(position_id):
                self.log.error(f"Rejected {event.order_id} was a stop-loss, now flattening {position_id}.")
                self.flatten_position(position_id)


# -- BACKTEST METHODS ------------------------------------------------------------------------------

    cpdef void change_clock(self, Clock clock) except *:
        """
        Backtest only method. Change the strategies clock with the given clock.

        :param clock: The clock to change to.
        """
        Condition.not_none(clock, "clock")
        Condition.not_none(self.trader_id, "trader_id")

        self.clock = clock
        self.clock.register_default_handler(self.handle_event)

        self.order_factory = OrderFactory(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=clock,
            uuid_factory=self.uuid_factory,
            initial_count=self.order_factory.count())

        self.position_id_generator = PositionIdGenerator(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=clock,
            initial_count=self.position_id_generator.count)

    cpdef void change_uuid_factory(self, UUIDFactory uuid_factory) except *:
        """
        Backtest only method. Change the strategies UUID factory with the given UUID factory.

        :param uuid_factory: The UUID factory to change to.
        """
        Condition.not_none(uuid_factory, "uuid_factory")

        self.uuid_factory = uuid_factory

    cpdef void change_logger(self, Logger logger) except *:
        """
        Backtest only method. Change the strategies logger with the given logger.

        :param logger: The logger to change to.
        """
        Condition.not_none(logger, "logger")

        self.log = LoggerAdapter(self.id.value, logger)
