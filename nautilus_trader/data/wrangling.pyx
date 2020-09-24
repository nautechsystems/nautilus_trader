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

import pandas as pd

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport as_utc_index
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class TickDataWrangler:
    """
    Provides a means of building lists of ticks from the given Pandas DataFrames
    of bid and ask data. Provided data can either be tick data or bar data.
    """

    def __init__(
            self,
            Instrument instrument not None,
            data_ticks: pd.DataFrame=None,
            dict data_bars_bid=None,
            dict data_bars_ask=None,
    ):
        """
        Initialize a new instance of the TickDataWrangler class.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the data wrangler.
        data_ticks : pd.DataFrame
            The optional pd.DataFrame containing the tick data.
        data_bars_bid : Dict[BarAggregation, pd.DataFrame], optional
            The bars bid data.
        data_bars_ask : Dict[BarAggregation, pd.DataFrame], optional
            The bars ask data.

        Raises
        ------
        ValueError
            If tick_data is a type other than None or DataFrame.
        ValueError
            If bid_data is a type other than None or Dict.
        ValueError
            If ask_data is a type other than None or Dict.
        ValueError
            If tick_data is None and the bars data is None.

        """
        Condition.type_or_none(data_ticks, pd.DataFrame, "tick_data")
        Condition.type_or_none(data_bars_bid, dict, "bid_data")
        Condition.type_or_none(data_bars_ask, dict, "ask_data")

        if data_ticks is not None and len(data_ticks) > 0:
            self._data_ticks = as_utc_index(data_ticks)
        else:
            Condition.true(data_bars_bid is not None, "data_bars_bid is not None")
            Condition.true(data_bars_ask is not None, "data_bars_ask is not None")
            self._data_bars_bid = data_bars_bid
            self._data_bars_ask = data_bars_ask

        self.instrument = instrument

        self.tick_data = []
        self.resolution = BarAggregation.UNDEFINED

    cpdef void build(self, int symbol_indexer) except *:
        """
        Return the built ticks from the held data.

        :return List[Tick].
        """
        if self._data_ticks is not None and len(self._data_ticks) > 0:
            # Build ticks from data
            self.tick_data = self._data_ticks
            self.tick_data["symbol"] = symbol_indexer

            if "bid_size" not in self.tick_data.columns:
                self.tick_data["bid_size"] = 1.0

            if "ask_size" not in self.tick_data.columns:
                self.tick_data["ask_size"] = 1.0

            self.resolution = BarAggregation.TICK
            return

        # Build ticks from highest resolution bar data
        if BarAggregation.SECOND in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarAggregation.SECOND]
            bars_ask = self._data_bars_ask[BarAggregation.SECOND]
            self.resolution = BarAggregation.SECOND
        elif BarAggregation.MINUTE in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarAggregation.MINUTE]
            bars_ask = self._data_bars_ask[BarAggregation.MINUTE]
            self.resolution = BarAggregation.MINUTE
        elif BarAggregation.HOUR in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarAggregation.HOUR]
            bars_ask = self._data_bars_ask[BarAggregation.HOUR]
            self.resolution = BarAggregation.HOUR
        elif BarAggregation.DAY in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarAggregation.DAY]
            bars_ask = self._data_bars_ask[BarAggregation.DAY]
            self.resolution = BarAggregation.DAY

        Condition.not_none(bars_bid, "bars_bid")
        Condition.not_none(bars_ask, "bars_ask")
        Condition.true(len(bars_bid) > 0, "len(bars_bid) > 0")
        Condition.true(len(bars_ask) > 0, "len(bars_ask) > 0")
        Condition.true(all(bars_bid.index) == all(bars_ask.index), "bars_bid.index == bars_ask.index")
        Condition.true(bars_bid.shape == bars_ask.shape, "bars_bid.shape == bars_ask.shape")

        bars_bid = as_utc_index(bars_bid)
        bars_ask = as_utc_index(bars_ask)

        cdef dict data_open = {
            "bid": bars_bid["open"].values,
            "ask": bars_ask["open"].values,
            "bid_size": bars_bid["volume"].values,
            "ask_size": bars_ask["volume"].values
        }

        cdef dict data_high = {
            "bid": bars_bid["high"].values,
            "ask": bars_ask["high"].values,
            "bid_size": bars_bid["volume"].values,
            "ask_size": bars_ask["volume"].values
        }

        cdef dict data_low = {
            "bid": bars_bid["low"].values,
            "ask": bars_ask["low"].values,
            "bid_size": bars_bid["volume"].values,
            "ask_size": bars_ask["volume"].values
        }

        cdef dict data_close = {
            "bid": bars_bid["close"],
            "ask": bars_ask["close"],
            "bid_size": bars_bid["volume"],
            "ask_size": bars_ask["volume"]
        }

        df_ticks_o = pd.DataFrame(data=data_open, index=bars_bid.index.shift(periods=-100, freq="ms"))
        df_ticks_h = pd.DataFrame(data=data_high, index=bars_bid.index.shift(periods=-100, freq="ms"))
        df_ticks_l = pd.DataFrame(data=data_low, index=bars_bid.index.shift(periods=-100, freq="ms"))
        df_ticks_c = pd.DataFrame(data=data_close)

        # Drop rows with no volume
        df_ticks_o = df_ticks_o[(df_ticks_h[["bid_size"]] > 0).all(axis=1)]
        df_ticks_h = df_ticks_h[(df_ticks_h[["bid_size"]] > 0).all(axis=1)]
        df_ticks_l = df_ticks_l[(df_ticks_l[["bid_size"]] > 0).all(axis=1)]
        df_ticks_c = df_ticks_c[(df_ticks_c[["bid_size"]] > 0).all(axis=1)]
        df_ticks_o = df_ticks_o[(df_ticks_h[["ask_size"]] > 0).all(axis=1)]
        df_ticks_h = df_ticks_h[(df_ticks_h[["ask_size"]] > 0).all(axis=1)]
        df_ticks_l = df_ticks_l[(df_ticks_l[["ask_size"]] > 0).all(axis=1)]
        df_ticks_c = df_ticks_c[(df_ticks_c[["ask_size"]] > 0).all(axis=1)]

        # Set high low tick volumes to zero
        df_ticks_o["bid_size"] = 0
        df_ticks_o["ask_size"] = 0
        df_ticks_h["bid_size"] = 0
        df_ticks_h["ask_size"] = 0
        df_ticks_l["bid_size"] = 0
        df_ticks_l["ask_size"] = 0

        # Merge tick data
        df_ticks_final = pd.concat([df_ticks_o, df_ticks_h, df_ticks_l, df_ticks_c])
        df_ticks_final.sort_index(axis=0, kind="mergesort", inplace=True)

        # Build ticks from data
        self.tick_data = df_ticks_final
        self.tick_data["symbol"] = symbol_indexer

    cpdef QuoteTick _build_tick_from_values_with_sizes(self, double[:] values, datetime timestamp):
        """
        Build a tick from the given values. The function expects the values to
        be an ndarray with 4 elements [bid, ask, bid_size, ask_size] of type double.
        """
        return QuoteTick(
            self.instrument.symbol,
            Price(values[0], self.instrument.price_precision),
            Price(values[1], self.instrument.price_precision),
            Quantity(values[2], self.instrument.size_precision),
            Quantity(values[3], self.instrument.size_precision),
            timestamp,
        )

    cpdef QuoteTick _build_tick_from_values(self, double[:] values, datetime timestamp):
        """
        Build a tick from the given values. The function expects the values to
        be an ndarray with 4 elements [bid, ask, bid_size, ask_size] of type double.
        """
        return QuoteTick(
            self.instrument.symbol,
            Price(values[0], self.instrument.price_precision),
            Price(values[1], self.instrument.price_precision),
            Quantity.one(),
            Quantity.one(),
            timestamp,
        )


