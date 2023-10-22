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

from __future__ import annotations

import abc
from typing import Any, ClassVar

import pandas as pd
import pyarrow as pa

from nautilus_trader.core.nautilus_pyo3 import Bar as RustBar
from nautilus_trader.core.nautilus_pyo3 import BarDataWrangler as RustBarDataWrangler

# fmt: off
from nautilus_trader.core.nautilus_pyo3 import OrderBookDelta as RustOrderBookDelta
from nautilus_trader.core.nautilus_pyo3 import OrderBookDeltaDataWrangler as RustOrderBookDeltaDataWrangler
from nautilus_trader.core.nautilus_pyo3 import QuoteTick as RustQuoteTick
from nautilus_trader.core.nautilus_pyo3 import QuoteTickDataWrangler as RustQuoteTickDataWrangler
from nautilus_trader.core.nautilus_pyo3 import TradeTick as RustTradeTick
from nautilus_trader.core.nautilus_pyo3 import TradeTickDataWrangler as RustTradeTickDataWrangler
from nautilus_trader.model.data import BarType
from nautilus_trader.model.instruments import Instrument


# fmt: on


###################################################################################################
# These classes are only intended to be used under the hood of the ParquetDataCatalog v2 at this stage
###################################################################################################


class WranglerBase(abc.ABC):
    IGNORE_KEYS: ClassVar[set[bytes]] = {b"class", b"pandas"}

    @classmethod
    def from_instrument(cls, instrument: Instrument, **kwargs: Any):
        return cls(  # type: ignore
            instrument_id=instrument.id.value,
            price_precision=instrument.price_precision,
            size_precision=instrument.size_precision,
            **kwargs,
        )

    @classmethod
    def from_schema(cls, schema: pa.Schema):
        def decode(k, v):
            if k in (b"price_precision", b"size_precision"):
                return int(v.decode())
            elif k in (b"instrument_id", b"bar_type"):
                return v.decode()

        metadata = schema.metadata
        return cls(
            **{k.decode(): decode(k, v) for k, v in metadata.items() if k not in cls.IGNORE_KEYS},
        )


