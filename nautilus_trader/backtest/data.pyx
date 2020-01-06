# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import pytz

from cpython.datetime cimport datetime
from typing import Set, List, Dict, Callable
from pandas import DatetimeIndex

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure, bar_structure_to_string
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.objects cimport Instrument, Tick, BarType, Bar, BarSpecification
from nautilus_trader.model.identifiers cimport Symbol, Venue
from nautilus_trader.model.events cimport TimeEvent
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.guid cimport TestGuidFactory
from nautilus_trader.common.logger cimport Logger
from nautilus_trader.common.data cimport DataClient, BarAggregator, TickBarAggregator, TimeBarAggregator
from nautilus_trader.data.market cimport TickDataWrangler, BarDataWrangler


cdef class BacktestDataContainer:
    """
    Provides a container for backtest data.
    """

    def __init__(self):
        """
        Initializes a new instance of the BacktestDataContainer class.
        """
        self.instruments = {}
        self.ticks = {}
        self.bars_bid = {}
        self.bars_ask = {}

    cpdef void add_instrument(self, Instrument instrument):
        self.instruments[instrument.symbol] = instrument
        self.instruments = dict(sorted(self.instruments.items()))

    cpdef void add_ticks(self, Symbol symbol, data: pd.DataFrame):
        self.ticks[symbol] = data
        self.ticks = dict(sorted(self.ticks.items()))

    cpdef void add_bars(self, Symbol symbol, BarStructure structure, PriceType price_type, data: pd.DataFrame):
        Condition.true(price_type != PriceType.LAST, 'price_type != PriceType.LAST')

        if price_type == PriceType.BID:
            if symbol not in self.bars_bid:
                self.bars_bid[symbol] = {}
                self.bars_bid = dict(sorted(self.bars_bid.items()))
            self.bars_bid[symbol][structure] = data
            self.bars_bid[symbol] = dict(sorted(self.bars_bid[symbol].items()))

        if price_type == PriceType.ASK:
            if symbol not in self.bars_ask:
                self.bars_ask[symbol] = {}
                self.bars_ask = dict(sorted(self.bars_ask.items()))
            self.bars_ask[symbol][structure] = data
            self.bars_bid[symbol] = dict(sorted(self.bars_ask[symbol].items()))

    cpdef void check_integrity(self):
        """
        Check the integrity of the data inside the container.
        
        :raises: AssertionFailed: If the any integrity check fails.
        """
        # Check there is the needed instrument for each data symbol
        cdef set data_symbols = {symbol for symbol in self.ticks}  # type: Set[Symbol]
        [data_symbols.add(symbol) for symbol in self.bars_bid]
        [data_symbols.add(symbol) for symbol in self.bars_ask]

        for symbol in data_symbols:
            assert(symbol in self.instruments, f'The needed instrument {symbol} was not provided.')

        # Check that all bar DataFrames for each symbol are of the same shape and index
        cdef dict shapes = {}  # type: Dict[BarStructure, tuple]
        cdef dict indexs = {}  # type: Dict[BarStructure, DatetimeIndex]
        for symbol, data in self.bars_bid.items():
            for structure, dataframe in data.items():
                if structure not in shapes:
                    shapes[structure] = dataframe.shape
                if structure not in indexs:
                    indexs[structure] = dataframe.index
                assert(dataframe.shape == shapes[structure], f'{dataframe} shape is not equal.')
                assert(dataframe.index == indexs[structure], f'{dataframe} index is not equal.')
        for symbol, data in self.bars_ask.items():
            for structure, dataframe in data.items():
                assert(dataframe.shape == shapes[structure], f'{dataframe} shape is not equal.')
                assert(dataframe.index == indexs[structure], f'{dataframe} index is not equal.')


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for backtesting.
    """

    def __init__(self,
                 Venue venue,
                 BacktestDataContainer data,
                 TestClock clock,
                 Logger logger):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param venue: The venue for the data client.
        :param data: The data needed for the backtest.
        :param clock: The clock for the component.
        :param logger: The logger for the component.
        :raises ConditionFailed: If the instruments list contains a type other than Instrument.
        :raises ConditionFailed: If the data_ticks dict contains a key type other than Symbol.
        :raises ConditionFailed: If the data_ticks dict contains a value type other than DataFrame.
        :raises ConditionFailed: If the data_bars_bid dict contains a key type other than Symbol.
        :raises ConditionFailed: If the data_bars_bid dict contains a value type other than DataFrame.
        :raises ConditionFailed: If the data_bars_ask dict contains a key type other than Symbol.
        :raises ConditionFailed: If the data_bars_ask dict contains a value type other than DataFrame.
        :raises ConditionFailed: If the data_bars_bid keys does not equal the data_bars_ask keys.
        :raises ConditionFailed: If the clock is None.
        :raises ConditionFailed: If the logger is None.
        """
        Condition.not_none(clock, 'clock')
        Condition.not_none(logger, 'logger')

        super().__init__(venue, clock, TestGuidFactory(), logger)

        # Check data integrity
        data.check_integrity()

        # Update instruments dictionary
        for instrument in data.instruments.values():
            self._handle_instrument(instrument)

        # Prepare data
        self.data_providers = {}  # type: Dict[Symbol, DataProvider]
        self._log.info("Preparing data...")
        for symbol, instrument in self._instruments.items():
            self._log.debug(f'Creating DataProvider for {symbol}...')
            start = datetime.utcnow()
            self._log.info(f"Building {symbol} ticks...")
            self.data_providers[symbol] = DataProvider(
                instrument=instrument,
                ticks=None if symbol not in data.ticks else data.ticks[symbol],
                bars_bid=data.bars_bid[symbol],
                bars_ask=data.bars_ask[symbol])
            self._log.info(f"Built {len(self.data_providers[symbol].ticks)} {symbol} ticks in {round((datetime.utcnow() - start).total_seconds(), 2)}s.")

        cdef list ticks = []
        self.execution_resolutions = []
        for symbol, provider in self.data_providers.items():
            ticks += provider.ticks
            self.execution_resolutions.append(f'{symbol.to_string()}={bar_structure_to_string(provider.execution_resolution)}')

        self.ticks = sorted(ticks)
        self.min_timestamp = ticks[0].timestamp
        self.max_timestamp = ticks[-1].timestamp

    cpdef void connect(self):
        """
        Connect to the data service.
        """
        self._log.info("Connected.")

    cpdef void disconnect(self):
        """
        Disconnect from the data service.
        """
        self._log.info("Disconnected.")

    cpdef void reset(self):
        """
        Reset the client to its initial state.
        """
        self._log.info(f"Resetting...")
        self._reset()
        self._log.info("Reset.")

    cpdef void dispose(self):
        """
        Dispose of the data client by releasing all resources.
        """
        pass

    cpdef void process_tick(self, Tick tick):
        """
        Process the given tick with the data client.
        
        :param tick: The tick to process.
        """
        self._handle_tick(tick)

        if self._clock.has_timers and tick.timestamp < self._clock.next_event_time:
            return  # No events to handle yet

        self._clock.advance_time(tick.timestamp)

        cdef TimeEvent event
        for event, handler in self._clock.get_pending_events().items():
            handler(event)

    cpdef void request_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,
            datetime to_datetime,
            callback: Callable):
        """
        Request the historical bars for the given parameters from the data service.

        :param symbol: The symbol for the bars to download.
        :param from_datetime: The datetime from which the historical bars should be downloaded.
        :param to_datetime: The datetime to which the historical bars should be downloaded.
        :param callback: The callback for the response.
        """
        Condition.type(callback, Callable, 'callback')

        self._log.info(f"Simulated request ticks for {symbol} from {from_datetime} to {to_datetime}.")

    cpdef void request_bars(
            self,
            BarType bar_type,
            datetime from_datetime,
            datetime to_datetime,
            callback: Callable):
        """
        Request the historical bars for the given parameters from the data service.

        :param bar_type: The bar type for the bars to download.
        :param from_datetime: The datetime from which the historical bars should be downloaded.
        :param to_datetime: The datetime to which the historical bars should be downloaded.
        :param callback: The callback for the response.
        """
        Condition.type(callback, Callable, 'callback')

        self._log.info(f"Simulated request bars for {bar_type} from {from_datetime} to {to_datetime}.")

    cpdef void request_instrument(self, Symbol symbol, callback: Callable):
        """
        Request the instrument for the given symbol.

        :param symbol: The symbol to update.
        :param callback: The callback for the response.
        """
        Condition.type(callback, Callable, 'callback')

        self._log.info(f"Requesting instrument for {symbol}...")

        callback(self._instruments[symbol])

    cpdef void request_instruments(self, callback: Callable):
        """
        Request all instrument for the data clients venue.
        """
        Condition.type(callback, Callable, 'callback')

        self._log.info(f"Requesting all instruments for the {self.venue} ...")

        callback([instrument for instrument in self._instruments.values()])

    cpdef void subscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Subscribe to tick data for the given symbol.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ConditionFailed: If the symbol is not a key in data_providers.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.is_in(symbol, self.data_providers, 'symbol', 'data_providers')
        Condition.type_or_none(handler, Callable, 'handler')

        self._add_tick_handler(symbol, handler)

    cpdef void subscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Subscribe to live bar data for the given bar parameters.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ConditionFailed: If the symbol is not a key in data_providers.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.is_in(bar_type.symbol, self.data_providers, 'symbol', 'data_providers')
        Condition.type_or_none(handler, Callable, 'handler')

        self._self_generate_bars(bar_type, handler)

    cpdef void subscribe_instrument(self, Symbol symbol, handler: Callable):
        """
        Subscribe to live instrument data updates for the given symbol and handler.

        :param symbol: The instrument symbol to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._log.info(f"Simulated subscribe to {symbol} instrument updates "
                       f"(a backtest data client wont update an instrument).")

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Unsubscribes from tick data for the given symbol.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ConditionFailed: If the symbol is not a key in data_providers.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.is_in(symbol, self.data_providers, 'symbol', 'data_providers')
        Condition.type_or_none(handler, Callable, 'handler')

        self._remove_tick_handler(symbol, handler)

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Unsubscribes from bar data for the given symbol and venue.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ConditionFailed: If the symbol is not a key in data_providers.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.is_in(bar_type.symbol, self.data_providers, 'symbol', 'data_providers')
        Condition.type_or_none(handler, Callable, 'handler')

        self._remove_bar_handler(bar_type, handler)

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: Callable):
        """
        Unsubscribe from live instrument data updates for the given symbol and handler.

        :param symbol: The instrument symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._log.info(f"Simulated unsubscribe from {symbol} instrument updates "
                       f"(a backtest data client will not update an instrument).")

    cpdef void update_instruments(self):
        """
        Update all instruments from the database.
        """
        self._log.info(f"Simulated update all instruments for the {self.venue} venue "
                       f"(a backtest data client already has all instruments needed).")


