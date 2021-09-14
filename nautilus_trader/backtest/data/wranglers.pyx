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

import random
from decimal import Decimal

import pandas as pd

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport as_utc_index
from nautilus_trader.core.datetime cimport secs_to_nanos
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSideParser
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class QuoteTickDataWrangler:
    """
    Provides a means of building lists of ticks from the given Pandas DataFrames
    of bid and ask data. Provided data can either be tick data or bar data.
    """

    def __init__(self, Instrument instrument not None):
        """
        Initialize a new instance of the ``QuoteTickDataWrangler`` class.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the data wrangler.

        """
        self.instrument = instrument

    def process_tick_data(
        self,
        data: pd.DataFrame,
        default_volume=Decimal(1_000_000),
    ):
        """
        Process the give tick dataset into built quote tick objects.

        Parameters
        ----------
        data : pd.DataFrame
            The tick data to process.
        default_volume : int, float or Decimal
            The default volume for each tick (if not provided).

        Returns
        -------
        list[QuoteTick]

        """
        Condition.false(data.empty, "data.empty")
        Condition.not_none(default_volume, "default_volume")

        as_utc_index(data)

        if "bid_size" not in data.columns:
            data["bid_size"] = <double>float(default_volume)
        if "ask_size" not in data.columns:
            data["ask_size"] = <double>float(default_volume)

        return list(map(
            self._build_tick_from_values,
            data.values,
            [<double>dt.timestamp() for dt in data.index],
        ))

    def process_bar_data(
        self,
        bid_data: pd.DataFrame,
        ask_data: pd.DataFrame,
        default_volume: Decimal=Decimal(1_000_000),
        random_seed=None,
    ):
        """
        Process the given bar datasets into built quote tick objects.

        Parameters
        ----------
        bid_data : pd.DataFrame
            The bid bar data.
        ask_data : pd.DataFrame
            The ask bar data.
        default_volume : Decimal
            The volume per tick if not available from the data.
        random_seed : int, optional
            The random seed for shuffling order of high and low ticks from bar
            data. If random_seed is ``None`` then won't shuffle.

        """
        Condition.false(bid_data.empty, "bid_data")
        Condition.false(ask_data.empty, "ask_data")
        Condition.not_none(default_volume, "default_volume")
        if random_seed is not None:
            Condition.type(random_seed, int, "random_seed")

        # Ensure index is tz-aware UTC
        bid_data = as_utc_index(bid_data)
        ask_data = as_utc_index(ask_data)

        if "volume" not in bid_data:
            bid_data["volume"] = <double>float(default_volume * 4)

        if "volume" not in ask_data:
            ask_data["volume"] = <double>float(default_volume * 4)

        cdef dict data_open = {
            "bid": bid_data["open"],
            "ask": ask_data["open"],
            "bid_size": bid_data["volume"] / 4,
            "ask_size": ask_data["volume"] / 4,
        }

        cdef dict data_high = {
            "bid": bid_data["high"],
            "ask": ask_data["high"],
            "bid_size": bid_data["volume"] / 4,
            "ask_size": ask_data["volume"] / 4,
        }

        cdef dict data_low = {
            "bid": bid_data["low"],
            "ask": ask_data["low"],
            "bid_size": bid_data["volume"] / 4,
            "ask_size": ask_data["volume"] / 4,
        }

        cdef dict data_close = {
            "bid": bid_data["close"],
            "ask": ask_data["close"],
            "bid_size": bid_data["volume"] / 4,
            "ask_size": ask_data["volume"] / 4,
        }

        df_ticks_o = pd.DataFrame(data=data_open)
        df_ticks_h = pd.DataFrame(data=data_high)
        df_ticks_l = pd.DataFrame(data=data_low)
        df_ticks_c = pd.DataFrame(data=data_close)

        # Latency offsets
        df_ticks_o.index = df_ticks_o.index.shift(periods=-300, freq="ms")
        df_ticks_h.index = df_ticks_h.index.shift(periods=-200, freq="ms")
        df_ticks_l.index = df_ticks_l.index.shift(periods=-100, freq="ms")

        # Merge tick data
        df_ticks_final = pd.concat([df_ticks_o, df_ticks_h, df_ticks_l, df_ticks_c])
        df_ticks_final.sort_index(axis=0, kind="mergesort", inplace=True)

        cdef int i
        # Randomly shift high low prices
        if random_seed is not None:
            random.seed(random_seed)
            for i in range(0, len(df_ticks_o), 4):
                if random.getrandbits(1):
                    high = df_ticks_h.iloc[i]
                    low = df_ticks_l.iloc[i]
                    df_ticks_final.iloc[i + 1] = low
                    df_ticks_final.iloc[i + 2] = high

        return list(map(
            self._build_tick_from_values,
            df_ticks_final.values,
            [dt.timestamp() for dt in df_ticks_final.index],
        ))

    # cpdef method for Python wrap() (called with map)
    cpdef QuoteTick _build_tick_from_values(self, double[:] values, double timestamp):
        # Build a quote tick from the given values. The function expects the values to
        # be an ndarray with 4 elements [bid, ask, bid_size, ask_size] of type double.
        return QuoteTick(
            instrument_id=self.instrument.id,
            bid=Price(values[0], self.instrument.price_precision),
            ask=Price(values[1], self.instrument.price_precision),
            bid_size=Quantity(values[2], self.instrument.size_precision),
            ask_size=Quantity(values[3], self.instrument.size_precision),
            ts_event=secs_to_nanos(timestamp),  # TODO(cs): Hardcoded identical for now
            ts_init=secs_to_nanos(timestamp),
        )


