# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from copy import copy
from typing import Optional

import numpy as np
import pandas as pd

from libc.stdint cimport int64_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport as_utc_index
from nautilus_trader.core.rust.core cimport secs_to_nanos
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.enums_c cimport AggressorSide
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class QuoteTickDataWrangler:
    """
    Provides a means of building lists of Nautilus `QuoteTick` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.
    """

    def __init__(self, Instrument instrument not None):
        self.instrument = instrument

    def process(
        self,
        data: pd.DataFrame,
        default_volume: float=1_000_000.0,
        ts_init_delta: int=0,
    ):
        """
        Process the give tick dataset into Nautilus `QuoteTick` objects.

        Expects columns ['bid', 'ask'] with 'timestamp' index.
        Note: The 'bid_size' and 'ask_size' columns are optional, will then use
        the `default_volume`.

        Parameters
        ----------
        data : pd.DataFrame
            The tick data to process.
        default_volume : float
            The default volume for each tick (if not provided).
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[QuoteTick]

        """
        Condition.false(data.empty, "data.empty")
        Condition.not_none(default_volume, "default_volume")

        as_utc_index(data)

        if "bid_size" not in data.columns:
            data["bid_size"] = float(default_volume)
        if "ask_size" not in data.columns:
            data["ask_size"] = float(default_volume)

        cdef uint64_t[:] ts_events = np.ascontiguousarray([secs_to_nanos(dt.timestamp()) for dt in data.index], dtype=np.uint64)  # noqa
        cdef uint64_t[:] ts_inits = np.ascontiguousarray([ts_event + ts_init_delta for ts_event in ts_events], dtype=np.uint64)  # noqa

        return list(map(
            self._build_tick,
            data["bid"],
            data["ask"],
            data["bid_size"],
            data["ask_size"],
            ts_events,
            ts_inits,
        ))

    def process_bar_data(
        self,
        bid_data: pd.DataFrame,
        ask_data: pd.DataFrame,
        default_volume: float = 1_000_000.0,
        ts_init_delta: int = 0,
        random_seed: Optional[int] = None,
        bint is_raw: bool = False,
    ):
        """
        Process the given bar datasets into Nautilus `QuoteTick` objects.

        Expects columns ['open', 'high', 'low', 'close', 'volume'] with 'timestamp' index.
        Note: The 'volume' column is optional, will then use the `default_volume`.

        Parameters
        ----------
        bid_data : pd.DataFrame
            The bid bar data.
        ask_data : pd.DataFrame
            The ask bar data.
        default_volume : float
            The volume per tick if not available from the data.
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system.
        random_seed : int, optional
            The random seed for shuffling order of high and low ticks from bar
            data. If random_seed is ``None`` then won't shuffle.
        is_raw : bool, default False
            If the data is scaled to the Nautilus fixed precision.

        """
        Condition.not_none(bid_data, "bid_data")
        Condition.not_none(ask_data, "ask_data")
        Condition.false(bid_data.empty, "bid_data.empty")
        Condition.false(ask_data.empty, "ask_data.empty")
        Condition.not_none(default_volume, "default_volume")
        if random_seed is not None:
            Condition.type(random_seed, int, "random_seed")

        # Ensure index is tz-aware UTC
        bid_data = as_utc_index(bid_data)
        ask_data = as_utc_index(ask_data)

        if "volume" not in bid_data:
            bid_data["volume"] = float(default_volume * 4)

        if "volume" not in ask_data:
            ask_data["volume"] = float(default_volume * 4)

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
        df_ticks_final.dropna(inplace=True)
        df_ticks_final.sort_index(axis=0, kind="mergesort", inplace=True)

        cdef int i
        # Randomly shift high low prices
        if random_seed is not None:
            random.seed(random_seed)
            for i in range(0, len(df_ticks_final), 4):
                if random.getrandbits(1):
                    high = copy(df_ticks_final.iloc[i + 1])
                    low = copy(df_ticks_final.iloc[i + 2])
                    df_ticks_final.iloc[i + 1] = low
                    df_ticks_final.iloc[i + 2] = high

        cdef uint64_t[:] ts_events = np.ascontiguousarray([secs_to_nanos(dt.timestamp()) for dt in df_ticks_final.index], dtype=np.uint64)  # noqa
        cdef uint64_t[:] ts_inits = np.ascontiguousarray([ts_event + ts_init_delta for ts_event in ts_events], dtype=np.uint64)  # noqa

        if is_raw:
            return list(map(
                self._build_tick_from_raw,
                df_ticks_final["bid"],
                df_ticks_final["ask"],
                df_ticks_final["bid_size"],
                df_ticks_final["ask_size"],
                ts_events,
                ts_inits,
            ))
        else:
            return list(map(
                self._build_tick,
                df_ticks_final["bid"],
                df_ticks_final["ask"],
                df_ticks_final["bid_size"],
                df_ticks_final["ask_size"],
                ts_events,
                ts_inits,
            ))

    # cpdef method for Python wrap() (called with map)
    cpdef QuoteTick _build_tick_from_raw(
        self,
        int64_t raw_bid,
        int64_t raw_ask,
        uint64_t raw_bid_size,
        uint64_t raw_ask_size,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        return QuoteTick.from_raw_c(
            self.instrument.id,
            raw_bid,
            raw_ask,
            self.instrument.price_precision,
            self.instrument.price_precision,
            raw_bid_size,
            raw_ask_size,
            self.instrument.size_precision,
            self.instrument.size_precision,
            ts_event,
            ts_init,
        )

    # cpdef method for Python wrap() (called with map)
    cpdef QuoteTick _build_tick(
        self,
        double bid,
        double ask,
        double bid_size,
        double ask_size,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        # Build a quote tick from the given values. The function expects the values to
        # be an ndarray with 4 elements [bid, ask, bid_size, ask_size] of type double.
        return QuoteTick.from_raw_c(
            self.instrument.id,
            int(bid * 1e9),
            int(ask * 1e9),
            self.instrument.price_precision,
            self.instrument.price_precision,
            int(bid_size * 1e9),
            int(ask_size * 1e9),
            self.instrument.size_precision,
            self.instrument.size_precision,
            ts_event,
            ts_init,
        )


cdef class TradeTickDataWrangler:
    """
    Provides a means of building lists of Nautilus `TradeTick` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.
    """

    def __init__(self, Instrument instrument not None):
        self.instrument = instrument

    def process(self, data: pd.DataFrame, ts_init_delta: int=0, bint is_raw=False):
        """
        Process the given trade tick dataset into Nautilus `TradeTick` objects.

        Parameters
        ----------
        data : pd.DataFrame
            The data to process.
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system.
        is_raw : bool, default False
            If the data is scaled to the Nautilus fixed precision.

        Raises
        ------
        ValueError
            If `data` is empty.

        """
        Condition.not_none(data, "data")
        Condition.false(data.empty, "data.empty")

        data = as_utc_index(data)

        cdef uint64_t[:] ts_events = np.ascontiguousarray([secs_to_nanos(dt.timestamp()) for dt in data.index], dtype=np.uint64)  # noqa
        cdef uint64_t[:] ts_inits = np.ascontiguousarray([ts_event + ts_init_delta for ts_event in ts_events], dtype=np.uint64)  # noqa

        if is_raw:
            return list(map(
                self._build_tick_from_raw,
                data["price"],
                data["quantity"],
                self._create_side_if_not_exist(data),
                data["trade_id"].astype(str),
                ts_events,
                ts_inits,
            ))
        else:
            return list(map(
                self._build_tick,
                data["price"],
                data["quantity"],
                self._create_side_if_not_exist(data),
                data["trade_id"].astype(str),
                ts_events,
                ts_inits,
            ))

    def _create_side_if_not_exist(self, data):
        if "side" in data.columns:
            return data["side"].apply(lambda x: AggressorSide.BUYER if str(x).upper() == "BUY" else AggressorSide.SELLER)
        else:
            return data["buyer_maker"].apply(lambda x: AggressorSide.SELLER if x is True else AggressorSide.BUYER)

    # cpdef method for Python wrap() (called with map)
    cpdef TradeTick _build_tick_from_raw(
        self,
        int64_t raw_price,
        uint64_t raw_size,
        AggressorSide aggressor_side,
        str trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        return TradeTick.from_raw_c(
            self.instrument.id,
            raw_price,
            self.instrument.price_precision,
            raw_size,
            self.instrument.size_precision,
            aggressor_side,
            TradeId(trade_id),
            ts_event,
            ts_init,
        )

    # cpdef method for Python wrap() (called with map)
    cpdef TradeTick _build_tick(
        self,
        double price,
        double size,
        AggressorSide aggressor_side,
        str trade_id,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        # Build a quote tick from the given values. The function expects the values to
        # be an ndarray with 4 elements [bid, ask, bid_size, ask_size] of type double.
        return TradeTick.from_raw_c(
            self.instrument.id,
            int(price * 1e9),
            self.instrument.price_precision,
            int(size * 1e9),
            self.instrument.size_precision,
            aggressor_side,
            TradeId(trade_id),
            ts_event,
            ts_init,
        )


cdef class BarDataWrangler:
    """
    Provides a means of building lists of Nautilus `Bar` objects.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the wrangler.
    instrument : Instrument
        The instrument for the wrangler.
    """

    def __init__(
        self,
        BarType bar_type not None,
        Instrument instrument not None,
    ):
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(instrument, "instrument")

        self.bar_type = bar_type
        self.instrument = instrument

    def process(
        self,
        data: pd.DataFrame,
        default_volume: float=1_000_000.0,
        ts_init_delta: int=0,
    ):
        """
        Process the given bar dataset into Nautilus `Bar` objects.

        Expects columns ['open', 'high', 'low', 'close', 'volume'] with 'timestamp' index.
        Note: The 'volume' column is optional, will then use the `default_volume`.

        Parameters
        ----------
        data : pd.DataFrame
            The data to process.
        default_volume : float
            The default volume for each bar (if not provided).
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system.

        Returns
        -------
        list[Bar]

        Raises
        ------
        ValueError
            If `data` is empty.

        """
        Condition.not_none(data, "data")
        Condition.false(data.empty, "data.empty")
        Condition.not_none(default_volume, "default_volume")

        data = as_utc_index(data)

        if "volume" not in data:
            data["volume"] = float(default_volume)

        cdef uint64_t[:] ts_events = np.ascontiguousarray([secs_to_nanos(dt.timestamp()) for dt in data.index], dtype=np.uint64)  # noqa
        cdef uint64_t[:] ts_inits = np.ascontiguousarray([ts_event + ts_init_delta for ts_event in ts_events], dtype=np.uint64)  # noqa

        return list(map(
            self._build_bar,
            data.values,
            ts_events,
            ts_inits
        ))

    # cpdef method for Python wrap() (called with map)
    cpdef Bar _build_bar(self, double[:] values, uint64_t ts_event, uint64_t ts_init):
        # Build a bar from the given index and values. The function expects the
        # values to be an ndarray with 5 elements [open, high, low, close, volume].
        return Bar(
            bar_type=self.bar_type,
            open=Price(values[0], self.instrument.price_precision),
            high=Price(values[1], self.instrument.price_precision),
            low=Price(values[2], self.instrument.price_precision),
            close=Price(values[3], self.instrument.price_precision),
            volume=Quantity(values[4], self.instrument.size_precision),
            ts_event=ts_event,
            ts_init=ts_init,
        )
