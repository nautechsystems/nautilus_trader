# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
This module provides a single data container for backtesting.

A `BacktestDataContainer` is a convenient container for holding and organizing
backtest related data - which can be passed to one or more `BacktestDataEngine`(s).
"""

import pandas as pd
from pandas import DatetimeIndex

from libc.stdint cimport int64_t

from nautilus_trader.backtest.data_client cimport BacktestDataClient
from nautilus_trader.backtest.data_client cimport BacktestMarketDataClient

from nautilus_trader.core.functions import get_size_of  # Not cimport

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.data cimport GenericData
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.orderbook.book cimport OrderBookData


cdef class BacktestDataContainer:
    """
    Provides a container for backtest data.
    """

    def __init__(self):
        """
        Initialize a new instance of the `BacktestDataContainer` class.
        """
        self._added_instrument_ids = set()  # type: set[InstrumentId]
        self.clients = {}                   # type: dict[ClientId, type]
        self.generic_data = []              # type: list[GenericData]
        self.books = []                     # type: list[InstrumentId]
        self.order_book_data = []           # type: list[OrderBookData]
        self.instruments = {}               # type: dict[InstrumentId, Instrument]
        self.quote_ticks = {}               # type: dict[InstrumentId, pd.DataFrame]
        self.trade_ticks = {}               # type: dict[InstrumentId, pd.DataFrame]
        self.bars_bid = {}                  # type: dict[InstrumentId, dict[BarAggregation, pd.DataFrame]]
        self.bars_ask = {}                  # type: dict[InstrumentId, dict[BarAggregation, pd.DataFrame]]

    def add_generic_data(self, ClientId client_id, list data) -> None:
        """
        Add the generic data to the container.

        Parameters
        ----------
        client_id : ClientId
            The data client identifier to associate with the generic data.
        data : list[GenericData]
            The data to add.

        Raises
        ------
        ValueError
            If data is empty.

        """
        Condition.not_none(client_id, "client_id")
        Condition.not_none(data, "data")
        Condition.not_empty(data, "data")
        Condition.list_type(data, GenericData, "data")

        # Add to clients to be constructed in backtest engine
        if client_id not in self.clients:
            self.clients[client_id] = BacktestDataClient

        # Add data
        self.generic_data = sorted(
            self.generic_data + data,
            key=lambda x: x.timestamp_ns,
        )

    def add_order_book_data(self, list data) -> None:
        """
        Add the order book data to the container.

        Parameters
        ----------
        data : list[OrderBookData]
            The order book data to add.

        Raises
        ------
        ValueError
            If data is empty.

        """
        Condition.not_none(data, "data")
        Condition.not_empty(data, "data")
        Condition.list_type(data, OrderBookData, "snapshots")

        cdef InstrumentId instrument_id = data[0].instrument_id
        self._added_instrument_ids.add(instrument_id)

        if instrument_id not in self.books:
            self.books.append(instrument_id)

        cdef ClientId client_id = instrument_id.venue.client_id
        # Add to clients to be constructed in backtest engine
        if client_id not in self.clients:
            self.clients[client_id] = BacktestMarketDataClient

        # Add data
        self.order_book_data = sorted(
            self.order_book_data + data,
            key=lambda x: x.timestamp_ns,
        )

    def add_instrument(self, Instrument instrument) -> None:
        """
        Add the instrument to the container.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        Condition.not_none(instrument, "instrument")

        # Add to clients to be constructed in backtest engine
        cdef ClientId client_id = instrument.id.venue.client_id
        if client_id not in self.clients:
            self.clients[client_id] = BacktestMarketDataClient

        # Add data
        self.instruments[instrument.id] = instrument
        self.instruments = dict(sorted(self.instruments.items()))

    def add_quote_ticks(self, InstrumentId instrument_id, data: pd.DataFrame) -> None:
        """
        Add the quote tick data to the container.

        The format of the dataframe is expected to be a DateTimeIndex (times are
        assumed to be UTC, and are converted to tz-aware in pre-processing).

        With index column named 'timestamp', and 'bid', 'ask' data columns.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the quote tick data.
        data : pd.DataFrame
            The quote tick data to add.

        Raises
        ------
        ValueError
            If data is empty.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(data, "data")
        Condition.type(data, pd.DataFrame, "data")
        Condition.false(data.empty, "data was empty")

        self._added_instrument_ids.add(instrument_id)

        # Add to clients to be constructed in backtest engine
        cdef ClientId client_id = instrument_id.venue.client_id
        if client_id not in self.clients:
            self.clients[client_id] = BacktestMarketDataClient

        # Add data
        self.quote_ticks[instrument_id] = data
        self.quote_ticks = dict(sorted(self.quote_ticks.items()))

    def add_trade_ticks(self, InstrumentId instrument_id, data: pd.DataFrame) -> None:
        """
        Add the trade tick data to the container.

        The format of the dataframe is expected to be a DateTimeIndex (times are
        assumed to be UTC, and are converted to tz-aware in pre-processing).

        With index column named 'timestamp', and 'trade_id', 'price', 'quantity',
        'buyer_maker' data columns.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the trade tick data.
        data : pd.DataFrame
            The trade tick data to add.

        Raises
        ------
        ValueError
            If data is empty.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(data, "data")
        Condition.type(data, pd.DataFrame, "data")
        Condition.false(data.empty, "data was empty")

        self._added_instrument_ids.add(instrument_id)

        # Add to clients to be constructed in backtest engine
        cdef ClientId client_id = instrument_id.venue.client_id
        if client_id not in self.clients:
            self.clients[client_id] = BacktestMarketDataClient

        # Add data
        self.trade_ticks[instrument_id] = data
        self.trade_ticks = dict(sorted(self.trade_ticks.items()))

    def add_bars(
        self,
        InstrumentId instrument_id,
        BarAggregation aggregation,
        PriceType price_type,
        data: pd.DataFrame
    ) -> None:
        """
        Add the bar data to the container.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the bar data.
        aggregation : BarAggregation
            The bar aggregation of the data.
        price_type : PriceType
            The price type of the data.
        data : pd.DataFrame
            The bar data to add.

        Raises
        ------
        ValueError
            If price_type is LAST.
        ValueError
            If data is empty.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(data, "data")
        Condition.true(price_type != PriceType.LAST, "price_type was PriceType.LAST")
        Condition.false(data.empty, "data was empty")

        self._added_instrument_ids.add(instrument_id)

        # Add to clients to be constructed in backtest engine
        cdef ClientId client_id = instrument_id.venue.client_id
        if client_id not in self.clients:
            self.clients[client_id] = BacktestMarketDataClient

        # Add data
        if price_type == PriceType.BID:
            if instrument_id not in self.bars_bid:
                self.bars_bid[instrument_id] = {}
                self.bars_bid = dict(sorted(self.bars_bid.items()))
            self.bars_bid[instrument_id][aggregation] = data
            self.bars_bid[instrument_id] = dict(sorted(self.bars_bid[instrument_id].items()))
        elif price_type == PriceType.ASK:
            if instrument_id not in self.bars_ask:
                self.bars_ask[instrument_id] = {}
                self.bars_ask = dict(sorted(self.bars_ask.items()))
            self.bars_ask[instrument_id][aggregation] = data
            self.bars_ask[instrument_id] = dict(sorted(self.bars_ask[instrument_id].items()))

    def check_integrity(self) -> None:
        """
        Check the integrity of the data inside the container.

        Raises
        ------
        RuntimeError
            If any integrity check fails.

        """
        cdef InstrumentId instrument_id

        # Check for execution type data for each added instrument
        for instrument_id in self.instruments.keys():
            if instrument_id not in self.books \
                    and instrument_id not in self.bars_bid \
                    and instrument_id not in self.bars_ask \
                    and instrument_id not in self.quote_ticks \
                    and instrument_id not in self.trade_ticks:
                raise RuntimeError(f"No execution level data for {instrument_id}")

        for instrument_id in self._added_instrument_ids:
            # Check instrument for each added data instrument_id
            if instrument_id not in self.instruments:
                raise RuntimeError(f"No instrument for {instrument_id}")

            # Check symmetry of bid ask bar data
            bid_bars_keys = self.bars_bid.get(instrument_id, {}).keys()
            ask_bars_keys = self.bars_ask.get(instrument_id, {}).keys()
            if bid_bars_keys != ask_bars_keys:
                raise RuntimeError(f"Bar data mismatch for {instrument_id}")

        # Check that all bar DataFrames for each instrument_id are of the same shape and index
        cdef dict shapes = {}  # type: dict[BarAggregation, tuple]
        cdef dict indices = {}  # type: dict[BarAggregation, DatetimeIndex]
        for instrument_id, data in self.bars_bid.items():
            for aggregation, dataframe in data.items():
                if aggregation not in shapes:
                    shapes[aggregation] = dataframe.shape
                if aggregation not in indices:
                    indices[aggregation] = dataframe.index
                if dataframe.shape != shapes[aggregation]:
                    raise RuntimeError(f"{dataframe} bid ask shape is not equal")
                if not all(dataframe.index == indices[aggregation]):
                    raise RuntimeError(f"{dataframe} bid ask index is not equal")
        for instrument_id, data in self.bars_ask.items():
            for aggregation, dataframe in data.items():
                if dataframe.shape != shapes[aggregation]:
                    raise RuntimeError(f"{dataframe} bid ask shape is not equal")
                if not all(dataframe.index == indices[aggregation]):
                    raise RuntimeError(f"{dataframe} bid ask index is not equal")

    def has_quote_data(self, InstrumentId instrument_id) -> bool:
        """
        Return a value indicating whether the container has quote data for the
        given instrument_id.

        Parameters
        ----------
        instrument_id : InstrumentId
            The query instrument identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(instrument_id, "instrument_id")
        return instrument_id in self.quote_ticks or instrument_id in self.bars_bid

    def has_trade_data(self, InstrumentId instrument_id) -> bool:
        """
        Return a value indicating whether the container has trade data for the
        given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The query instrument identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(instrument_id, "instrument_id")
        return instrument_id in self.trade_ticks

    def total_data_size(self) -> int:
        """
        Return the total memory size of the data in the container.

        Returns
        -------
        int64
            The total bytes.

        """
        cdef int64_t size = 0
        size += get_size_of(self.generic_data)
        size += get_size_of(self.order_book_data)
        size += get_size_of(self.quote_ticks)
        size += get_size_of(self.trade_ticks)
        size += get_size_of(self.bars_bid)
        size += get_size_of(self.bars_ask)
        return size