class OrderBookDeltaDataWrangler(WranglerBase):
    """
    Provides a means of building lists of Nautilus `OrderBookDelta` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `OrderBookDelta` and
    will not work the same way as the current wranglers which build the legacy `Cython` trade ticks.

    """

    def __init__(
        self,
        instrument_id: str,
        price_precision: int,
        size_precision: int,
    ) -> None:
        self._inner = RustOrderBookDeltaDataWrangler(
            instrument_id=instrument_id,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    def from_arrow(
        self,
        table: pa.Table,
    ) -> list[RustOrderBookDelta]:
        sink = pa.BufferOutputStream()
        writer: pa.RecordBatchStreamWriter = pa.ipc.new_stream(sink, table.schema)
        writer.write_table(table)
        writer.close()

        data: bytes = sink.getvalue().to_pybytes()
        return self._inner.process_record_batches_bytes(data)

    def from_pandas(
        self,
        df: pd.DataFrame,
        ts_init_delta: int = 0,
    ) -> list[RustOrderBookDelta]:
        """
        Process the given pandas.DataFrame into Nautilus `OrderBookDelta` objects.

        Parameters
        ----------
        df : pandas.DataFrame
            The order book deltas data frame to process.
        ts_init_delta : int, default 0
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[OrderBookDelta]
            A list of PyO3 [pyclass] `OrderBookDelta` objects.

        """
        # Rename columns (temporary pre-processing?)
        df = df.rename(
            columns={
                "timestamp": "ts_event",
                "ts_recv": "ts_init",
                "quantity": "size",
            },
        )

        # Scale prices and quantities
        df["price"] = (df["price"] * 1e9).astype(pd.Int64Dtype())
        df["size"] = (df["size"] * 1e9).round().astype(pd.UInt64Dtype())

        df["order_id"] = df["order_id"].astype(pd.UInt64Dtype())

        # Process timestamps
        df["ts_event"] = (
            pd.to_datetime(df["ts_event"], utc=True, format="mixed")
            .dt.tz_localize(None)
            .view("int64")
            .astype("uint64")
        )

        if "ts_init" in df.columns:
            df["ts_init"] = (
                pd.to_datetime(df["ts_init"], utc=True, format="mixed")
                .dt.tz_localize(None)
                .view("int64")
                .astype("uint64")
            )
        else:
            df["ts_init"] = df["ts_event"] + ts_init_delta

        # Reorder the columns and drop index column
        df = df[["price", "size", "aggressor_side", "trade_id", "ts_event", "ts_init"]]
        df = df.reset_index(drop=True)

        table = pa.Table.from_pandas(df)

        return self.from_arrow(table)


class QuoteTickDataWrangler(WranglerBase):
    """
    Provides a means of building lists of Nautilus `QuoteTick` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `QuoteTick` and
    will not work the same way as the current wranglers which build the legacy `Cython` quote ticks.

    """

    def __init__(self, instrument_id: str, price_precision: int, size_precision: int) -> None:
        self._inner = RustQuoteTickDataWrangler(
            instrument_id=instrument_id,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    def from_arrow(
        self,
        table: pa.Table,
    ) -> list[RustQuoteTick]:
        sink = pa.BufferOutputStream()
        writer: pa.RecordBatchStreamWriter = pa.ipc.new_stream(sink, table.schema)
        writer.write_table(table)
        writer.close()

        data: bytes = sink.getvalue().to_pybytes()
        return self._inner.process_record_batches_bytes(data)

    def from_pandas(
        self,
        df: pd.DataFrame,
        default_size: float = 1_000_000.0,
        ts_init_delta: int = 0,
    ) -> list[RustQuoteTick]:
        """
        Process the given `data` into Nautilus `QuoteTick` objects.

        Expects columns ['bid_price', 'ask_price'] with 'timestamp' index.
        Note: The 'bid_size' and 'ask_size' columns are optional, will then use
        the `default_size`.

        Parameters
        ----------
        df : pandas.DataFrame
            The quote tick data frame to process.
        default_size : float, default 1_000_000.0
            The default size for the bid and ask size of each tick (if not provided).
        ts_init_delta : int, default 0
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[RustQuoteTick]
            A list of PyO3 [pyclass] `QuoteTick` objects.

        """
        # Rename columns
        df = df.rename(
            columns={
                "bid": "bid_price",
                "ask": "ask_price",
                "timestamp": "ts_event",
                "ts_recv": "ts_init",
            },
        )

        # Scale prices and quantities
        df["bid_price"] = (df["bid_price"] * 1e9).astype(pd.Int64Dtype())
        df["ask_price"] = (df["ask_price"] * 1e9).astype(pd.Int64Dtype())

        # Create bid_size and ask_size columns
        if "bid_size" in df.columns:
            df["bid_size"] = (df["bid_size"] * 1e9).astype(pd.Int64Dtype())
        else:
            df["bid_size"] = pd.Series([default_size * 1e9] * len(df), dtype=pd.UInt64Dtype())

        if "ask_size" in df.columns:
            df["ask_size"] = (df["ask_size"] * 1e9).astype(pd.Int64Dtype())
        else:
            df["ask_size"] = pd.Series([default_size * 1e9] * len(df), dtype=pd.UInt64Dtype())

        # Process timestamps
        df["ts_event"] = (
            pd.to_datetime(df["ts_event"], utc=True, format="mixed")
            .dt.tz_localize(None)
            .view("int64")
            .astype("uint64")
        )

        if "ts_init" in df.columns:
            df["ts_init"] = (
                pd.to_datetime(df["ts_init"], utc=True, format="mixed")
                .dt.tz_localize(None)
                .view("int64")
                .astype("uint64")
            )
        else:
            df["ts_init"] = df["ts_event"] + ts_init_delta

        # Reorder the columns and drop index column
        df = df[["bid_price", "ask_price", "bid_size", "ask_size", "ts_event", "ts_init"]]
        df = df.reset_index(drop=True)

        table = pa.Table.from_pandas(df)

        return self.from_arrow(table)


class TradeTickDataWrangler(WranglerBase):
    """
    Provides a means of building lists of Nautilus `TradeTick` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `TradeTick` and
    will not work the same way as the current wranglers which build the legacy `Cython` trade ticks.

    """

    def __init__(
        self,
        instrument_id: str,
        price_precision: int,
        size_precision: int,
    ) -> None:
        self._inner = RustTradeTickDataWrangler(
            instrument_id=instrument_id,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    def from_arrow(
        self,
        table: pa.Table,
    ) -> list[RustTradeTick]:
        sink = pa.BufferOutputStream()
        writer: pa.RecordBatchStreamWriter = pa.ipc.new_stream(sink, table.schema)
        writer.write_table(table)
        writer.close()

        data: bytes = sink.getvalue().to_pybytes()
        return self._inner.process_record_batches_bytes(data)

    def from_json(
        self,
        data: list[dict[str, Any]],
    ) -> list[RustTradeTick]:
        return [RustTradeTick.from_dict(d) for d in data]

    def from_pandas(
        self,
        df: pd.DataFrame,
        ts_init_delta: int = 0,
    ) -> list[RustTradeTick]:
        """
        Process the given `data` into Nautilus `TradeTick` objects.

        Parameters
        ----------
        df : pandas.DataFrame
            The trade tick data frame to process.
        ts_init_delta : int, default 0
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[RustTradeTick]
            A list of PyO3 [pyclass] `TradeTick` objects.

        """
        # Rename columns (temporary pre-processing?)
        df = df.rename(
            columns={
                "timestamp": "ts_event",
                "ts_recv": "ts_init",
                "quantity": "size",
                "buyer_maker": "aggressor_side",
            },
        )

        # Scale prices and quantities
        df["price"] = (df["price"] * 1e9).astype(pd.Int64Dtype())
        df["size"] = (df["size"] * 1e9).round().astype(pd.UInt64Dtype())

        df["aggressor_side"] = df["aggressor_side"].map(_map_aggressor_side).astype(pd.UInt8Dtype())
        df["trade_id"] = df["trade_id"].astype(str)

        # Process timestamps
        df["ts_event"] = (
            pd.to_datetime(df["ts_event"], utc=True, format="mixed")
            .dt.tz_localize(None)
            .view("int64")
            .astype("uint64")
        )

        if "ts_init" in df.columns:
            df["ts_init"] = (
                pd.to_datetime(df["ts_init"], utc=True, format="mixed")
                .dt.tz_localize(None)
                .view("int64")
                .astype("uint64")
            )
        else:
            df["ts_init"] = df["ts_event"] + ts_init_delta

        # Reorder the columns and drop index column
        df = df[["price", "size", "aggressor_side", "trade_id", "ts_event", "ts_init"]]
        df = df.reset_index(drop=True)

        table = pa.Table.from_pandas(df)

        return self.from_arrow(table)


def _map_aggressor_side(val: bool) -> int:
    return 1 if val else 2


class BarDataWrangler(WranglerBase):
    IGNORE_KEYS = {b"class", b"pandas", b"instrument_id"}
    """
    Provides a means of building lists of Nautilus `Bar` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `Bar` and
    will not work the same way as the current wranglers which build the legacy `Cython` trade ticks.

    """

    def __init__(
        self,
        bar_type: BarType,
        price_precision: int,
        size_precision: int,
    ) -> None:
        self.bar_type = bar_type
        self._inner = RustBarDataWrangler(
            bar_type=bar_type,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    def from_arrow(
        self,
        table: pa.Table,
    ) -> list[RustBar]:
        sink = pa.BufferOutputStream()
        writer: pa.RecordBatchStreamWriter = pa.ipc.new_stream(sink, table.schema)
        writer.write_table(table)
        writer.close()

        data = sink.getvalue().to_pybytes()
        return self._inner.process_record_batches_bytes(data)

    def from_pandas(
        self,
        df: pd.DataFrame,
        default_volume: float = 1_000_000.0,
        ts_init_delta: int = 0,
    ) -> list[RustBar]:
        """
        Process the given `data` into Nautilus `Bar` objects.

        Parameters
        ----------
        df : pandas.DataFrame
            The bar data frame to process.
        default_volume : float, default 1_000_000.0
            The default volume for each bar (if not provided).
        ts_init_delta : int, default 0
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[RustBar]
            A list of PyO3 [pyclass] `TradeTick` objects.

        """
        # Rename column
        df = df.rename(columns={"timestamp": "ts_event"})

        # Scale prices and quantities
        df["open"] = (df["open"] * 1e9).astype(pd.Int64Dtype())
        df["high"] = (df["high"] * 1e9).astype(pd.Int64Dtype())
        df["low"] = (df["low"] * 1e9).astype(pd.Int64Dtype())
        df["clow"] = (df["close"] * 1e9).astype(pd.Int64Dtype())

        if "volume" not in df.columns:
            df["volume"] = pd.Series([default_volume * 1e9] * len(df), dtype=pd.UInt64Dtype())

        # Process timestamps
        df["ts_event"] = (
            pd.to_datetime(df["ts_event"], utc=True, format="mixed")
            .dt.tz_localize(None)
            .view("int64")
            .astype("uint64")
        )

        if "ts_init" in df.columns:
            df["ts_init"] = (
                pd.to_datetime(df["ts_init"], utc=True, format="mixed")
                .dt.tz_localize(None)
                .view("int64")
                .astype("uint64")
            )
        else:
            df["ts_init"] = df["ts_event"] + ts_init_delta

        # Reorder the columns and drop index column
        df = df[["open", "high", "low", "close", "volume", "ts_event", "ts_init"]]
        df = df.reset_index(drop=True)

        table = pa.Table.from_pandas(df)

        return self.from_arrow(table)
