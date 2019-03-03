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
from inv_trader.model.objects cimport Symbol, Instrument, Tick, BarType, Bar
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
        :param data_ticks: The historical ticks data needed for the backtest.
        :param data_bars_bid: The historical bid data needed for the backtest.
        :param data_bars_ask: The historical ask data needed for the backtest.
        :param clock: The clock for the component.
        :param logger: The logger for the component.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.dict_types(data_ticks, Symbol, DataFrame, 'dataframes_ticks')
        Precondition.dict_types(data_bars_bid, Symbol, dict, 'dataframes_bars_bid')
        Precondition.dict_types(data_bars_ask, Symbol, dict, 'dataframes_bars_ask')
        Precondition.true(data_bars_bid.keys() == data_bars_ask.keys(), 'dataframes_bars_bid.keys() == dataframes_bars_ask.keys()')
        Precondition.not_none(clock, 'clock')
        Precondition.not_none(logger, 'logger')

        super().__init__(clock, logger)
        self.data_ticks = data_ticks
        self.data_bars_bid = data_bars_bid  # type: Dict[Symbol, Dict[Resolution, DataFrame]]
        self.data_bars_ask = data_bars_ask  # type: Dict[Symbol, Dict[Resolution, DataFrame]]

        # Set minute data index
        first_dataframe = data_bars_bid[next(iter(data_bars_bid))][Resolution.MINUTE]
        self.data_minute_index = list(pd.to_datetime(first_dataframe.index, utc=True))  # type: List[datetime]

        assert(isinstance(self.data_minute_index[0], datetime))

        self.data_providers = {}  # type: Dict[Symbol, DataProvider]
        self.iteration = 0

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
        cdef set data_symbols = bid_data_symbols

        # Check there is the needed instrument for each data symbol
        for key in self._instruments.keys():
            assert(key in data_symbols, f'The needed instrument {key} was not provided')

        # Check that all resolution DataFrames are of the same shape and index
        cdef dict shapes = {}  # type: Dict[Resolution, tuple]
        cdef dict indexs = {}  # type: Dict[Resolution, datetime]
        for symbol, data in data_bars_bid.items():
            for resolution, dataframe in data.items():
                if resolution not in shapes:
                    shapes[resolution] = dataframe.shape
                if resolution not in indexs:
                    indexs[resolution] = dataframe.index
                assert(dataframe.shape == shapes[resolution], f'{dataframe} shape is not equal')
                assert(dataframe.index == indexs[resolution], f'{dataframe} index is not equal')

        for symbol, data in data_bars_ask.items():
            for resolution, dataframe in data.items():
                assert(dataframe.shape == shapes[resolution], f'{dataframe} shape is not equal')
                assert(dataframe.index == indexs[resolution], f'{dataframe} index is not equal')

    cpdef void create_data_providers(self):
        """
        Create the data providers for the client based on the given instruments.
        """
        for symbol, instrument in self._instruments.items():
            self._log.info(f'Creating data provider for {symbol}...')
            self.data_providers[symbol] = DataProvider(instrument=instrument,
                                                       data_bars_bid=self.data_bars_bid[symbol],
                                                       data_bars_ask=self.data_bars_ask[symbol])

    cpdef void set_initial_iteration(
            self,
            datetime to_time,
            timedelta time_step):
        """
        Wind the data client data providers bar iterations forwards to the given 
        to_time with the given time_step.
        
        :param to_time: The time to wind the data client to.
        :param time_step: The time step to iterate at.
        """
        cdef datetime current = self.data_minute_index[0]
        cdef int next_index = 0

        while current < to_time:
            if self.data_minute_index[next_index] == current:
                next_index += 1
                self.iteration += 1
            current += time_step

        for symbol, data_provider in self.data_providers.items():
            data_provider.set_initial_iterations(self.data_minute_index[0], to_time, time_step)

        self._clock.set_time(current)

    cpdef void iterate(self):
        """
        Iterate the data client one time step.
        """
        # Iterate ticks
        cdef list ticks = []
        for data_provider in self.data_providers.values():
            if data_provider.has_ticks:
                ticks = data_provider.iterate_ticks(self._clock.time_now())
                for tick in ticks:
                    if tick.symbol in self._tick_handlers:
                        for handler in self._tick_handlers[tick.symbol]:
                            handler(tick)

        # Iterate bars
        cdef list bars = []
        for data_provider in self.data_providers.values():
            bars = data_provider.iterate_bars(self._clock.time_now())
            for bar_type, bar in bars:
                if bar_type in self._bar_handlers:
                    for handler in self._bar_handlers[bar_type]:
                        handler(bar_type, bar)

        self.iteration += 1

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
        :raises ValueError: If the quantity is not None and not positive (> 0).
        """
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
        :raises ValueError: If the from_datetime is not less than datetime.utcnow().
        """
        self._log.info(f"Simulating download of historical bars from {from_datetime} for {bar_type}.")

    cpdef void subscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Subscribe to tick data for the given symbol and venue.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        """
        Precondition.is_in(symbol, self.data_providers, 'symbol', 'data_providers')

        cdef start = datetime.utcnow()
        self.data_providers[symbol].register_ticks()
        self._log.info(f"Built {len(self.data_providers[symbol].ticks)} {symbol} ticks in {round((datetime.utcnow() - start).total_seconds(), 2)}s.")
        self._subscribe_ticks(symbol, handler)

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Unsubscribes from tick data for the given symbol and venue.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed (can be None).
        """
        Precondition.is_in(symbol, self.data_providers, 'symbol', 'data_providers')

        self.data_providers[symbol].deregister_ticks()
        self._unsubscribe_ticks(symbol, handler)

    cpdef void subscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Subscribe to live bar data for the given bar parameters.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        """
        Precondition.is_in(bar_type.symbol, self.data_providers, 'symbol', 'data_providers')

        cdef start = datetime.utcnow()
        if bar_type not in self.data_providers[bar_type.symbol].bars:
            self.data_providers[bar_type.symbol].register_bars(bar_type)
            self._log.info(f"Built {len(self.data_providers[bar_type.symbol].bars[bar_type])} {bar_type} bars in {round((datetime.utcnow() - start).total_seconds(), 2)}s.")

        self._subscribe_bars(bar_type, handler)

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Unsubscribes from bar data for the given symbol and venue.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed (can be None).
        """
        Precondition.is_in(bar_type.symbol, self.data_providers, 'symbol', 'data_providers')

        self.data_providers[bar_type.symbol].deregister_bars(bar_type)
        self._unsubscribe_bars(bar_type, handler)


