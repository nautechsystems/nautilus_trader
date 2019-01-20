#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

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
from inv_trader.model.objects cimport Symbol, BarType, Instrument, Bar
from inv_trader.tools cimport BarBuilder


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for the BacktestEngine.
    """

    def __init__(self,
                 list instruments: List[Instrument],
                 dict tick_data: Dict[Symbol, DataFrame],
                 dict bar_data_bid: Dict[Symbol, Dict[Resolution, DataFrame]],
                 dict bar_data_ask: Dict[Symbol, Dict[Resolution, DataFrame]],
                 TestClock clock,
                 Logger logger):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param instruments: The instruments needed for the backtest.
        :param tick_data: The historical tick market data needed for the backtest.
        :param bar_data_bid: The historical bid market data needed for the backtest.
        :param bar_data_ask: The historical ask market data needed for the backtest.
        :param clock: The clock for the component.
        :param logger: The logger for the component.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.dict_types(tick_data, Symbol, DataFrame, 'tick_data')
        Precondition.dict_types(bar_data_bid, Symbol, dict, 'bar_data_bid')
        Precondition.dict_types(bar_data_ask, Symbol, dict, 'bar_data_ask')
        Precondition.true(bar_data_bid.keys() == bar_data_ask.keys(), 'bar_data_bid.keys() == bar_data_ask.keys()')
        Precondition.not_none(clock, 'clock')
        Precondition.not_none(logger, 'logger')

        super().__init__(clock, logger)
        self.tick_data = tick_data
        self.bar_data_bid = bar_data_bid  # type: Dict[Symbol, Dict[Resolution, DataFrame]]
        self.bar_data_ask = bar_data_ask  # type: Dict[Symbol, Dict[Resolution, DataFrame]]

        # Set minute data index
        first_dataframe = bar_data_bid[next(iter(bar_data_bid))][Resolution.MINUTE]
        self.minute_data_index = list(pd.to_datetime(first_dataframe.index, utc=True))

        self.iteration = 0
        self.data_providers = dict()  # type: Dict[Symbol, DataProvider]

        # Convert instruments list to dictionary indexed by symbol
        cdef dict instruments_dict = {}  # type: Dict[Symbol, Instrument]
        for instrument in instruments:
            instruments_dict[instrument.symbol] = instrument
        self._instruments = instruments_dict

        # Create set of all data symbols
        cdef set bid_data_symbols = set()  # type: Set[Symbol]
        for symbol in bar_data_bid:
            bid_data_symbols.add(symbol)
        cdef set ask_data_symbols = set()  # type: Set[Symbol]
        for symbol in bar_data_ask:
            ask_data_symbols.add(symbol)
        assert(bid_data_symbols == ask_data_symbols)
        cdef set data_symbols = bid_data_symbols

        # Check there is the needed instrument for each data symbol
        for key in self._instruments.keys():
            assert(key in data_symbols, f'The needed instrument {key} was not provided')

        # Check that all resolution DataFrames are of the same shape and index
        cdef dict shapes = {}  # type: Dict[Resolution, tuple]
        cdef dict indexs = {}  # type: Dict[Resolution, datetime]
        for symbol, data in bar_data_bid.items():
            for resolution, dataframe in data.items():
                if resolution not in shapes:
                    shapes[resolution] = dataframe.shape
                if resolution not in indexs:
                    indexs[resolution] = dataframe.index
                assert(dataframe.shape == shapes[resolution], f'{dataframe} shape is not equal')
                assert(dataframe.index == indexs[resolution], f'{dataframe} index is not equal')

        for symbol, data in bar_data_ask.items():
            for resolution, dataframe in data.items():
                assert(dataframe.shape == shapes[resolution], f'{dataframe} shape is not equal')
                assert(dataframe.index == indexs[resolution], f'{dataframe} index is not equal')

        for symbol in data_symbols:
            self._log.info(f'Preparing data for {symbol}...')
            self.data_providers[symbol] = DataProvider(instrument=self._instruments[symbol],
                                                       bar_data_bid=bar_data_bid[symbol],
                                                       bar_data_ask=bar_data_ask[symbol])

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
        cdef datetime current = self.minute_data_index[0]
        cdef int next_index = 0

        while current < to_time:
            if self.minute_data_index[next_index] == current:
                next_index += 1
                self.iteration += 1
            current += time_step

        for symbol, data_provider in self.data_providers.items():
            data_provider.set_initial_iterations(self.minute_data_index[0], to_time, time_step)

        self._clock.set_time(current)

    cpdef void iterate(self, datetime time):
        """
        Iterate the data client one time step.
        """
        # TODO: Iterate tick data.

        cdef dict bars = {}
        for data_provider in self.data_providers.values():
            bars = data_provider.iterate_bars(time)
            for bar_type, bar in bars.items():
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

    cpdef void subscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Subscribe to live bar data for the given bar parameters.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        """
        Precondition.true(bar_type.symbol in self.data_providers, 'bar_type.symbol in self.data_providers')

        self.data_providers[bar_type.symbol].register_bar_type(bar_type)
        self._subscribe_bars(bar_type, handler)

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Unsubscribes from bar data for the given symbol and venue.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed (can be None).
        """
        Precondition.true(bar_type.symbol in self.data_providers, 'bar_type.symbol in self.data_providers')

        self.data_providers[bar_type.symbol].deregister_bar_type(bar_type)
        self._unsubscribe_bars(bar_type, handler)

    cpdef void subscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Subscribe to tick data for the given symbol and venue.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        """
        # Do nothing
        self._subscribe_ticks(symbol, handler)

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Unsubscribes from tick data for the given symbol and venue.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed (can be None).
        """
        # Do nothing.
        self._unsubscribe_ticks(symbol, handler)


cdef class DataProvider:
    """
    Provides data for the BacktestDataClient.
    """

    def __init__(self,
                 Instrument instrument,
                 dict bar_data_bid: Dict[Resolution, DataFrame],
                 dict bar_data_ask: Dict[Resolution, DataFrame]):
        """
        Initializes a new instance of the DataProvider class.

        :param instrument: The instrument for the data provider.
        :param bar_data_bid: The bid data for the data provider (must contain minute resolution).
        :param bar_data_ask: The ask data for the data provider (must contain minute resolution).
        """
        Precondition.true(Resolution.MINUTE in bar_data_bid, 'Resolution.MINUTE in bid_data')
        Precondition.true(Resolution.MINUTE in bar_data_ask, 'Resolution.MINUTE in bid_data')

        self.instrument = instrument
        self.iterations = dict()           # type: Dict[BarType, int]
        self._bar_data_bid = bar_data_bid  # type: Dict[Resolution, DataFrame]
        self._bar_data_ask = bar_data_ask  # type: Dict[Resolution, DataFrame]
        self._bars = dict()                # type: Dict[BarType, List[Bar]]

    cpdef void register_bar_type(self, BarType bar_type):
        """
        Register the given bar type with the data provider.
        
        :param bar_type: The bar type to register.
        """
        Precondition.true(bar_type.symbol == self.instrument.symbol, 'bar_type.symbol == self.instrument.symbol')

        # TODO: Add capability for re-sampled bars
        # TODO: QuoteType.LAST not yet supported

        if bar_type not in self._bars:
            if bar_type.quote_type is QuoteType.BID:
                data = self._bar_data_bid[bar_type.resolution]
                tick_precision = self.instrument.tick_precision
            elif bar_type.quote_type is QuoteType.ASK:
                data = self._bar_data_ask[bar_type.resolution]
                tick_precision = self.instrument.tick_precision
            elif bar_type.quote_type is QuoteType.MID:
                data = (self._bar_data_bid[bar_type.resolution] + self._bar_data_ask[bar_type.resolution]) / 2
                tick_precision = self.instrument.tick_precision + 1

            builder = BarBuilder(data=data, decimal_precision=tick_precision)
            self._bars[bar_type] = builder.build_bars_all()

        if bar_type not in self.iterations:
            self.iterations[bar_type] = 0

    cpdef void deregister_bar_type(self, BarType bar_type):
        """
        Deregister the given bar type with the data provider.
        
        :param bar_type: The bar type to deregister.
        """
        Precondition.true(bar_type.symbol == self.instrument.symbol, 'bar_type.symbol == self.instrument.symbol')

        if bar_type in self._bars:
            del self._bars[bar_type]

        if bar_type in self.iterations:
            del self.iterations[bar_type]

    cpdef void set_initial_iterations(
            self,
            datetime from_time,
            datetime to_time,
            timedelta time_step):
        """
        Set the iteration based on the given time.
        """
        cdef datetime current = from_time

        while current < to_time:
            for bar_type, bars in self._bars.items():
                next_index = self.iterations[bar_type]
                if self._bars[bar_type][next_index].timestamp == current:
                    self.iterations[bar_type] += 1
            current += time_step

    cpdef dict iterate_bars(self, datetime time):
        """
        Build a list of bars which have closed based on the held historical data
        and the given datetime.

        :return: The list of built bars.
        """
        cdef dict bars_dict = dict()
        cdef int next_index = 0

        for bar_type, bars in self._bars.items():
            next_index = self.iterations[bar_type]
            # print(self.iterations)
            # print(self._bars[bar_type][next_index].timestamp)
            # print(time)
            if self._bars[bar_type][next_index].timestamp == time:
                bars_dict[bar_type] = bars[next_index]
                self.iterations[bar_type] += 1

        return bars_dict
