# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import numpy as np
import pandas as pd

from nautilus_trader.model.enums import book_action_from_str
from nautilus_trader.model.enums import order_side_from_str

from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport as_utc_index
from nautilus_trader.core.rust.model cimport FIXED_SCALAR
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport BookAction
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.core.rust.model cimport QuantityRaw
from nautilus_trader.core.rust.model cimport RecordFlag
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport BookOrder
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


BAR_PRICES = ("open", "high", "low", "close")
BAR_COLUMNS = (*BAR_PRICES, "volume")


def preprocess_bar_data(data: pd.DataFrame, is_raw: bool):
    """
    Preprocess financial bar data to a standardized format.

    Ensures the DataFrame index is labeled as "timestamp", converts the index to UTC, removes time zone awareness,
    drops rows with NaN values in critical columns, and optionally scales the data.

    Parameters
    ----------
        data : pd.DataFrame
            The input DataFrame containing financial bar data.
        is_raw : bool
            A flag to determine whether the data should be scaled. If True, scales the data back by FIXED_SCALAR.

    Returns
    -------
        pd.DataFrame: The preprocessed DataFrame with a cleaned and standardized structure.

    """
    # Ensure index is timestamp
    if data.index.name != "timestamp":
        data.index.name = "timestamp"

    # Standardize index to UTC and remove time zone awareness
    data = as_utc_index(data)
    data.index = data.index.tz_localize(None).astype("datetime64[ns]")

    # Drop rows with NaN values in critical columns
    data = data.dropna(subset=BAR_COLUMNS)

    # Scale data if raw (we have to do this now to accommodate high_precision mode)
    if is_raw:
        data[list(BAR_COLUMNS)] = data[list(BAR_COLUMNS)] / FIXED_SCALAR

    return data


def calculate_bar_price_offsets(num_records, timestamp_is_close: bool, offset_interval_ms: int, random_seed=None):
    """
    Calculate and potentially randomize the time offsets for bar prices based on the closeness of the timestamp.

    Parameters
    ----------
        num_records : int
            The number of records for which offsets are to be generated.
        timestamp_is_close : bool
            A flag indicating whether the timestamp is close to the trading time.
        offset_interval_ms : int
            The offset interval in milliseconds to be applied.
        random_seed : Optional[int]
            The seed for random number generation to ensure reproducibility.

    Returns
    -------
        dict: A dictionary with arrays of offsets for open, high, low, and close prices. If random_seed is provided,
              high and low offsets are randomized.
    """
    # Initialize offsets
    offsets = {
        "open": np.full(num_records, np.timedelta64((-3 if timestamp_is_close else 0) * offset_interval_ms, "ms")),
        "high": np.full(num_records, np.timedelta64((-2 if timestamp_is_close else 1) * offset_interval_ms, "ms")),
        "low": np.full(num_records, np.timedelta64((-1 if timestamp_is_close else 2) * offset_interval_ms, "ms")),
        "close": np.full(num_records, np.timedelta64((0 if timestamp_is_close else 3) * offset_interval_ms, "ms")),
    }

    # Randomize high and low if seed is given
    if random_seed is not None:
        local_random = random.Random(random_seed)
        for i in range(num_records):
            if local_random.getrandbits(1):  # With a 50% chance, swap high and low
                offsets["high"][i], offsets["low"][i] = offsets["low"][i], offsets["high"][i]

    return offsets


def calculate_volume_quarter(volume: np.ndarray, precision: int, size_increment: float):
    """
    Convert raw volume data to quarter precision.

    Parameters
    ----------
    volume : np.ndarray
        An array of volume data to be processed.
    precision : int
        The decimal precision to which the volume data is rounded.

    Returns
    -------
    np.ndarray
        The volume data adjusted to quarter precision.

    """
    # Convert volume to quarter precision (respect minimum size increment)
    return np.round(np.maximum(volume / 4.0, size_increment), precision)


def align_bid_ask_bar_data(bid_data: pd.DataFrame, ask_data: pd.DataFrame):
    """
    Merge bid and ask data into a single DataFrame with prefixed column names.

    Parameters
    ----------
    bid_data : pd.DataFrame
        The DataFrame containing bid data.
    ask_data : pd.DataFrame
        The DataFrame containing ask data.

    Returns
    pd.DataFrame
        A merged DataFrame with columns prefixed by 'bid_' for bid data and 'ask_' for ask data, joined on their indexes.

    """
    bid_prefixed = bid_data.add_prefix("bid_")
    ask_prefixed = ask_data.add_prefix("ask_")
    merged_data = pd.merge(bid_prefixed, ask_prefixed, left_index=True, right_index=True, how="inner")
    return merged_data


