#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import pandas as pd

from cpython.datetime cimport datetime, timedelta
from pandas import DataFrame
from typing import Set, List, Dict, Callable

from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.quote_type cimport QuoteType
from inv_trader.enums.resolution cimport Resolution
from inv_trader.common.clock cimport TestClock
from inv_trader.common.logger cimport Logger
from inv_trader.common.data cimport DataClient
from inv_trader.model.objects cimport Symbol, Instrument, Tick, BarType, Bar, BarSpecification
from inv_trader.tools cimport TickBuilder, BarBuilder


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for backtesting.
    """

    def __init__(self,
                 list instruments: List[Instrument],
                 dict data_ticks: Dict[Symbol, DataFrame],
                 dict data_bars_bid: Dict[Symbol, Dict[Resolution, DataFrame]],
                 dict data_bars_ask: Dict[Symbol, Dict[Resolution, DataFrame]],
                 TestClock clock,
                 Logger logger):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param instruments: The instruments needed for the backtest.
        :param data_ticks: The historical tick data needed for the backtest.
        :param data_bars_bid: The historical bid bar data needed for the backtest.
        :param data_bars_ask: The historical ask bar data needed for the backtest.
        :param clock: The clock for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the instruments list contains a type other than Instrument.
        :raises ValueError: If the data_ticks dict contains a key type other than Symbol.
        :raises ValueError: If the data_ticks dict contains a value type other than DataFrame.
        :raises ValueError: If the data_bars_bid dict contains a key type other than Symbol.
        :raises ValueError: If the data_bars_bid dict contains a value type other than DataFrame.
        :raises ValueError: If the data_bars_ask dict contains a key type other than Symbol.
        :raises ValueError: If the data_bars_ask dict contains a value type other than DataFrame.
        :raises ValueError: If the data_bars_bid keys does not equal the data_bars_ask keys.
        :raises ValueError: If the clock is None.
        :raises ValueError: If the logger is None.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.dict_types(data_ticks, Symbol, DataFrame, 'dataframes_ticks')
        Precondition.dict_types(data_bars_bid, Symbol, dict, 'dataframes_bars_bid')
        Precondition.dict_types(data_bars_ask, Symbol, dict, 'dataframes_bars_ask')
        Precondition.true(data_bars_bid.keys() == data_bars_ask.keys(), 'dataframes_bars_bid.keys() == dataframes_bars_ask.keys()')
        Precondition.not_none(clock, 'clock')
        Precondition.not_none(logger, 'logger')

        super().__init__(clock, logger)
        self.data_ticks = data_ticks        # type: Dict[Symbol, DataFrame]
        self.data_bars_bid = data_bars_bid  # type: Dict[Symbol, Dict[Resolution, DataFrame]]
        self.data_bars_ask = data_bars_ask  # type: Dict[Symbol, Dict[Resolution, DataFrame]]

        if len(self.data_ticks) > 0:
            self.use_ticks = True

        # Set minute data index
        first_dataframe = data_bars_bid[next(iter(data_bars_bid))][Resolution.MINUTE]
        self.data_minute_index = list(pd.to_datetime(first_dataframe.index, utc=True))  # type: List[datetime]

        assert(isinstance(self.data_minute_index[0], datetime))

        self.data_providers = {}  # type: Dict[Symbol, DataProvider]

        # Convert instruments list to dictionary indexed by symbol
        cdef dict instruments_dict = {}  # type: Dict[Symbol, Instrument]
        for instrument in instruments:
            instruments_dict[instrument.symbol] = instrument
        self._instruments = instruments_dict

        # Create set of all data symbols
        cdef set bid_data_symbols = set()  # type: Set[Symbol]
        for symbol in data_bars_bid:
            bid_data_symbols.add(symbol)
        cdef set ask_data_symbols = set()  # type: Set[Symbol]
        for symbol in data_bars_ask:
            ask_data_symbols.add(symbol)
        assert(bid_data_symbols == ask_data_symbols)
        cdef set data_symbols = bid_data_symbols.union(ask_data_symbols)

        # Check there is the needed instrument for each data symbol
        for key in self._instruments.keys():
            assert(key in data_symbols, f'The needed instrument {key} was not provided.')

        # Check that all resolution DataFrames are of the same shape and index
        cdef dict shapes = {}  # type: Dict[Resolution, tuple]
        cdef dict indexs = {}  # type: Dict[Resolution, datetime]
        for symbol, data in data_bars_bid.items():
            for resolution, dataframe in data.items():
                if resolution not in shapes:
                    shapes[resolution] = dataframe.shape
                if resolution not in indexs:
                    indexs[resolution] = dataframe.index
                assert(dataframe.shape == shapes[resolution], f'{dataframe} shape is not equal.')
                assert(dataframe.index == indexs[resolution], f'{dataframe} index is not equal.')
        for symbol, data in data_bars_ask.items():
            for resolution, dataframe in data.items():
                assert(dataframe.shape == shapes[resolution], f'{dataframe} shape is not equal.')
                assert(dataframe.index == indexs[resolution], f'{dataframe} index is not equal.')

        # Create the data providers for the client based on the given instruments
        for symbol, instrument in self._instruments.items():
            self._log.debug(f'Creating data provider for {symbol}...')
            self.data_providers[symbol] = DataProvider(instrument=instrument,
                                                       data_ticks=None if symbol not in self.data_ticks else self.data_ticks[symbol].tz_localize('UTC'),
                                                       data_bars_bid=self.data_bars_bid[symbol],
                                                       data_bars_ask=self.data_bars_ask[symbol])

        # Prepare data for backtest engine and execution
        for symbol, data_provider in self.data_providers.items():
            start = datetime.utcnow()
            self._log.info(f"Building {symbol} ticks...")
            data_provider.register_ticks()
            self._log.info(f"Built {len(self.data_providers[symbol].ticks)} {symbol} ticks in {round((datetime.utcnow() - start).total_seconds(), 2)}s.")

        if not self.use_ticks:
            for data_provider in self.data_providers.values():
                data_provider.build_minute_bars()

    cpdef void set_initial_iteration(
            self,
            datetime to_time,
            timedelta time_step):
        """
        Set the initial internal iteration by winding the data client data 
        providers bar iterations and tick indexs forwards to the given to_time 
        at the given time_step.
        
        :param to_time: The time to wind the data client to.
        :param time_step: The time step to iterate at.
        """
        cdef datetime current = self.data_minute_index[0]
        cdef int next_index = 0

        while current < to_time:
            if self.data_minute_index[next_index] == current:
                next_index += 1
            current += time_step

        for symbol, data_provider in self.data_providers.items():
            data_provider.set_initial_iterations(self.data_minute_index[0], to_time, time_step)

        self._clock.set_time(current)

    cpdef list iterate_ticks(self, datetime to_time):
        """
        Return the iterated ticks up to the given time.
        
        :param to_time: The datetime to iterate to.
        :return: List[Tick].
        """
        cdef list ticks = []
        for data_provider in self.data_providers.values():
            if data_provider.has_ticks:
                ticks += data_provider.iterate_ticks(to_time)
        ticks.sort()
        return ticks

    cpdef dict iterate_bars(self, datetime to_time):
        """
        Return the iterated bars up to the given time.

        :param to_time: The datetime to iterate to.
        :return: Dict[BarType, Bar].
        """
        cdef dict bars = {}
        for data_provider in self.data_providers.values():
            for bar_type, bar in data_provider.iterate_bars(to_time).items():
                bars[bar_type] = bar
        return bars

    cpdef dict get_next_minute_bars(self, datetime time):
        """
        Return a dictionary of the next bid and ask minute bars if they exist 
        at the given time.

        Note: Values are a tuple of the bid bar [0], then the ask bar [1].
        :param time: The index time for the minute bars.
        :return: Dict[Symbol, (Bar, Bar)].
        """
        cdef dict minute_bars = {}
        for symbol, data_provider in self.data_providers.items():
            if data_provider.is_next_minute_bars_at_time(time):
                minute_bars[symbol] = (data_provider.get_next_minute_bid_bar(time), data_provider.get_next_minute_ask_bar(time))
        return minute_bars

    cpdef void process_tick(self, Tick tick):
        """
        Iterate the data client one time step.
        """
        if tick.symbol in self._tick_handlers:
            for handler in self._tick_handlers[tick.symbol]:
                handler(tick)

    cpdef void process_bars(self, dict bars):
        """
        Iterate the data client one time step.
        """
        # Iterate bars
        cdef BarType bar_type
        cdef Bar bar
        for bar_type, bar in bars.items():
            if bar_type in self._bar_handlers:
                for handler in self._bar_handlers[bar_type]:
                    handler(bar_type, bar)

    cpdef void reset(self):
        """
        Reset the data client by returning all stateful internal values to their
        initial values, whilst preserving any constructed bar and tick data.
        """
        self._log.info(f"Resetting...")

        for symbol, data_provider in self.data_providers.items():
            data_provider.reset()
            self._log.debug(f"Reset data provider for {symbol}.")

        self._log.info("Reset.")

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

    cpdef void update_all_instruments(self):
        """
        Update all instruments from the database.
        """
        self._log.info(f"Updated all instruments.")

    cpdef void update_instrument(self, Symbol symbol):
        """
        Update the instrument corresponding to the given symbol (if found).
        Will log a warning is symbol is not found.

        :param symbol: The symbol to update.
        """
        self._log.info(f"Updated instrument {symbol}.")

    cpdef void historical_bars(
            self,
            BarType bar_type,
            int quantity,
            handler: Callable):
        """
        Download the historical bars for the given parameters from the data
        service, then pass them to the callable bar handler.

        :param bar_type: The historical bar type to download.
        :param quantity: The number of historical bars to download (can be None, will download all).
        :param handler: The bar handler to pass the bars to.
        :raises ValueError: If the handler is not of type Callable.
        :raises ValueError: If the quantity is not None and not positive (> 0).
        """
        Precondition.type(handler, Callable, 'handler')

        if quantity is not None:
            Precondition.positive(quantity, 'quantity')

        self._log.info(f"Simulating download of {quantity} historical bars for {bar_type}.")

    cpdef void historical_bars_from(
            self,
            BarType bar_type,
            datetime from_datetime,
            handler: Callable):
        """
        Download the historical bars for the given parameters from the data
        service, then pass them to the callable bar handler.

        :param bar_type: The historical bar type to download.
        :param from_datetime: The datetime from which the historical bars should be downloaded.
        :param handler: The handler to pass the bars to.
        :raises ValueError: If the handler is not of type Callable.
        :raises ValueError: If the from_datetime is not less than that current datetime.
        """
        Precondition.type(handler, Callable, 'handler')
        Precondition.true(from_datetime < self._clock.time_now(), 'from_datetime < self._clock.time_now().')

        self._log.info(f"Simulating download of historical bars from {from_datetime} for {bar_type}.")

    cpdef void subscribe_ticks(self, Symbol symbol, handler: Callable=None):
        """
        Subscribe to tick data for the given symbol.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        :raises ValueError: If the symbol is not a key in data_providers.
        :raises ValueError: If the handler is not of type None or Callable.
        """
        Precondition.is_in(symbol, self.data_providers, 'symbol', 'data_providers')
        Precondition.type_or_none(handler, Callable, 'handler')

        self._subscribe_ticks(symbol, handler)

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: Callable=None):
        """
        Unsubscribes from tick data for the given symbol.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed (can be None).
        :raises ValueError: If the symbol is not a key in data_providers.
        :raises ValueError: If the handler is not of type None or Callable.
        """
        Precondition.is_in(symbol, self.data_providers, 'symbol', 'data_providers')
        Precondition.type_or_none(handler, Callable, 'handler')

        self.data_providers[symbol].deregister_ticks()
        self._unsubscribe_ticks(symbol, handler)

    cpdef void subscribe_bars(self, BarType bar_type, handler: Callable=None):
        """
        Subscribe to live bar data for the given bar parameters.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        :raises ValueError: If the symbol is not a key in data_providers.
        :raises ValueError: If the handler is not of type None or Callable.
        """
        Precondition.is_in(bar_type.symbol, self.data_providers, 'symbol', 'data_providers')
        Precondition.type_or_none(handler, Callable, 'handler')

        cdef start = datetime.utcnow()
        if bar_type not in self.data_providers[bar_type.symbol].bars:
            self._log.info(f"Building {bar_type} bars...")
            self.data_providers[bar_type.symbol].register_bars(bar_type)
            self._log.info(f"Built {len(self.data_providers[bar_type.symbol].bars[bar_type])} {bar_type} bars in {round((datetime.utcnow() - start).total_seconds(), 2)}s.")

        self._subscribe_bars(bar_type, handler)

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: Callable=None):
        """
        Unsubscribes from bar data for the given symbol and venue.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed (can be None).
        :raises ValueError: If the symbol is not a key in data_providers.
        :raises ValueError: If the handler is not of type None or Callable.
        """
        Precondition.is_in(bar_type.symbol, self.data_providers, 'symbol', 'data_providers')
        Precondition.type_or_none(handler, Callable, 'handler')

        self.data_providers[bar_type.symbol].deregister_bars(bar_type)
        self._unsubscribe_bars(bar_type, handler)


cdef class DataProvider:
    """
    Provides data for a particular instrument for the BacktestDataClient.
    """

    def __init__(self,
                 Instrument instrument,
                 data_ticks: DataFrame,
                 dict data_bars_bid: Dict[Resolution, DataFrame],
                 dict data_bars_ask: Dict[Resolution, DataFrame]):
        """
        Initializes a new instance of the DataProvider class.

        :param instrument: The instrument for the data provider.
        :param data_ticks: The tick data for the data provider.
        :param data_bars_bid: The bid bars data for the data provider (must contain MINUTE resolution).
        :param data_bars_ask: The ask bars data for the data provider (must contain MINUTE resolution).
        :raises ValueError: If the data_ticks is a type other than None or DataFrame.
        :raises ValueError: If the data_bars_bid is None.
        :raises ValueError: If the data_bars_ask is None.
        :raises ValueError: If the data_bars_bid does not contain the MINUTE resolution key.
        :raises ValueError: If the data_bars_ask does not contain the MINUTE resolution key.
        """
        Precondition.type_or_none(data_ticks, DataFrame, 'data_ticks')
        Precondition.not_none(data_bars_bid, 'data_bars_bid')
        Precondition.not_none(data_bars_ask, 'data_bars_ask')
        Precondition.is_in(Resolution.MINUTE, data_bars_bid, 'Resolution.MINUTE', 'data_bars_bid')
        Precondition.is_in(Resolution.MINUTE, data_bars_ask, 'Resolution.MINUTE', 'data_bars_ask')

        self.instrument = instrument
        self._dataframe_ticks = data_ticks
        self._dataframes_bars_bid = data_bars_bid  # type: Dict[Resolution, DataFrame]
        self._dataframes_bars_ask = data_bars_ask  # type: Dict[Resolution, DataFrame]
        self.bar_type_min_bid = BarType(self.instrument.symbol, BarSpecification(1, Resolution.MINUTE, QuoteType.BID))
        self.bar_type_min_ask = BarType(self.instrument.symbol, BarSpecification(1, Resolution.MINUTE, QuoteType.ASK))
        self.ticks = []                            # type: List[Tick]
        self.bars = {}                             # type: Dict[BarType, List[Bar]]
        self.iterations = {}                       # type: Dict[BarType, int]
        self.tick_index = 0
        self.has_ticks = False
        self.register_ticks()

    cpdef void register_ticks(self):
        """
        Register ticks for the data provider.
        """
        cdef TickBuilder builder = TickBuilder(symbol=self.instrument.symbol,
                                               decimal_precision=self.instrument.tick_precision,
                                               tick_data=self._dataframe_ticks,
                                               bid_data=self._dataframes_bars_bid[Resolution.MINUTE],
                                               ask_data=self._dataframes_bars_ask[Resolution.MINUTE])

        self.ticks = builder.build_ticks_all()
        self.has_ticks = len(self.ticks) > 0

    cpdef void deregister_ticks(self):
        """
        Deregister ticks with the data provider.
        """
        self.ticks = []
        self.has_ticks = False

    cpdef void build_minute_bars(self):
        """
        Build bars for 1-MINUTE BID and ASK.
        """
        self.bars[self.bar_type_min_bid] = BarBuilder(self.instrument.tick_precision, self._dataframes_bars_bid[Resolution.MINUTE]).build_bars_all()
        self.bars[self.bar_type_min_ask] = BarBuilder(self.instrument.tick_precision, self._dataframes_bars_ask[Resolution.MINUTE]).build_bars_all()

    cpdef void register_bars(self, BarType bar_type):
        """
        Register the given bar type with the data provider.
        
        :param bar_type: The bar type to register.
        """
        Precondition.true(bar_type.symbol == self.instrument.symbol, 'bar_type.symbol == self.instrument.symbol')

        # TODO: Add capability for re-sampled bars

        if bar_type not in self.bars:
            if bar_type.specification.quote_type is QuoteType.BID:
                data = self._dataframes_bars_bid[bar_type.specification.resolution]
                tick_precision = self.instrument.tick_precision
            elif bar_type.specification.quote_type is QuoteType.ASK:
                data = self._dataframes_bars_ask[bar_type.specification.resolution]
                tick_precision = self.instrument.tick_precision
            elif bar_type.specification.quote_type is QuoteType.MID:
                data = (self._dataframes_bars_bid[bar_type.specification.resolution] + self._dataframes_bars_ask[bar_type.specification.resolution]) / 2
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
        Precondition.true(bar_type.symbol == self.instrument.symbol, 'bar_type.symbol == self.instrument.symbol')

        if bar_type in self.iterations:
            del self.iterations[bar_type]

    cpdef void set_initial_iterations(
            self,
            datetime from_time,
            datetime to_time,
            timedelta time_step):
        """
        Set the initial bar iterations based on the given datetimes and time_step.
        """
        cdef datetime current = from_time

        while current < to_time:
            if self.has_ticks:
                 while self.ticks[self.tick_index].timestamp <= current:
                     if self.tick_index + 1 < len(self.ticks):
                         self.tick_index += 1
                     else:
                         break # No more ticks to iterate

            for bar_type, iterations in self.iterations.items():
                if self.bars[bar_type][iterations].timestamp == current:
                    self.iterations[bar_type] += 1
            current += time_step

    cpdef list iterate_ticks(self, datetime to_time):
        """
        Return a list of ticks which have been generated based on the given to datetime.
        
        :param to_time: The time to build the tick list to.
        :return: List[Tick].
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

    cpdef bint is_next_minute_bars_at_time(self, datetime time):
        return self.bars[self.bar_type_min_bid][self.iterations[self.bar_type_min_bid]].timestamp == time

    cpdef Bar get_next_minute_bid_bar(self, datetime time):
        """
        Return the 1-MINUTE[BID] bar if one exists at the given to_time.
        
        :return: Bar.
        """
        return self.bars[self.bar_type_min_bid][self.iterations[self.bar_type_min_bid]]

    cpdef Bar get_next_minute_ask_bar(self, datetime time):
        """
        Return the 1-MINUTE[ASK] bar if one exists at the given to_time.
        
        :return: Bar.
        """
        return self.bars[self.bar_type_min_ask][self.iterations[self.bar_type_min_ask]]

    cpdef dict iterate_bars(self, datetime to_time):
        """
        Return a list of bars which have closed based on the given to datetime.

        :param to_time: The time to build the bar list to.
        :return: List[Bar].
        """
        cdef dict bars_dict = {}  # type: Dict[BarType, Bar]

        for bar_type, iterations in self.iterations.items():
            if self.bars[bar_type][iterations].timestamp == to_time:
                bars_dict[bar_type] = self.bars[bar_type][iterations]
                self.iterations[bar_type] += 1

        return bars_dict

    cpdef void reset(self):
        """
        Reset the data provider by returning all stateful internal values to their
        initial values, whilst preserving any constructed bar and tick data.
        """
        for bar_type in self.iterations.keys():
            self.iterations[bar_type] = 0

        self.tick_index = 0