cdef class DataProvider:
    """
    Provides data for a particular instrument for the BacktestDataClient.
    """

    def __init__(self,
                 Instrument instrument,
                 ticks: pd.DataFrame,
                 dict bars_bid: Dict[BarStructure, pd.DataFrame],
                 dict bars_ask: Dict[BarStructure, pd.DataFrame]):
        """
        Initializes a new instance of the DataProvider class.

        :param instrument: The instrument for the data provider.
        :param ticks: The tick data for the data provider.
        :param bars_bid: The bid bars data for the data provider.
        :param bars_ask: The ask bars data for the data provider.
        :raises ConditionFailed: If the data_ticks is a type other than None or DataFrame.
        :raises ConditionFailed: If the data_bars_bid is None.
        :raises ConditionFailed: If the data_bars_ask is None.
        """
        Condition.type_or_none(ticks, pd.DataFrame, 'data_ticks')
        Condition.type_or_none(bars_bid, Dict, 'data_bars_bid')
        Condition.type_or_none(bars_ask, Dict, 'data_bars_ask')

        self.instrument = instrument

        # Determine highest tick resolution
        if ticks is not None and len(ticks) > 0:
            self.execution_resolution = BarStructure.TICK
            bid_data = None
            ask_data = None
        elif BarStructure.SECOND in bars_bid:
            bid_data = bars_bid[BarStructure.SECOND]
            ask_data = bars_ask[BarStructure.SECOND]
            self.execution_resolution = BarStructure.SECOND
        elif BarStructure.MINUTE in bars_bid:
            bid_data = bars_bid[BarStructure.MINUTE]
            ask_data = bars_ask[BarStructure.MINUTE]
            self.execution_resolution = BarStructure.MINUTE
        elif BarStructure.HOUR in bars_bid:
            bid_data = bars_bid[BarStructure.HOUR]
            ask_data = bars_ask[BarStructure.HOUR]
            self.execution_resolution = BarStructure.HOUR
        elif BarStructure.DAY in bars_bid:
            bid_data = bars_bid[BarStructure.DAY]
            ask_data = bars_ask[BarStructure.DAY]
            self.execution_resolution = BarStructure.DAY
        else:
            bid_data = pd.DataFrame()
            ask_data = pd.DataFrame()

        cdef TickDataWrangler builder = TickDataWrangler(
            symbol=self.instrument.symbol,
            precision=self.instrument.tick_precision,
            tick_data=ticks,
            bid_data=bid_data,
            ask_data=ask_data)

        self.ticks = builder.build_ticks_all()  # type: List[Tick]

        # Check tick data timestamp integrity (UTC timezone)
        assert(self.ticks[0].timestamp.tz == pytz.UTC)