def prepare_event_and_init_timestamps(
    index: pd.DatetimeIndex,
    ts_init_delta: int,
):
    Condition.type(index, pd.DatetimeIndex, "index")
    Condition.not_negative(ts_init_delta, "ts_init_delta")
    ts_events = index.view(np.uint64)
    ts_inits = ts_events + ts_init_delta
    return ts_events, ts_inits


cdef class OrderBookDeltaDataWrangler:
    """
    Provides a means of building lists of Nautilus `OrderBookDelta` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    """

    def __init__(self, Instrument instrument not None):
        self.instrument = instrument

    def process(self, data: pd.DataFrame, ts_init_delta: int=0, bint is_raw=False):
        """
        Process the given order book dataset into Nautilus `OrderBookDelta` objects.

        Parameters
        ----------
        data : pd.DataFrame
            The data to process.
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system.
        is_raw : bool, default False
            If the data is scaled to Nautilus fixed-point values.

        Raises
        ------
        ValueError
            If `data` is empty.

        """
        Condition.not_none(data, "data")
        Condition.is_false(data.empty, "data.empty")

        data = as_utc_index(data)
        ts_events, ts_inits = prepare_event_and_init_timestamps(data.index, ts_init_delta)

        if is_raw:
            data["price"] /= FIXED_SCALAR
            data["size"] /= FIXED_SCALAR

        cdef list[OrderBookDelta] deltas
        deltas = list(map(
            self._build_delta,
            data["action"].apply(book_action_from_str),
            data["side"].apply(order_side_from_str),
            data["price"],
            data["size"],
            data["order_id"],
            data["flags"],
            data["sequence"],
            ts_events,
            ts_inits,
        ))

        cdef:
            OrderBookDelta first
            OrderBookDelta clear
        if deltas and deltas[0].flags & RecordFlag.F_SNAPSHOT:
            first = deltas[0]
            clear = OrderBookDelta.clear(
                first.instrument_id,
                first.sequence,
                first.ts_event,
                first.ts_init,
            )
            deltas.insert(0, clear)

        return deltas

    # cpdef method for Python wrap() (called with map)
    cpdef OrderBookDelta _build_delta(
        self,
        BookAction action,
        OrderSide side,
        double price,
        double size,
        uint64_t order_id,
        uint8_t flags,
        uint64_t sequence,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        cdef BookOrder order = BookOrder(
            side,
            Price(price, self.instrument.price_precision),
            Quantity(size, self.instrument.size_precision),
            order_id,
        )
        return OrderBookDelta(
            self.instrument.id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )


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
        Process the given tick dataset into Nautilus `QuoteTick` objects.

        Expects columns ['bid_price', 'ask_price'] with 'timestamp' index.
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
        Condition.is_false(data.empty, "data.empty")
        Condition.not_none(default_volume, "default_volume")

        data = as_utc_index(data)

        columns = {
            "bid": "bid_price",
            "ask": "ask_price",
        }
        data.rename(columns=columns, inplace=True)

        if "bid_size" not in data.columns:
            data["bid_size"] = float(default_volume)
        if "ask_size" not in data.columns:
            data["ask_size"] = float(default_volume)

        ts_events, ts_inits = prepare_event_and_init_timestamps(data.index, ts_init_delta)

        return list(map(
            self._build_tick,
            data["bid_price"],
            data["ask_price"],
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
        offset_interval_ms: int = 100,
        bint timestamp_is_close: bool = True,
        random_seed: int | None = None,
        bint is_raw: bool = False,
        bint sort_data: bool = True,
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
        offset_interval_ms : int, default 100
            The number of milliseconds to offset each tick for the bar timestamps.
            If `timestamp_is_close` then will use negative offsets,
            otherwise will use positive offsets (see also `timestamp_is_close`).
        random_seed : int, optional
            The random seed for shuffling order of high and low ticks from bar
            data. If random_seed is ``None`` then won't shuffle.
        is_raw : bool, default False
            If the data is scaled to Nautilus fixed-point values.
        timestamp_is_close : bool, default True
            If bar timestamps are at the close.
            If True, then open, high, low timestamps are offset before the close timestamp.
            If False, then high, low, close timestamps are offset after the open timestamp.
        sort_data : bool, default True
            If the data should be sorted by timestamp.

        """
        Condition.type(bid_data, pd.DataFrame, "bid_data")
        Condition.type(ask_data, pd.DataFrame, "ask_data")
        Condition.is_false(bid_data.empty, "bid_data.empty")
        Condition.is_false(ask_data.empty, "ask_data.empty")
        Condition.type(bid_data.index, pd.DatetimeIndex, "bid_data.index")
        Condition.type(ask_data.index, pd.DatetimeIndex, "ask_data.index")
        Condition.not_none(default_volume, "default_volume")
        for col in BAR_PRICES:
            Condition.is_in(col, bid_data.columns, col, "bid_data.columns")
            Condition.is_in(col, ask_data.columns, col, "ask_data.columns")
        if random_seed is not None:
            Condition.type(random_seed, int, "random_seed")

        # Add default volume if not present
        if "volume" not in bid_data:
            bid_data.loc[:, "volume"] = float(default_volume * 4.0) / (FIXED_SCALAR if is_raw else 1.0)
        if "volume" not in ask_data:
            ask_data.loc[:, "volume"] = float(default_volume * 4.0) / (FIXED_SCALAR if is_raw else 1.0)

        # Standardize and preprocess data
        bid_data = preprocess_bar_data(bid_data, is_raw)
        ask_data = preprocess_bar_data(ask_data, is_raw)

        merged_data = align_bid_ask_bar_data(bid_data, ask_data)
        offsets = calculate_bar_price_offsets(len(merged_data), timestamp_is_close, offset_interval_ms, random_seed)
        ticks_final = self._create_quote_ticks_array(merged_data, is_raw, self.instrument, offsets, ts_init_delta)

        # Sort data by timestamp, if required
        if sort_data:
            sorted_indices = np.argsort(ticks_final["timestamp"])
            ticks_final = ticks_final[sorted_indices]

        ts_events = ticks_final["timestamp"].view(np.uint64)
        ts_inits = ts_events + ts_init_delta

        return QuoteTick.from_raw_arrays_to_list_c(
            self.instrument.id,
            self.instrument.price_precision,
            self.instrument.size_precision,
            ticks_final["bid_price_raw"],
            ticks_final["ask_price_raw"],
            ticks_final["bid_size_raw"],
            ticks_final["ask_size_raw"],
            ts_events,
            ts_inits,
        )

    def _create_quote_ticks_array(
        self,
        merged_data,
        is_raw,
        instrument: Instrument,
        offsets,
        ts_init_delta,
    ):
        dtype = [
            ("bid_price_raw", np.double), ("ask_price_raw", np.double),
            ("bid_size_raw", np.double), ("ask_size_raw", np.double),
            ("timestamp", "datetime64[ns]")
        ]

        size_precision = instrument.size_precision
        size_increment = float(instrument.size_increment)
        merged_data.loc[:, "bid_volume"] = calculate_volume_quarter(merged_data["bid_volume"], size_precision, size_increment)
        merged_data.loc[:, "ask_volume"] = calculate_volume_quarter(merged_data["ask_volume"], size_precision, size_increment)

        # Convert to record array
        records = merged_data.to_records()

        # Create structured array
        total_records = len(records) * 4  # For open, high, low, close
        tick_data = np.empty(total_records, dtype=dtype)

        for i, price_key in enumerate(BAR_PRICES):
            start_index = i * len(records)
            end_index = start_index + len(records)

            tick_data["bid_price_raw"][start_index:end_index] = records[f"bid_{price_key}"].astype(np.double)
            tick_data["ask_price_raw"][start_index:end_index] = records[f"ask_{price_key}"].astype(np.double)
            tick_data["bid_size_raw"][start_index:end_index] = records["bid_volume"].astype(np.double)
            tick_data["ask_size_raw"][start_index:end_index] = records["ask_volume"].astype(np.double)
            tick_data["timestamp"][start_index:end_index] = records["timestamp"] + offsets[price_key]

        return tick_data

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
        return QuoteTick(
            self.instrument.id,
            Price(bid, self.instrument.price_precision),
            Price(ask, self.instrument.price_precision),
            Quantity(bid_size, self.instrument.size_precision),
            Quantity(ask_size, self.instrument.size_precision),
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
            If the data is scaled to Nautilus fixed-point values.

        Raises
        ------
        ValueError
            If `data` is empty.

        """
        Condition.not_none(data, "data")
        Condition.is_false(data.empty, "data.empty")

        data = as_utc_index(data)
        ts_events, ts_inits = prepare_event_and_init_timestamps(data.index, ts_init_delta)

        if is_raw:
            data["price"] /= FIXED_SCALAR
            data["quantity"] /= FIXED_SCALAR

        return list(map(
            self._build_tick,
            data["price"],
            data["quantity"],
            self._create_side_if_not_exist(data),
            data["trade_id"].astype(str),
            ts_events,
            ts_inits,
        ))

    def process_bar_data(
        self,
        data: pd.DataFrame,
        ts_init_delta: int = 0,
        offset_interval_ms: int = 100,
        bint timestamp_is_close: bool = True,
        random_seed: int | None = None,
        bint is_raw: bool = False,
        bint sort_data: bool = True,
    ):
        """
        Process the given bar datasets into Nautilus `QuoteTick` objects.

        Expects columns ['open', 'high', 'low', 'close', 'volume'] with 'timestamp' index.
        Note: The 'volume' column is optional, will then use the `default_volume`.

        Parameters
        ----------
        data : pd.DataFrame
            The trade bar data.
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system.
        offset_interval_ms : int, default 100
            The number of milliseconds to offset each tick for the bar timestamps.
            If `timestamp_is_close` then will use negative offsets,
            otherwise will use positive offsets (see also `timestamp_is_close`).
        random_seed : int, optional
            The random seed for shuffling order of high and low ticks from bar
            data. If random_seed is ``None`` then won't shuffle.
        is_raw : bool, default False
            If the data is scaled to Nautilus fixed-point.
        timestamp_is_close : bool, default True
            If bar timestamps are at the close.
            If True, then open, high, low timestamps are offset before the close timestamp.
            If False, then high, low, close timestamps are offset after the open timestamp.
        sort_data : bool, default True
            If the data should be sorted by timestamp.

        """
        Condition.type(data, pd.DataFrame, "data")
        Condition.is_false(data.empty, "data.empty")
        Condition.type(data.index, pd.DatetimeIndex, "data.index")
        for col in BAR_COLUMNS:
            Condition.is_in(col, data.columns, col, "data.columns")
        if random_seed is not None:
            Condition.type(random_seed, int, "random_seed")

        # Standardize and preprocess data
        data = preprocess_bar_data(data, is_raw)
        size_precision = self.instrument.size_precision
        size_increment = float(self.instrument.size_increment)
        data.loc[:, "volume"] = calculate_volume_quarter(data["volume"], size_precision, size_increment)
        data.loc[:, "trade_id"] = data.index.view(np.uint64).astype(str)

        records = data.to_records()
        offsets = calculate_bar_price_offsets(len(records), timestamp_is_close, offset_interval_ms, random_seed)
        ticks_final = self._create_trade_ticks_array(records, offsets)

        # Sort data by timestamp, if required
        if sort_data:
            sorted_indices = np.argsort(ticks_final["timestamp"])
            ticks_final = ticks_final[sorted_indices]

        ts_events = ticks_final["timestamp"].view(np.uint64)
        ts_inits = ts_events + ts_init_delta

        cdef uint8_t[:] aggressor_sides = np.full(len(ts_events), AggressorSide.NO_AGGRESSOR, dtype=np.uint8)

        return TradeTick.from_raw_arrays_to_list(
            self.instrument.id,
            self.instrument.price_precision,
            self.instrument.size_precision,
            ticks_final["price"],
            ticks_final["size"],
            aggressor_sides,
            ts_events.astype(str).tolist(),
            ts_events,
            ts_inits,
        )

    def _create_trade_ticks_array(
        self,
        records,
        offsets,
    ):
        dtype = [("price", np.double), ("size", np.double), ("timestamp", "datetime64[ns]")]
        tick_data = np.empty(len(records) * 4, dtype=dtype)
        for i, price_key in enumerate(BAR_PRICES):
            start_index = i * len(records)
            end_index = start_index + len(records)
            tick_data["price"][start_index:end_index] = records[price_key].astype(np.double)
            tick_data["size"][start_index:end_index] = records["volume"].astype(np.double)
            tick_data["timestamp"][start_index:end_index] = records["timestamp"] + offsets[price_key]

        return tick_data

    def _create_side_if_not_exist(self, data):
        if "side" in data.columns:
            return data["side"].apply(lambda x: AggressorSide.BUYER if str(x).upper() == "BUY" else AggressorSide.SELLER)
        elif "buyer_maker" in data.columns:
            return data["buyer_maker"].apply(lambda x: AggressorSide.SELLER if x is True else AggressorSide.BUYER)
        else:
            return [AggressorSide.NO_AGGRESSOR] * len(data)

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
        return TradeTick(
            self.instrument.id,
            Price(price, self.instrument.price_precision),
            Quantity(size, self.instrument.size_precision),
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
        Note: The 'volume' column is optional, if one does not exist then will use the `default_volume`.

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
        Condition.is_false(data.empty, "data.empty")
        Condition.not_none(default_volume, "default_volume")

        data = as_utc_index(data)

        if "volume" not in data:
            data["volume"] = float(default_volume)

        ts_events, ts_inits = prepare_event_and_init_timestamps(data.index, ts_init_delta)

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