cdef class BarDataWrangler:
    """
    Provides a means of building lists of bars from a given Pandas DataFrame of
    the correct specification.
    """

    def __init__(
            self,
            int precision,
            int volume_multiple=1,
            data: pd.DataFrame=None,
    ):
        """
        Initialize a new instance of the BarDataWrangler class.

        Parameters
        ----------
        precision : int
            The decimal precision for bar prices (>= 0).
        volume_multiple : int
            The volume multiple for the builder (> 0). This can be used to
            transform decimalized volumes to integers.
        data : pd.DataFrame
            The the bars market data.

        Raises
        ------
        ValueError
            If decimal_precision is negative (< 0).
        ValueError
            If volume_multiple is not positive (> 0).
        ValueError
            If data is a type other than DataFrame.

        """
        Condition.not_negative_int(precision, "precision")
        Condition.positive_int(volume_multiple, "volume_multiple")
        Condition.type(data, pd.DataFrame, "data")

        self._precision = precision
        self._volume_multiple = volume_multiple
        self._data = as_utc_index(data)

    cpdef list build_bars_all(self):
        """
        Return a list of bars from all data.

        Returns
        -------
        List[Bar]

        """
        return list(map(self._build_bar,
                        self._data.values,
                        pd.to_datetime(self._data.index)))

    cpdef list build_bars_from(self, int index=0):
        """
        Return a list of bars from the given index (>= 0).

        Returns
        -------
        List[Bar]

        """
        Condition.not_negative_int(index, "index")

        return list(map(self._build_bar,
                        self._data.iloc[index:].values,
                        pd.to_datetime(self._data.iloc[index:].index)))

    cpdef list build_bars_range(self, int start=0, int end=-1):
        """
        Return a list of bars within the given range.

        Returns
        -------
        List[Bar]

        """
        Condition.not_negative_int(start, "start")

        return list(map(self._build_bar,
                        self._data.iloc[start:end].values,
                        pd.to_datetime(self._data.iloc[start:end].index)))

    cpdef Bar _build_bar(self, double[:] values, datetime timestamp):
        # Build a bar from the given index and values. The function expects the
        # values to be an ndarray with 5 elements [open, high, low, close, volume].
        return Bar(
            Price(values[0], self._precision),
            Price(values[1], self._precision),
            Price(values[2], self._precision),
            Price(values[3], self._precision),
            Quantity(values[4] * self._volume_multiple),
            timestamp,
        )
