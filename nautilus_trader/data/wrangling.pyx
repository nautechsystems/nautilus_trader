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

from decimal import Decimal
import random

import pandas as pd

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport as_utc_index
from nautilus_trader.core.datetime cimport secs_to_nanos
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSideParser
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.identifiers cimport TradeMatchId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class QuoteTickDataWrangler:
    """
    Provides a means of building lists of ticks from the given Pandas DataFrames
    of bid and ask data. Provided data can either be tick data or bar data.
    """

    def __init__(
        self,
        Instrument instrument not None,
        data_quotes: pd.DataFrame=None,
        dict data_bars_bid=None,
        dict data_bars_ask=None,
    ):
        """
        Initialize a new instance of the ``QuoteTickDataWrangler`` class.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the data wrangler.
        data_quotes : pd.DataFrame, optional
            The pd.DataFrame containing the quote tick data.
        data_bars_bid : dict[BarAggregation, pd.DataFrame], optional
            The bars bid data.
        data_bars_ask : dict[BarAggregation, pd.DataFrame], optional
            The bars ask data.

        Raises
        ------
        ValueError
            If data_quotes not type None or DataFrame.
        ValueError
            If data_bars_bid not type None or dict.
        ValueError
            If data_bars_ask not type None or dict.
        ValueError
            If all data is empty.

        """
        Condition.type_or_none(data_quotes, pd.DataFrame, "data_quotes")
        Condition.type_or_none(data_bars_bid, dict, "data_bars_bid")
        Condition.type_or_none(data_bars_ask, dict, "data_bars_ask")

        if data_quotes is not None and not data_quotes.empty:
            self._data_quotes = as_utc_index(data_quotes)
        else:
            Condition.not_none(data_bars_bid, "data_bars_bid")
            Condition.not_none(data_bars_ask, "data_bars_ask")
            self._data_bars_bid = data_bars_bid
            self._data_bars_ask = data_bars_ask

        self.instrument = instrument

        self.processed_data = []
        self.resolution = BarAggregation.DAY

    def pre_process(
        self,
        int instrument_indexer,
        random_seed=None,
        default_volume=Decimal(1_000_000),
    ):
        """
        Pre-process the tick data in preparation for building ticks.

        Parameters
        ----------
        instrument_indexer : int
            The instrument identifier indexer for the built ticks.
        random_seed : int, optional
            The random seed for shuffling order of high and low ticks from bar
            data. If random_seed is None then won't shuffle.
        default_volume : Decimal
            The volume per tick if not available from the data.

        """
        if random_seed is not None:
            Condition.type(random_seed, int, "random_seed")

        if self._data_quotes is not None and not self._data_quotes.empty:
            # Build ticks from data
            self.processed_data = self._data_quotes

            if "bid_size" not in self.processed_data.columns:
                self.processed_data["bid_size"] = default_volume

            if "ask_size" not in self.processed_data.columns:
                self.processed_data["ask_size"] = default_volume

            # Pre-process prices into formatted strings
            price_cols = ["bid", "ask"]
            self._data_quotes[price_cols] = self._data_quotes[price_cols].applymap(lambda x: f'{x:.{self.instrument.price_precision}f}')

            # Pre-process sizes into formatted strings
            size_cols = ["bid_size", "ask_size"]
            self._data_quotes[size_cols] = self._data_quotes[size_cols].applymap(lambda x: f'{x:.{self.instrument.size_precision}f}')

            self.processed_data["instrument_id"] = instrument_indexer
            self.resolution = BarAggregation.TICK
            return

        # Build ticks from highest resolution bar data
        bars_bid = None
        bars_ask = None
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
        Condition.false(bars_bid.empty, "bars_bid.empty")
        Condition.false(bars_ask.empty, "bars_ask.empty")
        Condition.true(all(bars_bid.index) == all(bars_ask.index), "bars_bid.index was != bars_ask.index")
        Condition.true(bars_bid.shape == bars_ask.shape, "bars_bid.shape was != bars_ask.shape")

        # Ensure index is tz-aware UTC
        bars_bid = as_utc_index(bars_bid)
        bars_ask = as_utc_index(bars_ask)

        if "volume" not in bars_bid:
            bars_bid["volume"] = default_volume * 4

        if "volume" not in bars_ask:
            bars_ask["volume"] = default_volume * 4

        cdef dict data_open = {
            "bid": bars_bid["open"],
            "ask": bars_ask["open"],
            "bid_size": bars_bid["volume"] / 4,
            "ask_size": bars_ask["volume"] / 4,
        }

        cdef dict data_high = {
            "bid": bars_bid["high"],
            "ask": bars_ask["high"],
            "bid_size": bars_bid["volume"] / 4,
            "ask_size": bars_ask["volume"] / 4,
        }

        cdef dict data_low = {
            "bid": bars_bid["low"],
            "ask": bars_ask["low"],
            "bid_size": bars_bid["volume"] / 4,
            "ask_size": bars_ask["volume"] / 4,
        }

        cdef dict data_close = {
            "bid": bars_bid["close"],
            "ask": bars_ask["close"],
            "bid_size": bars_bid["volume"] / 4,
            "ask_size": bars_ask["volume"] / 4,
        }

        df_ticks_o = pd.DataFrame(data=data_open)
        df_ticks_h = pd.DataFrame(data=data_high)
        df_ticks_l = pd.DataFrame(data=data_low)
        df_ticks_c = pd.DataFrame(data=data_close)

        # Pre-process prices into formatted strings
        price_cols = ["bid", "ask"]
        df_ticks_o[price_cols] = df_ticks_o[price_cols].applymap(lambda x: f'{x:.{self.instrument.price_precision}f}')
        df_ticks_h[price_cols] = df_ticks_h[price_cols].applymap(lambda x: f'{x:.{self.instrument.price_precision}f}')
        df_ticks_l[price_cols] = df_ticks_l[price_cols].applymap(lambda x: f'{x:.{self.instrument.price_precision}f}')
        df_ticks_c[price_cols] = df_ticks_c[price_cols].applymap(lambda x: f'{x:.{self.instrument.price_precision}f}')

        # Pre-process sizes into formatted strings
        size_cols = ["bid_size", "ask_size"]
        df_ticks_o[size_cols] = df_ticks_o[size_cols].applymap(lambda x: f'{x:.{self.instrument.size_precision}f}')
        df_ticks_h[size_cols] = df_ticks_h[size_cols].applymap(lambda x: f'{x:.{self.instrument.size_precision}f}')
        df_ticks_l[size_cols] = df_ticks_l[size_cols].applymap(lambda x: f'{x:.{self.instrument.size_precision}f}')
        df_ticks_c[size_cols] = df_ticks_c[size_cols].applymap(lambda x: f'{x:.{self.instrument.size_precision}f}')

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

        self.processed_data = df_ticks_final
        self.processed_data["instrument_id"] = instrument_indexer

    def build_ticks(self):
        """
        Build ticks from all data.

        Returns
        -------
        list[QuoteTick]

        """
        return list(map(self._build_tick_from_values,
                        self.processed_data.values,
                        [dt.timestamp() for dt in self.processed_data.index]))

    cpdef QuoteTick _build_tick_from_values(self, str[:] values, double timestamp):
        # Build a quote tick from the given values. The function expects the values to
        # be an ndarray with 4 elements [bid, ask, bid_size, ask_size] of type double.
        return QuoteTick(
            instrument_id=self.instrument.id,
            bid=Price(values[0], self.instrument.price_precision),
            ask=Price(values[1], self.instrument.price_precision),
            bid_size=Quantity(values[2], self.instrument.size_precision),
            ask_size=Quantity(values[3], self.instrument.size_precision),
            ts_event_ns=secs_to_nanos(timestamp),  # TODO(cs): Hardcoded identical for now
            ts_recv_ns=secs_to_nanos(timestamp),
        )


