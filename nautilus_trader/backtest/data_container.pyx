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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport get_size_of
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Security
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument


cdef class BacktestDataContainer:
    """
    Provides a container for backtest data.
    """

    def __init__(self):
        """
        Initialize a new instance of the `BacktestDataContainer` class.
        """
        self.venues = set()    # type: set[Venue]
        self.securities = set()   # type: set[Instrument]
        self.instruments = {}  # type: dict[Security, Instrument]
        self.quote_ticks = {}  # type: dict[Security, pd.DataFrame]
        self.trade_ticks = {}  # type: dict[Security, pd.DataFrame]
        self.bars_bid = {}     # type: dict[Security, dict[BarAggregation, pd.DataFrame]]
        self.bars_ask = {}     # type: dict[Security, dict[BarAggregation, pd.DataFrame]]

    cpdef void add_instrument(self, Instrument instrument) except *:
        """
        Add the instrument to the container.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        Condition.not_none(instrument, "instrument")

        self.venues.add(instrument.security.venue)
        self.instruments[instrument.security] = instrument
        self.instruments = dict(sorted(self.instruments.items()))

    cpdef void add_quote_ticks(self, Security security, data: pd.DataFrame) except *:
        """
        Add the quote tick data to the container.

        The format of the dataframe is expected to be a DateTimeIndex (times are
        assumed to be UTC, and are converted to tz-aware in pre-processing).

        With index column named 'timestamp', and 'bid', 'ask' data columns.

        Parameters
        ----------
        security : Security
            The security identifier for the quote tick data.
        data : pd.DataFrame
            The quote tick data to add.

        """
        Condition.not_none(security, "security")
        Condition.not_none(data, "data")
        Condition.type(data, pd.DataFrame, "data")

        self.securities.add(security)
        self.quote_ticks[security] = data
        self.quote_ticks = dict(sorted(self.quote_ticks.items()))

    cpdef void add_trade_ticks(self, Security security, data: pd.DataFrame) except *:
        """
        Add the trade tick data to the container.

        The format of the dataframe is expected to be a DateTimeIndex (times are
        assumed to be UTC, and are converted to tz-aware in pre-processing).

        With index column named 'timestamp', and 'trade_id', 'price', 'quantity',
        'buyer_maker' data columns.

        Parameters
        ----------
        security : Security
            The security identifier for the trade tick data.
        data : pd.DataFrame
            The trade tick data to add.

        Returns
        -------

        """
        Condition.not_none(security, "security")
        Condition.not_none(data, "data")
        Condition.type(data, pd.DataFrame, "data")

        self.securities.add(security)
        self.trade_ticks[security] = data
        self.trade_ticks = dict(sorted(self.trade_ticks.items()))

    cpdef void add_bars(
        self,
        Security security,
        BarAggregation aggregation,
        PriceType price_type,
        data: pd.DataFrame
    ) except *:
        """
        Add the bar data to the container.

        Parameters
        ----------
        security : Security
            The security identifier for the bar data.
        aggregation : BarAggregation (Enum)
            The bar aggregation of the data.
        price_type : PriceType (Enum)
            The price type of the data.
        data : pd.DataFrame
            The bar data to add.

        """
        Condition.not_none(security, "security")
        Condition.not_none(data, "data")
        Condition.true(price_type != PriceType.LAST, "price_type was PriceType.LAST")

        self.securities.add(security)

        if price_type == PriceType.BID:
            if security not in self.bars_bid:
                self.bars_bid[security] = {}
                self.bars_bid = dict(sorted(self.bars_bid.items()))
            self.bars_bid[security][aggregation] = data
            self.bars_bid[security] = dict(sorted(self.bars_bid[security].items()))

        if price_type == PriceType.ASK:
            if security not in self.bars_ask:
                self.bars_ask[security] = {}
                self.bars_ask = dict(sorted(self.bars_ask.items()))
            self.bars_ask[security][aggregation] = data
            self.bars_ask[security] = dict(sorted(self.bars_ask[security].items()))

    cpdef void check_integrity(self) except *:
        """
        Check the integrity of the data inside the container.

        Raises
        ------
        ValueError
            If any integrity check fails.

        """
        # Check there is the needed instrument for each data security
        for security in self.securities:
            Condition.true(security in self.instruments, f"security not in self.instruments")

        # Check that all bar DataFrames for each security are of the same shape and index
        cdef dict shapes = {}  # type: dict[BarAggregation, tuple]
        cdef dict indexs = {}  # type: dict[BarAggregation, DatetimeIndex]
        for security, data in self.bars_bid.items():
            for aggregation, dataframe in data.items():
                if aggregation not in shapes:
                    shapes[aggregation] = dataframe.shape
                if aggregation not in indexs:
                    indexs[aggregation] = dataframe.index
                if dataframe.shape != shapes[aggregation]:
                    raise RuntimeError(f"{dataframe} bid ask shape is not equal")
                if not all(dataframe.index == indexs[aggregation]):
                    raise RuntimeError(f"{dataframe} bid ask index is not equal")
        for security, data in self.bars_ask.items():
            for aggregation, dataframe in data.items():
                if dataframe.shape != shapes[aggregation]:
                    raise RuntimeError(f"{dataframe} bid ask shape is not equal")
                if not all(dataframe.index == indexs[aggregation]):
                    raise RuntimeError(f"{dataframe} bid ask index is not equal")

    cpdef bint has_quote_data(self, Security security) except *:
        """
        Return a value indicating whether the container has quote data for the
        given security.

        Parameters
        ----------
        security : Security
            The query security.

        Returns
        -------
        bool

        """
        Condition.not_none(security, "security")
        return security in self.quote_ticks or security in self.bars_bid

    cpdef bint has_trade_data(self, Security security) except *:
        """
        Return a value indicating whether the container has trade data for the
        given security.

        Parameters
        ----------
        security : Security
            The query security.

        Returns
        -------
        bool

        """
        Condition.not_none(security, "security")
        return security in self.trade_ticks

    cpdef long total_data_size(self):
        """
        Return the total memory size of the data in the container.

        Returns
        -------
        long
            The total bytes.

        """
        cdef long size = 0
        size += get_size_of(self.quote_ticks)
        size += get_size_of(self.trade_ticks)
        size += get_size_of(self.bars_bid)
        size += get_size_of(self.bars_ask)
        return size