cdef class DataProvider:
    """
    Provides data for a particular instrument for the BacktestDataClient.
    """

    def __init__(self,
                 Instrument instrument,
                 dict data_bars_bid: Dict[Resolution, DataFrame],
                 dict data_bars_ask: Dict[Resolution, DataFrame]):
        """
        Initializes a new instance of the DataProvider class.

        :param instrument: The instrument for the data provider.
        :param data_bars_bid: The bid bars data for the data provider (must contain minute resolution).
        :param data_bars_ask: The ask bars data for the data provider (must contain minute resolution).
        """
        Precondition.is_in(Resolution.MINUTE, data_bars_bid, 'Resolution.MINUTE', 'data_bars_bid')
        Precondition.is_in(Resolution.MINUTE, data_bars_ask, 'Resolution.MINUTE', 'data_bars_ask')

        self.instrument = instrument
        self._dataframes_bars_bid = data_bars_bid  # type: Dict[Resolution, DataFrame]
        self._dataframes_bars_ask = data_bars_ask  # type: Dict[Resolution, DataFrame]
        self.ticks = []                            # type: List[Tick]
        self.bars = {}                             # type: Dict[BarType, List[Bar]]
        self.iterations = {}                       # type: Dict[BarType, int]
        self.tick_index = 0
        self.has_ticks = False

    cpdef void register_ticks(self):
        """
        Register ticks for the data provider.
        """
        cdef TickBuilder builder = TickBuilder(symbol=self.instrument.symbol,
                                               decimal_precision=self.instrument.tick_precision,
                                               bid_data=self._dataframes_bars_bid[Resolution.MINUTE],
                                               ask_data=self._dataframes_bars_ask[Resolution.MINUTE])

        self.ticks = builder.build_ticks_all()
        self.has_ticks = True

    cpdef void deregister_ticks(self):
        """
        Deregister ticks with the data provider.
        """
        self.ticks = []
        self.has_ticks = False

    cpdef void register_bars(self, BarType bar_type):
        """
        Register the given bar type with the data provider.
        
        :param bar_type: The bar type to register.
        """
        Precondition.true(bar_type.symbol == self.instrument.symbol, 'bar_type.symbol == self.instrument.symbol')

        # TODO: Add capability for re-sampled bars

        if bar_type not in self.bars:
            if bar_type.bar_spec.quote_type is QuoteType.BID:
                data = self._dataframes_bars_bid[bar_type.bar_spec.resolution]
                tick_precision = self.instrument.tick_precision
            elif bar_type.bar_spec.quote_type is QuoteType.ASK:
                data = self._dataframes_bars_ask[bar_type.bar_spec.resolution]
                tick_precision = self.instrument.tick_precision
            elif bar_type.bar_spec.quote_type is QuoteType.MID:
                data = (self._dataframes_bars_bid[bar_type.bar_spec.resolution] + self._dataframes_bars_ask[bar_type.bar_spec.resolution]) / 2
                tick_precision = self.instrument.tick_precision + 1
            elif bar_type.bar_spec.quote_type is QuoteType.LAST:
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
            while self.ticks[self.tick_index].timestamp <= current:
                self.tick_index += 1

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

        while self.ticks[self.tick_index].timestamp <= to_time:
            ticks_list.append(self.ticks[self.tick_index])
            self.tick_index += 1

        return ticks_list

    cpdef list iterate_bars(self, datetime to_time):
        """
        Return a list of bars which have closed based on the given to datetime.

        :param to_time: The time to build the bar list to.
        :return: List[Bar].
        """
        cdef list bars_list = []  # type: List[Bar]

        for bar_type, iterations in self.iterations.items():
            if self.bars[bar_type][iterations].timestamp == to_time:
                bars_list.append((bar_type, self.bars[bar_type][iterations]))
                self.iterations[bar_type] += 1

        return bars_list