cdef class TradeTickDataWrangler:
    """
    Provides a means of building lists of trade ticks from the given DataFrame
    of data.
    """

    def __init__(self, Instrument instrument not None):
        """
        Initialize a new instance of the ``TradeTickDataWrangler`` class.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the data wrangler.

        """
        self.instrument = instrument

    def process(self, data: pd.DataFrame):
        processed = pd.DataFrame(index=data.index)
        processed["price"] = data["price"].apply(lambda x: f'{x:.{self.instrument.price_precision}f}')
        processed["quantity"] = data["quantity"].apply(lambda x: f'{x:.{self.instrument.size_precision}f}')
        processed["aggressor_side"] = self._create_side_if_not_exist(data)
        processed["match_id"] = data["trade_id"].apply(str)

        return list(map(
            self._build_tick_from_values,
            processed.values,
            [dt.timestamp() for dt in data.index]))

    def _create_side_if_not_exist(self, data):
        if "side" in data.columns:
            return data["side"]
        else:
            return data["buyer_maker"].apply(lambda x: "SELL" if x is True else "BUY")

    cpdef TradeTick _build_tick_from_values(self, str[:] values, double timestamp):
        # Build a quote tick from the given values. The function expects the values to
        # be an ndarray with 4 elements [bid, ask, bid_size, ask_size] of type double.
        return TradeTick(
            instrument_id=self.instrument.id,
            price=Price(values[0], self.instrument.price_precision),
            size=Quantity(values[1], self.instrument.size_precision),
            aggressor_side=AggressorSideParser.from_str(values[2]),
            match_id=values[3],
            ts_event=secs_to_nanos(timestamp),  # TODO(cs): Hardcoded identical for now
            ts_init=secs_to_nanos(timestamp),
        )


cdef class BarDataWrangler:
    """
    Provides a means of building lists of bars from a given Pandas DataFrame of
    the correct specification.
    """

    def __init__(
        self,
        BarType bar_type,
        int price_precision,
        int size_precision,
        data: pd.DataFrame=None,
    ):
        """
        Initialize a new instance of the ``BarDataWrangler`` class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the wrangler.
        price_precision : int
            The decimal precision for bar prices (>= 0).
        size_precision : int
            The decimal precision for bar volumes (>= 0).
        data : pd.DataFrame
            The the bars market data.

        Raises
        ------
        ValueError
            If price_precision is negative (< 0).
        ValueError
            If size_precision is negative (< 0).
        ValueError
            If data not type DataFrame.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_negative_int(price_precision, "price_precision")
        Condition.not_negative_int(size_precision, "size_precision")
        Condition.type(data, pd.DataFrame, "data")

        self._bar_type = bar_type
        self._price_precision = price_precision
        self._size_precision = size_precision
        self._data = as_utc_index(data)

        if "volume" not in self._data:
            self._data["volume"] = 1_000_000

    def build_bars_all(self):
        """
        Build bars from all data.

        Returns
        -------
        list[Bar]

        """
        return list(map(self._build_bar,
                        self._data.values,
                        [dt.timestamp() for dt in self._data.index]))

    def build_bars_from(self, int index=0):
        """
        Build bars from the given index (>= 0).

        Returns
        -------
        list[Bar]

        """
        Condition.not_negative_int(index, "index")

        return list(map(self._build_bar,
                        self._data.iloc[index:].values,
                        [dt.timestamp() for dt in self._data.iloc[index:].index]))

    def build_bars_range(self, int start=0, int end=-1):
        """
        Build bars within the given range.

        Returns
        -------
        list[Bar]

        """
        Condition.not_negative_int(start, "start")

        return list(map(self._build_bar,
                        self._data.iloc[start:end].values,
                        [dt.timestamp() for dt in self._data.iloc[start:end].index]))

    cpdef Bar _build_bar(self, double[:] values, double timestamp):
        # Build a bar from the given index and values. The function expects the
        # values to be an ndarray with 5 elements [open, high, low, close, volume].
        return Bar(
            bar_type=self._bar_type,
            open=Price(values[0], self._price_precision),
            high=Price(values[1], self._price_precision),
            low=Price(values[2], self._price_precision),
            close=Price(values[3], self._price_precision),
            volume=Quantity(values[4], self._size_precision),
            ts_event=secs_to_nanos(timestamp),  # TODO(cs): Hardcoded identical for now
            ts_init=secs_to_nanos(timestamp),
        )
