#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from cpython.datetime cimport datetime
from pandas import DataFrame
from typing import Set, List, Dict, Callable

from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.quote_type cimport QuoteType
from inv_trader.model.enums import Resolution
from inv_trader.enums.resolution cimport Resolution
from inv_trader.common.data cimport DataClient
from inv_trader.model.objects cimport Symbol, BarType, Instrument, Bar
from inv_trader.tools cimport BarBuilder


cdef class BacktestDataClient(DataClient):
    """
    Provides a data client for the BacktestEngine.
    """

    def __init__(self,
                 list instruments: List[Instrument],
                 dict bid_data: Dict[Symbol, Dict[Resolution, DataFrame]],
                 dict ask_data: Dict[Symbol, Dict[Resolution, DataFrame]]):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param instruments: The instruments needed for the backtest.
        :param bid_data: The historical bid market data needed for the backtest.
        :param bid_data: The historical ask market data needed for the backtest.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.dict_types(bid_data, Symbol, dict, 'bid_data')
        Precondition.dict_types(ask_data, Symbol, dict, 'ask_data')
        Precondition.equal(bid_data.keys(), ask_data.keys())

        self._iteration = 0
        self.data_providers = dict()

        # Convert instruments list to dictionary indexed by symbol
        cdef dict instruments_dict = {}  # type: Dict[Symbol, Instrument]
        for instrument in instruments:
            instruments_dict[instrument.symbol] = instrument
        self._instruments = instruments_dict

        # Create set of all data symbols
        cdef set bid_data_symbols = set()  # type: Set[Symbol]
        for symbol in bid_data:
            bid_data_symbols.add(symbol)
        cdef set ask_data_symbols = set()  # type: Set[Symbol]
        for symbol in ask_data:
            ask_data_symbols.add(symbol)
        assert(bid_data_symbols == ask_data_symbols)
        cdef set data_symbols = bid_data_symbols

        # Check there is the needed instrument for each data symbol
        for key in self._instruments.keys():
            assert(key in data_symbols, f'The needed instrument {key} was not provided')

        # Check that all resolution DataFrames are of the same shape and index
        cdef dict shapes = {}  # type: Dict[Resolution, tuple]
        cdef dict indexs = {}  # type: Dict[Resolution, datetime]
        for symbol, data in bid_data.items():
            for resolution, dataframe in data.items():
                if resolution not in shapes:
                    shapes[resolution] = dataframe.shape
                if resolution not in indexs:
                    indexs[resolution] = dataframe.index
                assert(dataframe.shape == shapes[resolution], f'{dataframe} shape is not equal')
                assert(dataframe.index == indexs[resolution], f'{dataframe} index is not equal')

        for symbol, data in ask_data.items():
            for resolution, dataframe in data.items():
                assert(dataframe.shape == shapes[resolution], f'{dataframe} shape is not equal')
                assert(dataframe.index == indexs[resolution], f'{dataframe} index is not equal')

        for symbol in data_symbols:
            self.data_providers[symbol] = DataProvider(instrument=self._instruments[symbol],
                                                       bid_data=bid_data[symbol],
                                                       ask_data=ask_data[symbol])

        print(self._instruments)
        print(self.data_providers)

    cpdef void connect(self):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError()

    cpdef void disconnect(self):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError()

    cpdef void update_all_instruments(self):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError()

    cpdef void update_instrument(self, Symbol symbol):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError()

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
        Precondition.true(bar_type.symbol in self.data_providers, 'bar_type.symbol in self.data_providers')

        # cdef list bars = self.bar_builders[bar_type].build_bars_range(start=0, end=quantity)
        #
        # self._log.info(f"Historical download of {len(bars)} bars for {bar_type} complete.")
        #
        # for bar in bars:
        #     handler(bar_type, bar)
        # self._log.debug(f"Historical bars hydrated to handler {handler}.")

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
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the data client.")

    cdef void iterate(self):
        """
        Iterate the data client one time step.
        """
        for bar_type in self._bar_handlers:
            self._iterate_bar_type(bar_type)

        self._iteration += 1

    cdef void _iterate_bar_type(self, BarType bar_type):
        """
        Send the next bar to all of the handlers for that bar type.
        """
        cdef Bar bar = self.bar_builders[bar_type].build_bar(self._iteration)

        for handler in self._bar_handlers[bar_type]:
            handler(bar)


cdef class DataProvider:
    """
    Provides data for the BacktestDataClient.
    """

    def __init__(self,
                 Instrument instrument,
                 dict bid_data: Dict[Resolution, DataFrame],
                 dict ask_data: Dict[Resolution, DataFrame]):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param instrument: The instrument for the data provider.
        :param bid_data: The bid data (must contain minute resolution).
        :param ask_data: The ask data (must contain minute resolution).
        """
        Precondition.true(Resolution.MINUTE in bid_data, 'Resolution.MINUTE in bid_data')
        Precondition.true(Resolution.MINUTE in ask_data, 'Resolution.MINUTE in bid_data')

        self.instrument = instrument
        self._bid_data = bid_data  # type: Dict[Resolution, DataFrame]
        self._ask_data = ask_data  # type: Dict[Resolution, DataFrame]
        self._bar_builders = {}    # type: Dict[BarType, BarBuilder]

    cpdef void register_bar_type(self, BarType bar_type):
        """
        TBA
        :param bar_type: 
        :return: 
        """
        Precondition.true(bar_type.symbol == self.instrument.symbol, 'bar_type.symbol == self.instrument.symbol')

        # TODO: Add capability for re-sampled bars
        # TODO: QuoteType.LAST not yet supported

        if bar_type not in self._bar_builders:
            if bar_type.quote_type is QuoteType.BID:
                data = self._bid_data[bar_type.resolution]
                tick_precision = self.instrument.tick_precision
            elif bar_type.quote_type is QuoteType.ASK:
                data = self._ask_data[bar_type.resolution]
                tick_precision = self.instrument.tick_precision
            elif bar_type.quote_type is QuoteType.MID:
                data = (self._bid_data[bar_type.resolution] + self._ask_data[bar_type.resolution]) / 2
                tick_precision = self.instrument.tick_precision + 1

        self._bar_builders[bar_type] = BarBuilder(data=data, decimal_precision=self.instrument.tick_precision)