cdef class TradeTickDataWrangler:
    """
    Provides a means of building lists of trade ticks from the given DataFrame
    of data.
    """

    def __init__(self, Instrument instrument not None, data not None: pd.DataFrame):
        """
        Initialize a new instance of the ``TradeTickDataWrangler`` class.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the data wrangler.
        data : pd.DataFrame
            The pd.DataFrame containing the tick data.

        Raises
        ------
        ValueError
            If processed_data not type None or DataFrame.

        """
        Condition.not_none(data, "data")
        Condition.type_or_none(data, pd.DataFrame, "data")
        Condition.false(data.empty, "data was empty")

        self.instrument = instrument
        self._data_trades = as_utc_index(data)

        self.processed_data = []

    def pre_process(self, int instrument_indexer):
        """
        Pre-process the tick data in preparation for building ticks.

        Parameters
        ----------
        instrument_indexer : int
            The instrument identifier indexer for the built ticks.

        """
        processed_trades = pd.DataFrame(index=self._data_trades.index)
        processed_trades["price"] = self._data_trades["price"].apply(lambda x: f'{x:.{self.instrument.price_precision}f}')
        processed_trades["quantity"] = self._data_trades["quantity"].apply(lambda x: f'{x:.{self.instrument.size_precision}f}')
        processed_trades["aggressor_side"] = self._create_side_if_not_exist()
        processed_trades["match_id"] = self._data_trades["trade_id"].apply(str)
        processed_trades["instrument_id"] = instrument_indexer
        self.processed_data = processed_trades

    def _create_side_if_not_exist(self):
        if "side" in self._data_trades.columns:
            return self._data_trades["side"]
        else:
            return self._data_trades["buyer_maker"].apply(lambda x: "SELL" if x is True else "BUY")

    def build_ticks(self):
        """
        Build ticks from all data.

        Returns
        -------
        list[TradeTick]

        """
        return list(map(self._build_tick_from_values,
                        self.processed_data.values,
                        [dt.timestamp() for dt in self.processed_data.index]))

    cpdef TradeTick _build_tick_from_values(self, str[:] values, double timestamp):
        # Build a quote tick from the given values. The function expects the values to
        # be an ndarray with 4 elements [bid, ask, bid_size, ask_size] of type double.
        return TradeTick(
            instrument_id=self.instrument.id,
            price=Price(values[0], self.instrument.price_precision),
            size=Quantity(values[1], self.instrument.size_precision),
            aggressor_side=AggressorSideParser.from_str(values[2]),
            match_id=TradeMatchId(values[3]),
            ts_event_ns=secs_to_nanos(timestamp),  # TODO(cs): Hardcoded identical for now
            ts_recv_ns=secs_to_nanos(timestamp),
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
            open_price=Price(values[0], self._price_precision),
            high_price=Price(values[1], self._price_precision),
            low_price=Price(values[2], self._price_precision),
            close_price=Price(values[3], self._price_precision),
            volume=Quantity(values[4], self._size_precision),
            ts_event_ns=secs_to_nanos(timestamp),  # TODO(cs): Hardcoded identical for now
            ts_recv_ns=secs_to_nanos(timestamp),
        )
