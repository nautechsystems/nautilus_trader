# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import pytz

from cpython.datetime cimport datetime, timedelta
from pandas import DataFrame
from typing import Set, List, Dict, Callable

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.quote_type cimport QuoteType
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure, bar_structure_to_string
from nautilus_trader.model.objects cimport Instrument, Tick, BarType, Bar, BarSpecification
from nautilus_trader.model.identifiers cimport Symbol, Venue
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.guid cimport TestGuidFactory
from nautilus_trader.common.logger cimport Logger
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.data.tools cimport TickBuilder, BarBuilder


cdef class BidAskBarPair:
    """
    Represents a bid ask bar pair for the same market and timestamp.
    """

    def __init__(self, Bar bid_bar, Bar ask_bar):
        """
         Initializes a new instance of the BidAskBarPair class.

        :param bid_bar: The bid bar for the pair.
        :param ask_bar: The ask bar for the pair.
        """
        self.bid = bid_bar
        self.ask = ask_bar


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for backtesting.
    """

    def __init__(self,
                 Venue venue,
                 list instruments: List[Instrument],
                 dict data_ticks: Dict[Symbol, DataFrame],
                 dict data_bars_bid: Dict[Symbol, Dict[BarStructure, DataFrame]],
                 dict data_bars_ask: Dict[Symbol, Dict[BarStructure, DataFrame]],
                 TestClock clock,
                 Logger logger):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param venue: The venue for the data client.
        :param instruments: The instruments needed for the backtest.
        :param data_ticks: The historical tick data needed for the backtest.
        :param data_bars_bid: The historical bid bar data needed for the backtest.
        :param data_bars_ask: The historical ask bar data needed for the backtest.
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
        Condition.list_type(instruments, Instrument, 'instruments')
        Condition.dict_types(data_ticks, Symbol, DataFrame, 'dataframes_ticks')
        Condition.dict_types(data_bars_bid, Symbol, dict, 'dataframes_bars_bid')
        Condition.dict_types(data_bars_ask, Symbol, dict, 'dataframes_bars_ask')
        Condition.true(data_bars_bid.keys() == data_bars_ask.keys(), 'dataframes_bars_bid.keys() == dataframes_bars_ask.keys()')
        Condition.not_none(clock, 'clock')
        Condition.not_none(logger, 'logger')

        super().__init__(venue, clock, TestGuidFactory(), logger)
        self.data_ticks = data_ticks                      # type: Dict[Symbol, DataFrame]
        self.data_bars_bid = data_bars_bid                # type: Dict[Symbol, Dict[BarStructure, DataFrame]]
        self.data_bars_ask = data_bars_ask                # type: Dict[Symbol, Dict[BarStructure, DataFrame]]
        self.data_providers = {}                          # type: Dict[Symbol, DataProvider]
        self.data_symbols = set()                         # type: Set[Symbol]
        self.execution_data_index_min = None              # Set below
        self.execution_data_index_max = None              # Set below
        self.execution_structure = BarStructure.UNKNOWN   # Set below
        self.max_time_step = timedelta(0)                 # Set below

        self._log.info("Preparing data...")

        # Update instruments dictionary
        for instrument in instruments:
            self._handle_instrument(instrument)

        # Create data symbols set
        cdef set tick_data_symbols = {symbol for symbol in self.data_ticks}    # type: Set[Symbol]
        cdef set bid_data_symbols = {symbol for symbol in self.data_bars_bid}  # type: Set[Symbol]
        cdef set ask_data_symbols = {symbol for symbol in self.data_bars_ask}  # type: Set[Symbol]
        assert(bid_data_symbols == ask_data_symbols)
        self.data_symbols = tick_data_symbols.union(bid_data_symbols.union(ask_data_symbols))

        # Check there is the needed instrument for each data symbol
        for key in self._instruments.keys():
            assert(key in self.data_symbols, f'The needed instrument {key} was not provided.')

        # Check that all bar structure DataFrames are of the same shape and index
        cdef dict shapes = {}  # type: Dict[BarStructure, tuple]
        cdef dict indexs = {}  # type: Dict[BarStructure, datetime]
        for symbol, data in data_bars_bid.items():
            for structure, dataframe in data.items():
                if structure not in shapes:
                    shapes[structure] = dataframe.shape
                if structure not in indexs:
                    indexs[structure] = dataframe.index
                assert(dataframe.shape == shapes[structure], f'{dataframe} shape is not equal.')
                assert(dataframe.index == indexs[structure], f'{dataframe} index is not equal.')
        for symbol, data in data_bars_ask.items():
            for structure, dataframe in data.items():
                assert(dataframe.shape == shapes[structure], f'{dataframe} shape is not equal.')
                assert(dataframe.index == indexs[structure], f'{dataframe} index is not equal.')

        # Create the data providers for the client based on the given instruments
        for symbol, instrument in self._instruments.items():
            self._log.info(f'Creating DataProvider for {symbol}...')
            self.data_providers[symbol] = DataProvider(
                instrument=instrument,
                data_ticks=None if symbol not in self.data_ticks else self.data_ticks[symbol],
                data_bars_bid=self.data_bars_bid[symbol],
                data_bars_ask=self.data_bars_ask[symbol])

            # Build ticks
            start = datetime.utcnow()
            self._log.info(f"Building {symbol} ticks...")
            self.data_providers[symbol].register_ticks()
            self._log.info(f"Built {len(self.data_providers[symbol].ticks)} {symbol} ticks in {round((datetime.utcnow() - start).total_seconds(), 2)}s.")

            # Check tick data timestamp integrity (UTC timezone)
            ticks = self.data_providers[symbol].ticks
            assert(ticks[0].timestamp.tz == pytz.UTC)

        self._setup_execution_data()

    cdef void _setup_execution_data(self):
        # Check if necessary data for TICK bar structure
        if self._check_ticks_exist():
            self.execution_structure = BarStructure.TICK
            self.max_time_step = timedelta(seconds=1)

        # Check if necessary data for SECOND bar structure
        if self.execution_structure == BarStructure.UNKNOWN and self._check_bar_resolution_exists(BarStructure.SECOND):
            self.execution_structure = BarStructure.SECOND
            self.max_time_step = timedelta(seconds=1)

        # Check if necessary data for MINUTE bar structure
        if self.execution_structure == BarStructure.UNKNOWN and self._check_bar_resolution_exists(BarStructure.MINUTE):
            self.execution_structure = BarStructure.MINUTE
            self.max_time_step = timedelta(minutes=1)

        # Check if necessary data for HOUR bar structure
        if self.execution_structure == BarStructure.UNKNOWN and self._check_bar_resolution_exists(BarStructure.HOUR):
            self.execution_structure = BarStructure.HOUR
            self.max_time_step = timedelta(hours=1)

        if self.execution_structure == BarStructure.UNKNOWN:
            raise RuntimeError('Insufficient data for ANY execution bar structure')

        # Setup the execution data based on the given structure
        if self.execution_structure == BarStructure.TICK:
            for symbol in self.data_symbols:
                # Set execution timestamp indexs
                ticks = self.data_providers[symbol].ticks
                first_timestamp = ticks[0].timestamp
                last_timestamp = ticks[len(ticks) - 1].timestamp
                self._set_execution_data_index(symbol, first_timestamp, last_timestamp)
        else:
            # Build bars required for execution
            for data_provider in self.data_providers.values():
                data_provider.set_execution_bar_res(self.execution_structure)
                self._build_bars(data_provider.bar_type_execution_bid)
                self._build_bars(data_provider.bar_type_execution_ask)
                 # Check bars data integrity
                exec_bid_bars = data_provider.bars[data_provider.bar_type_execution_bid]
                exec_ask_bars = data_provider.bars[data_provider.bar_type_execution_ask]
                assert(len(exec_bid_bars) == len(exec_ask_bars))
                assert(exec_bid_bars[0].timestamp.tz == exec_ask_bars[0].timestamp.tz)
                assert(exec_bid_bars[0].timestamp.tz == pytz.UTC)  # Check data is UTC timezone
                # Set execution timestamp indexs
                first_timestamp = exec_bid_bars[0].timestamp
                last_timestamp = exec_bid_bars[len(exec_bid_bars) - 1].timestamp
                self._set_execution_data_index(data_provider.instrument.symbol, first_timestamp, last_timestamp)

        self._log.info(f"Execution bar structure = {bar_structure_to_string(self.execution_structure)}")
        self._log.info(f"Iteration maximum time-step = {self.max_time_step}")

    cdef bint _check_ticks_exist(self):
        # Check if the tick data contains ticks for all data symbols
        for symbol in self.data_symbols:
            if symbol not in self.data_ticks or len(self.data_ticks[symbol]) == 0:
                return False
        return True

    cdef bint _check_bar_resolution_exists(self, BarStructure structure):
        # Check if the bar data contains the given bar structure and is not empty
        for symbol in self.data_symbols:
            if structure not in self.data_bars_bid[symbol] or len(self.data_bars_bid[symbol][structure]) == 0:
                return False
            if structure not in self.data_bars_ask[symbol] or len(self.data_bars_ask[symbol][structure]) == 0:
               return False
        return True

    cdef void _set_execution_data_index(self, Symbol symbol, datetime first, datetime last):
        # Set minimum execution data timestamp
        if self.execution_data_index_min is None or self.execution_data_index_min < first:
            self.execution_data_index_min = first

        # Set maximum execution data timestamp
        if self.execution_data_index_max is None or self.execution_data_index_max > last:
            self.execution_data_index_max = last

    cdef void _build_bars(self, BarType bar_type):
        Condition.is_in(bar_type.symbol, self.data_providers, 'symbol', 'data_providers')

        # Build bars of the given bar type inside the data provider
        cdef datetime start = datetime.utcnow()
        self._log.info(f"Building {bar_type} bars...")
        self.data_providers[bar_type.symbol].register_bars(bar_type)
        self._log.info(f"Built {len(self.data_providers[bar_type.symbol].bars[bar_type])} {bar_type} bars in {round((datetime.utcnow() - start).total_seconds(), 2)}s.")

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
        cdef Symbol symbol
        cdef DataProvider data_provider
        for symbol, data_provider in self.data_providers.items():
            data_provider.reset()
            self._log.debug(f"Reset data provider for {symbol}.")

        self._log.info("Reset.")

    cpdef void dispose(self):
        """
        Dispose of the data client by releasing all resources.
        """
        pass

    cpdef void set_initial_iteration_indexes(self, datetime to_time):
        """
        Set the initial tick and bar iteration indexes for each data provider
        to the given to_time.
        
        :param to_time: The datetime to set the iteration indexes to.
        """
        for data_provider in self.data_providers.values():
            data_provider.set_initial_iteration_indexes(to_time)
        self._clock.set_time(to_time)

    cpdef list iterate_ticks(self, datetime to_time):
        """
        Return the iterated ticks up to the given time.
        
        :param to_time: The datetime to iterate to.
        :return List[Tick].
        """
        cdef list ticks = []  # type: List[Tick]
        cdef DataProvider data_provider
        for data_provider in self.data_providers.values():
            ticks += data_provider.iterate_ticks(to_time)
        ticks.sort()
        return ticks

    cpdef dict iterate_bars(self, datetime to_time):
        """
        Return the iterated bars up to the given time.

        :param to_time: The datetime to iterate to.
        :return Dict[BarType, Bar].
        """
        cdef dict bars = {}  # type: Dict[BarType, List[Bar]]
        cdef DataProvider data_provider
        cdef BarType bar_type
        cdef Bar bar
        for data_provider in self.data_providers.values():
            for bar_type, bar in data_provider.iterate_bars(to_time).items():
                bars[bar_type] = bar
        return bars

    cpdef dict get_next_execution_bars(self, datetime time):
        """
        Return a dictionary of the next bid and ask minute bars if they exist 
        at the given time for each symbol.

        Note: Values are a tuple of the bid bar [0], then the ask bar [1].
        :param time: The index time for the minute bars.
        :return Dict[Symbol, BidAskBarPair].
        """
        cdef dict minute_bars = {}  # type: Dict[Symbol, BidAskBarPair]
        cdef Symbol symbol
        cdef DataProvider data_provider
        for symbol, data_provider in self.data_providers.items():
            if data_provider.is_next_exec_bars_at_time(time):
                minute_bars[symbol] = BidAskBarPair(
                    bid_bar=data_provider.get_next_exec_bid_bar(),
                    ask_bar=data_provider.get_next_exec_ask_bar())
        return minute_bars

    cpdef void process_tick(self, Tick tick):
        """
        Iterate the data client one time step.
        
        :param tick: The tick to process.
        """
        self._handle_tick(tick)

    cpdef void process_bars(self, dict bars):
        """
        Iterate the data client one time step.
        
        :param bars: The dictionary of bars to process Dict[BarType, Bar].
        """
        # Iterate bars
        cdef BarType bar_type
        cdef Bar bar
        for bar_type, bar in bars.items():
            self._handle_bar(bar_type, bar)

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
        self.data_providers[symbol].set_tick_iteration_index(self.time_now())

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

        cdef start = datetime.utcnow()
        self._add_bar_handler(bar_type, handler)
        if bar_type not in self.data_providers[bar_type.symbol].bars:
            self._build_bars(bar_type)
        self.data_providers[bar_type.symbol].set_bar_iteration_index(bar_type, self.time_now())

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

        self.data_providers[symbol].deregister_ticks()
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

        self.data_providers[bar_type.symbol].deregister_bars(bar_type)
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
                 data_ticks: DataFrame,
                 dict data_bars_bid: Dict[BarStructure, DataFrame],
                 dict data_bars_ask: Dict[BarStructure, DataFrame]):
        """
        Initializes a new instance of the DataProvider class.

        :param instrument: The instrument for the data provider.
        :param data_ticks: The tick data for the data provider.
        :raises ConditionFailed: If the data_ticks is a type other than None or DataFrame.
        :raises ConditionFailed: If the data_bars_bid is None.
        :raises ConditionFailed: If the data_bars_ask is None.
        """
        Condition.type_or_none(data_ticks, DataFrame, 'data_ticks')
        Condition.not_none(data_bars_bid, 'data_bars_bid')
        Condition.not_none(data_bars_ask, 'data_bars_ask')

        self.instrument = instrument
        self._dataframe_ticks = data_ticks
        self._dataframes_bars_bid = data_bars_bid  # type: Dict[BarStructure, DataFrame]
        self._dataframes_bars_ask = data_bars_ask  # type: Dict[BarStructure, DataFrame]
        self.bar_type_sec_bid = BarType(self.instrument.symbol, BarSpecification(1, BarStructure.SECOND, QuoteType.BID))
        self.bar_type_sec_ask = BarType(self.instrument.symbol, BarSpecification(1, BarStructure.SECOND, QuoteType.ASK))
        self.bar_type_min_bid = BarType(self.instrument.symbol, BarSpecification(1, BarStructure.MINUTE, QuoteType.BID))
        self.bar_type_min_ask = BarType(self.instrument.symbol, BarSpecification(1, BarStructure.MINUTE, QuoteType.ASK))
        self.bar_type_hour_bid = BarType(self.instrument.symbol, BarSpecification(1, BarStructure.HOUR, QuoteType.BID))
        self.bar_type_hour_ask = BarType(self.instrument.symbol, BarSpecification(1, BarStructure.HOUR, QuoteType.ASK))
        self.bar_type_execution_bid = None
        self.bar_type_execution_ask = None
        self.ticks = []                            # type: List[Tick]
        self.bars = {}                             # type: Dict[BarType, List[Bar]]
        self.iterations = {}                       # type: Dict[BarType, int]
        self.tick_index = 0

    cpdef void register_ticks(self):
        """
        Register ticks for the data provider.
        """
        if BarStructure.SECOND in self._dataframes_bars_bid:
            bid_data = self._dataframes_bars_bid[BarStructure.SECOND]
            ask_data = self._dataframes_bars_ask[BarStructure.SECOND]
        elif BarStructure.MINUTE in self._dataframes_bars_bid:
            bid_data = self._dataframes_bars_bid[BarStructure.MINUTE]
            ask_data = self._dataframes_bars_ask[BarStructure.MINUTE]
        elif BarStructure.HOUR in self._dataframes_bars_bid:
            bid_data = self._dataframes_bars_bid[BarStructure.HOUR]
            ask_data = self._dataframes_bars_ask[BarStructure.HOUR]
        else:
            bid_data = pd.DataFrame()
            ask_data = pd.DataFrame()

        cdef TickBuilder builder = TickBuilder(symbol=self.instrument.symbol,
                                               decimal_precision=self.instrument.tick_precision,
                                               tick_data=self._dataframe_ticks,
                                               bid_data=bid_data,
                                               ask_data=ask_data)
        self.ticks = builder.build_ticks_all()

    cpdef void deregister_ticks(self):
        """
        Deregister ticks with the data provider.
        """
        self.ticks = []

    cpdef void register_bars(self, BarType bar_type):
        """
        Register the given bar type with the data provider.
        
        :param bar_type: The bar type to register.
        """
        Condition.true(bar_type.symbol == self.instrument.symbol, 'bar_type.symbol == self.instrument.symbol')

        # TODO: Add capability for re-sampled bars

        if bar_type not in self.bars:
            if bar_type.specification.quote_type is QuoteType.BID:
                data = self._dataframes_bars_bid[bar_type.specification.structure]
                tick_precision = self.instrument.tick_precision
            elif bar_type.specification.quote_type is QuoteType.ASK:
                data = self._dataframes_bars_ask[bar_type.specification.structure]
                tick_precision = self.instrument.tick_precision
            elif bar_type.specification.quote_type is QuoteType.MID:
                data = (self._dataframes_bars_bid[bar_type.specification.structure] + self._dataframes_bars_ask[bar_type.specification.structure]) / 2
                tick_precision = self.instrument.tick_precision + 1
            elif bar_type.specification.quote_type is QuoteType.LAST:
                raise NotImplemented('QuoteType.LAST not supported for bar type.')

            builder = BarBuilder(decimal_precision=tick_precision, data=data)
            self.bars[bar_type] = builder.build_bars_all()

        if bar_type not in self.iterations:
            self.iterations[bar_type] = 0

    cpdef void deregister_bars(self, BarType bar_type):
        """
        Deregister the given bar type with the data provider.
        
        :param bar_type: The bar type to deregister.
        """
        Condition.true(bar_type.symbol == self.instrument.symbol, 'bar_type.symbol == self.instrument.symbol')

        if bar_type in self.iterations:
            del self.iterations[bar_type]

    cpdef void set_execution_bar_res(self, BarStructure structure):
        """
        Set the execution bar type based on the given structure.
        
        :param structure: The structure.
        """
        if structure == BarStructure.SECOND:
            self.bar_type_execution_bid = self.bar_type_sec_bid
            self.bar_type_execution_ask = self.bar_type_sec_ask
        elif structure == BarStructure.MINUTE:
            self.bar_type_execution_bid = self.bar_type_min_bid
            self.bar_type_execution_ask = self.bar_type_min_ask
        elif structure == BarStructure.HOUR:
            self.bar_type_execution_bid = self.bar_type_hour_bid
            self.bar_type_execution_ask = self.bar_type_hour_ask
        else:
            raise ValueError(f'cannot set execution bar structure to {bar_structure_to_string(structure)}')

    cpdef void set_initial_iteration_indexes(self, datetime to_time):
        """
        Set the initial bar iterations based on the given to_time.
        
        :param to_time: The time to iterate to.
        """
        self.set_tick_iteration_index(to_time)

        for bar_type in self.iterations:
            self.set_bar_iteration_index(bar_type, to_time)

    cpdef void set_tick_iteration_index(self, datetime to_time):
        """
        Set the iteration tick index to the given to_time.

        :param to_time: The time to iterate to.
        """
        while self.ticks[self.tick_index].timestamp < to_time:
            if self.tick_index + 1 < len(self.ticks):
                self.tick_index += 1
            else:
                break # No more ticks to iterate

    cpdef void set_bar_iteration_index(self, BarType bar_type, datetime to_time):
        """
        Set the iteration index for the given bar type to the given to_time.
        
        :param bar_type: The bar type to iterate.
        :param to_time: The time to iterate to.
        """
        while self.bars[bar_type][self.iterations[bar_type]].timestamp < to_time:
                if  self.iterations[bar_type] + 1 < len(self.bars[bar_type]):
                    self.iterations[bar_type] += 1
                else:
                    break # No more bars to iterate

    cpdef bint is_next_exec_bars_at_time(self, datetime time):
        """
        Return a value indicating whether the timestamp of the next execution bars equals the given time.

        :param time: The reference time for next execution bars.
        :return bool.
        """
        return self.bars[self.bar_type_execution_bid][self.iterations[self.bar_type_execution_bid]].timestamp == time

    cpdef Bar get_next_exec_bid_bar(self):
        """
        Return the next execution bid bar.
        
        :return Bar.
        """
        return self.bars[self.bar_type_execution_bid][self.iterations[self.bar_type_execution_bid]]

    cpdef Bar get_next_exec_ask_bar(self):
        """
        Return the next execution ask bar.
        
        :return Bar.
        """
        return self.bars[self.bar_type_execution_ask][self.iterations[self.bar_type_execution_ask]]

    cpdef list iterate_ticks(self, datetime to_time):
        """
        Return a list of ticks which have been generated based on the given to datetime.
        
        :param to_time: The time to build the tick list to.
        :return List[Tick].
        """
        cdef list ticks_list = []  # type: List[Tick]
        if self.tick_index < len(self.ticks):
            while self.ticks[self.tick_index].timestamp <= to_time:
                ticks_list.append(self.ticks[self.tick_index])
                if self.tick_index + 1 < len(self.ticks):
                    self.tick_index += 1
                else:
                    self.tick_index += 1
                    break # No more ticks to append

        return ticks_list

    cpdef dict iterate_bars(self, datetime to_time):
        """
        Return a list of bars which have closed based on the given to datetime.

        :param to_time: The time to build the bar list to.
        :return Dict[BarType, Bar].
        """
        cdef dict bars_dict = {}  # type: Dict[BarType, Bar]
        for bar_type, iterations in self.iterations.items():
            if self.bars[bar_type][iterations].timestamp <= to_time:
                bars_dict[bar_type] = self.bars[bar_type][iterations]
                self.iterations[bar_type] += 1

        return bars_dict

    cpdef void reset(self):
        """
        Reset the data provider by returning all stateful values to their
        initial value, whilst preserving any constructed bar and tick data.
        """
        for bar_type in self.iterations.keys():
            self.iterations[bar_type] = 0

        self.tick_index = 0
