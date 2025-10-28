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

import abc
from typing import Any
from typing import ClassVar

import pandas as pd
import pyarrow as pa

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import FIXED_PRECISION_BYTES
from nautilus_trader.model.objects import FIXED_SCALAR


class WranglerBase(abc.ABC):
    IGNORE_KEYS: ClassVar[set[bytes]] = {b"class", b"pandas"}

    @classmethod
    def from_instrument(
        cls,
        instrument: Instrument,
        **kwargs: Any,
    ) -> Any:
        return cls(  # type: ignore
            instrument_id=instrument.id.value,
            price_precision=instrument.price_precision,
            size_precision=instrument.size_precision,
            **kwargs,
        )

    @classmethod
    def from_schema(
        cls,
        schema: pa.Schema,
    ) -> Any:
        def decode(k, v):
            if k in (b"price_precision", b"size_precision"):
                return int(v.decode())
            elif k in (b"instrument_id", b"bar_type"):
                return v.decode()

        metadata = schema.metadata
        return cls(
            **{k.decode(): decode(k, v) for k, v in metadata.items() if k not in cls.IGNORE_KEYS},
        )


class OrderBookDeltaDataWranglerV2(WranglerBase):
    """
    Provides a means of building lists of Nautilus `OrderBookDelta` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `OrderBookDelta` and
    will not work the same way as the current wranglers which build the legacy `Cython` trades.

    """

    def __init__(
        self,
        instrument_id: str,
        price_precision: int,
        size_precision: int,
    ) -> None:
        self._inner = nautilus_pyo3.OrderBookDeltaDataWrangler(
            instrument_id=instrument_id,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    def from_arrow(
        self,
        table: pa.Table,
    ) -> list[nautilus_pyo3.OrderBookDelta]:
        sink = pa.BufferOutputStream()
        writer: pa.RecordBatchStreamWriter = pa.ipc.new_stream(sink, table.schema)
        writer.write_table(table)
        writer.close()

        data: bytes = sink.getvalue().to_pybytes()
        return self._inner.process_record_batch_bytes(data)

    def from_pandas(
        self,
        df: pd.DataFrame,
        ts_init_delta: int = 0,
    ) -> list[nautilus_pyo3.OrderBookDelta]:
        """
        Process the given pandas DataFrame into Nautilus `OrderBookDelta` objects.

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
        # Rename columns
        expected_columns = {
            "timestamp": "ts_event",
            "ts_recv": "ts_init",
            "quantity": "size",
        }
        df = df.rename(columns=expected_columns)

        if "action" not in df.columns:
            df["action"] = 0
        if "flags" not in df.columns:
            df["flags"] = 0

        # Process timestamps
        ts_event = (
            pd.to_datetime(df["ts_event"], utc=True, format="mixed")
            .dt.tz_localize(None)
            .astype("int64")
        ).to_numpy(dtype="uint64")

        if "ts_init" in df.columns:
            ts_init = (
                pd.to_datetime(df["ts_init"], utc=True, format="mixed")
                .dt.tz_localize(None)
                .astype("int64")
            ).to_numpy(dtype="uint64")
        else:
            ts_init = ts_event + ts_init_delta

        # Convert prices and sizes to fixed binary
        price = (
            (df["price"] * FIXED_SCALAR)
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="big", signed=True))
            .to_numpy()
        )
        size = (
            (df["quantity"] if "quantity" in df else df["size"] * FIXED_SCALAR)
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="big", signed=False))
            .to_numpy()
        )

        # Other uint fields
        order_id = df["order_id"].to_numpy(dtype="uint64")
        sequence = df.index.to_numpy(dtype="uint64")  # Default to index if not provided
        action = df["action"].to_numpy(dtype="uint8")
        flags = df["flags"].to_numpy(dtype="uint8")
        side = df["aggressor_side"].to_numpy(dtype="uint8")

        arrays = [
            pa.array(action, type=pa.uint8()),
            pa.array(side, type=pa.uint8()),
            pa.array(price, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(size, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(order_id, type=pa.uint64()),
            pa.array(flags, type=pa.uint8()),
            pa.array(sequence, type=pa.uint64()),
            pa.array(ts_event, type=pa.uint64()),
            pa.array(ts_init, type=pa.uint64()),
        ]

        fields = [
            pa.field("action", pa.uint8(), nullable=False),
            pa.field("side", pa.uint8(), nullable=False),
            pa.field("price", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("size", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("order_id", pa.uint64(), nullable=False),
            pa.field("flags", pa.uint8(), nullable=False),
            pa.field("sequence", pa.uint64(), nullable=False),
            pa.field("ts_event", pa.uint64(), nullable=False),
            pa.field("ts_init", pa.uint64(), nullable=False),
        ]

        table = pa.Table.from_arrays(arrays, schema=pa.schema(fields))

        return self.from_arrow(table)


class OrderBookDepth10DataWranglerV2(WranglerBase):
    """
    Provides a means of building lists of Nautilus `OrderBookDepth10` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `OrderBookDepth10` and
    will not work the same way as the current wranglers which build the legacy `Cython` trades.

    """

    def __init__(
        self,
        instrument_id: str,
        price_precision: int,
        size_precision: int,
    ) -> None:
        self._inner = nautilus_pyo3.OrderBookDepth10DataWrangler(
            instrument_id=instrument_id,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    def from_arrow(
        self,
        table: pa.Table,
    ) -> list[nautilus_pyo3.OrderBookDepth10]:
        sink = pa.BufferOutputStream()
        writer: pa.RecordBatchStreamWriter = pa.ipc.new_stream(sink, table.schema)
        writer.write_table(table)
        writer.close()

        data: bytes = sink.getvalue().to_pybytes()
        return self._inner.process_record_batch_bytes(data)

    def _process_price_column(self, df: pd.DataFrame, col_name: str, default_bytes: bytes) -> list:
        """
        Process a price column from the DataFrame.
        """
        if col_name in df.columns:
            return (
                df[col_name]
                .apply(lambda x: int(x * FIXED_SCALAR))
                .apply(
                    lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="big", signed=True),
                )
                .to_numpy()
            )
        else:
            return [default_bytes] * len(df)

    def _process_size_column(self, df: pd.DataFrame, col_name: str, default_bytes: bytes) -> list:
        """
        Process a size column from the DataFrame.
        """
        if col_name in df.columns:
            return (
                df[col_name]
                .apply(lambda x: int(x * FIXED_SCALAR))
                .apply(
                    lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="big", signed=False),
                )
                .to_numpy()
            )
        else:
            return [default_bytes] * len(df)

    def _process_count_column(self, df: pd.DataFrame, col_name: str) -> list:
        """
        Process a count column from the DataFrame.
        """
        if col_name in df.columns:
            return df[col_name].to_numpy(dtype="uint32").tolist()
        else:
            return [1] * len(df)

    def from_pandas(
        self,
        df: pd.DataFrame,
        ts_init_delta: int = 0,
    ) -> list[nautilus_pyo3.OrderBookDepth10]:
        """
        Process the given pandas DataFrame into Nautilus `OrderBookDepth10` objects.

        Parameters
        ----------
        df : pandas.DataFrame
            The order book depth data frame to process. Expected columns:
            - bid_price_0 to bid_price_9: Bid prices for each level
            - ask_price_0 to ask_price_9: Ask prices for each level
            - bid_size_0 to bid_size_9: Bid sizes for each level
            - ask_size_0 to ask_size_9: Ask sizes for each level
            - bid_count_0 to bid_count_9: Number of orders at each bid level (optional)
            - ask_count_0 to ask_count_9: Number of orders at each ask level (optional)
            - flags: Flags field (optional)
            - sequence: Sequence number (optional, uses index if not provided)
            - timestamp or ts_event: Event timestamp
            - ts_recv or ts_init: Initialization timestamp (optional)
        ts_init_delta : int, default 0
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[OrderBookDepth10]
            A list of PyO3 [pyclass] `OrderBookDepth10` objects.

        """
        # Rename columns
        expected_columns = {
            "timestamp": "ts_event",
            "ts_recv": "ts_init",
        }
        df = df.rename(columns=expected_columns)

        if "flags" not in df.columns:
            df["flags"] = 0
        if "sequence" not in df.columns:
            df["sequence"] = df.index

        # Process timestamps
        ts_event = (
            pd.to_datetime(df["ts_event"], utc=True, format="mixed")
            .dt.tz_localize(None)
            .astype("int64")
        ).to_numpy(dtype="uint64")

        if "ts_init" in df.columns:
            ts_init = (
                pd.to_datetime(df["ts_init"], utc=True, format="mixed")
                .dt.tz_localize(None)
                .astype("int64")
            ).to_numpy(dtype="uint64")
        else:
            ts_init = ts_event + ts_init_delta

        # Process metadata fields
        flags = df["flags"].to_numpy(dtype="uint8")
        sequence = df["sequence"].to_numpy(dtype="uint64")

        # Build arrays for Arrow table
        arrays = []
        fields = []

        # Create default zero bytes for missing values
        zero_price_bytes = (0).to_bytes(FIXED_PRECISION_BYTES, byteorder="big", signed=True)
        zero_size_bytes = (0).to_bytes(FIXED_PRECISION_BYTES, byteorder="big", signed=False)

        # Process all price and size columns
        depth = 10

        for idx in range(depth):
            # Process bid price
            col_name = f"bid_price_{idx}"
            bid_price = self._process_price_column(df, col_name, zero_price_bytes)
            arrays.append(pa.array(bid_price, type=pa.binary(FIXED_PRECISION_BYTES)))
            fields.append(pa.field(col_name, pa.binary(FIXED_PRECISION_BYTES), nullable=False))

        for idx in range(depth):
            # Process ask price
            col_name = f"ask_price_{idx}"
            ask_price = self._process_price_column(df, col_name, zero_price_bytes)
            arrays.append(pa.array(ask_price, type=pa.binary(FIXED_PRECISION_BYTES)))
            fields.append(pa.field(col_name, pa.binary(FIXED_PRECISION_BYTES), nullable=False))

        for idx in range(depth):
            # Process bid size
            col_name = f"bid_size_{idx}"
            bid_size = self._process_size_column(df, col_name, zero_size_bytes)
            arrays.append(pa.array(bid_size, type=pa.binary(FIXED_PRECISION_BYTES)))
            fields.append(pa.field(col_name, pa.binary(FIXED_PRECISION_BYTES), nullable=False))

        for idx in range(depth):
            # Process ask size
            col_name = f"ask_size_{idx}"
            ask_size = self._process_size_column(df, col_name, zero_size_bytes)
            arrays.append(pa.array(ask_size, type=pa.binary(FIXED_PRECISION_BYTES)))
            fields.append(pa.field(col_name, pa.binary(FIXED_PRECISION_BYTES), nullable=False))

        for idx in range(depth):
            # Process bid count
            col_name = f"bid_count_{idx}"
            bid_count = self._process_count_column(df, col_name)
            arrays.append(pa.array(bid_count, type=pa.uint32()))
            fields.append(pa.field(col_name, pa.uint32(), nullable=False))

        for idx in range(depth):
            # Process ask count
            col_name = f"ask_count_{idx}"
            ask_count = self._process_count_column(df, col_name)
            arrays.append(pa.array(ask_count, type=pa.uint32()))
            fields.append(pa.field(col_name, pa.uint32(), nullable=False))

        # Add metadata fields at the end
        arrays.extend(
            [
                pa.array(flags, type=pa.uint8()),
                pa.array(sequence, type=pa.uint64()),
                pa.array(ts_event, type=pa.uint64()),
                pa.array(ts_init, type=pa.uint64()),
            ],
        )

        fields.extend(
            [
                pa.field("flags", pa.uint8(), nullable=False),
                pa.field("sequence", pa.uint64(), nullable=False),
                pa.field("ts_event", pa.uint64(), nullable=False),
                pa.field("ts_init", pa.uint64(), nullable=False),
            ],
        )

        table = pa.Table.from_arrays(arrays, schema=pa.schema(fields))
        return self.from_arrow(table)


class QuoteTickDataWranglerV2(WranglerBase):
    """
    Provides a means of building lists of Nautilus `QuoteTick` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `QuoteTick` and
    will not work the same way as the current wranglers which build the legacy `Cython` quotes.

    """

    def __init__(self, instrument_id: str, price_precision: int, size_precision: int) -> None:
        self._inner = nautilus_pyo3.QuoteTickDataWrangler(
            instrument_id=instrument_id,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    def from_arrow(
        self,
        table: pa.Table,
    ) -> list[nautilus_pyo3.QuoteTick]:
        sink = pa.BufferOutputStream()
        writer: pa.RecordBatchStreamWriter = pa.ipc.new_stream(sink, table.schema)
        writer.write_table(table)
        writer.close()

        data: bytes = sink.getvalue().to_pybytes()
        return self._inner.process_record_batch_bytes(data)

    def from_pandas(
        self,
        df: pd.DataFrame,
        default_size: float = 1_000_000.0,
        ts_init_delta: int = 0,
    ) -> list[nautilus_pyo3.QuoteTick]:
        """
        Process the given pandas DataFrame into Nautilus `QuoteTick` objects.

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
        list[nautilus_pyo3.QuoteTick]
            A list of PyO3 [pyclass] `QuoteTick` objects.

        """
        # Rename columns
        expected_columns = {
            "bid": "bid_price",
            "ask": "ask_price",
            "timestamp": "ts_event",
            "ts_recv": "ts_init",
        }
        df = df.rename(columns=expected_columns)

        if "bid_size" not in df.columns:
            df["bid_size"] = default_size
        if "ask_size" not in df.columns:
            df["ask_size"] = default_size

        # Process timestamps
        ts_event = (
            pd.to_datetime(df["ts_event"], utc=True, format="mixed")
            .dt.tz_localize(None)
            .astype("int64")
        ).to_numpy(dtype="uint64")

        if "ts_init" in df.columns:
            ts_init = (
                pd.to_datetime(df["ts_init"], utc=True, format="mixed")
                .dt.tz_localize(None)
                .astype("int64")
            ).to_numpy(dtype="uint64")
        else:
            ts_init = ts_event + ts_init_delta

        # Convert prices and sizes to fixed binary
        bid_price = (
            df["bid_price"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=True))
            .to_numpy()
        )
        ask_price = (
            df["ask_price"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=True))
            .to_numpy()
        )
        bid_size = (
            df["bid_size"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=False))
            .to_numpy()
        )
        ask_size = (
            df["ask_size"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=False))
            .to_numpy()
        )

        fields = [
            pa.field("bid_price", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("ask_price", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("bid_size", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("ask_size", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("ts_event", pa.uint64(), nullable=False),
            pa.field("ts_init", pa.uint64(), nullable=False),
        ]

        arrays = [
            pa.array(bid_price, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(ask_price, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(bid_size, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(ask_size, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(ts_event, type=pa.uint64()),
            pa.array(ts_init, type=pa.uint64()),
        ]

        table = pa.Table.from_arrays(arrays, schema=pa.schema(fields))
        return self.from_arrow(table)


class TradeTickDataWranglerV2(WranglerBase):
    """
    Provides a means of building lists of Nautilus `TradeTick` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `TradeTick` and
    will not work the same way as the current wranglers which build the legacy `Cython` trades.

    """

    def __init__(
        self,
        instrument_id: str,
        price_precision: int,
        size_precision: int,
    ) -> None:
        self._inner = nautilus_pyo3.TradeTickDataWrangler(
            instrument_id=instrument_id,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    def from_arrow(
        self,
        table: pa.Table,
    ) -> list[nautilus_pyo3.TradeTick]:
        sink = pa.BufferOutputStream()
        writer: pa.RecordBatchStreamWriter = pa.ipc.new_stream(sink, table.schema)
        writer.write_table(table)
        writer.close()

        data: bytes = sink.getvalue().to_pybytes()
        return self._inner.process_record_batch_bytes(data)

    def from_json(
        self,
        data: list[dict[str, Any]],
    ) -> list[nautilus_pyo3.TradeTick]:
        return [nautilus_pyo3.TradeTick.from_dict(d) for d in data]

    def from_pandas(
        self,
        df: pd.DataFrame,
        ts_init_delta: int = 0,
    ) -> list[nautilus_pyo3.TradeTick]:
        """
        Process the given pandas DataFrame into Nautilus `TradeTick` objects.

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
        list[nautilus_pyo3.TradeTick]
            A list of PyO3 [pyclass] `TradeTick` objects.

        """
        # Rename columns
        expected_columns = {
            "timestamp": "ts_event",
            "ts_recv": "ts_init",
            "quantity": "size",
            "buyer_maker": "aggressor_side",
        }
        df = df.rename(columns=expected_columns)

        # Process timestamps
        ts_event = (
            pd.to_datetime(df["ts_event"], utc=True, format="mixed")
            .dt.tz_localize(None)
            .astype("int64")
        ).to_numpy(dtype="uint64")

        if "ts_init" in df.columns:
            ts_init = (
                pd.to_datetime(df["ts_init"], utc=True, format="mixed")
                .dt.tz_localize(None)
                .astype("int64")
            ).to_numpy(dtype="uint64")
        else:
            ts_init = ts_event + ts_init_delta

        # Convert prices and sizes to fixed binary
        price = (
            df["price"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=True))
        )

        size = (
            df["size"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=False))
        )

        aggressor_side = df["aggressor_side"].map(_map_aggressor_side)
        trade_id = df["trade_id"].astype(str)

        fields = [
            pa.field("price", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("size", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("aggressor_side", pa.uint8(), nullable=False),
            pa.field("trade_id", pa.string(), nullable=False),
            pa.field("ts_event", pa.uint64(), nullable=False),
            pa.field("ts_init", pa.uint64(), nullable=False),
        ]

        arrays = [
            pa.array(price, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(size, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(aggressor_side, type=pa.uint8()),
            pa.array(trade_id, type=pa.string()),
            pa.array(ts_event, type=pa.uint64()),
            pa.array(ts_init, type=pa.uint64()),
        ]

        table = pa.Table.from_arrays(arrays, schema=pa.schema(fields))
        return self.from_arrow(table)


def _map_aggressor_side(val: bool) -> int:
    return 1 if val else 2


class BarDataWranglerV2(WranglerBase):
    IGNORE_KEYS = {b"class", b"pandas", b"instrument_id"}
    """
    Provides a means of building lists of Nautilus `Bar` objects.

    Parameters
    ----------
    bar_type : str
        The bar type for the data wrangler. For example,
        "GBP/USD.SIM-1-MINUTE-BID-EXTERNAL"
    price_precision: int
        The price precision for the data wrangler.
    size_precision: int
        The size precision for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `Bar` and
    will not work the same way as the current wranglers which build the legacy `Cython` trades.

    """

    def __init__(
        self,
        bar_type: str,
        price_precision: int,
        size_precision: int,
    ) -> None:
        self.bar_type = bar_type
        self._inner = nautilus_pyo3.BarDataWrangler(
            bar_type=bar_type,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    def from_arrow(
        self,
        table: pa.Table,
    ) -> list[nautilus_pyo3.Bar]:
        sink = pa.BufferOutputStream()
        writer: pa.RecordBatchStreamWriter = pa.ipc.new_stream(sink, table.schema)
        writer.write_table(table)
        writer.close()

        data = sink.getvalue().to_pybytes()
        return self._inner.process_record_batch_bytes(data)

    def from_pandas(
        self,
        df: pd.DataFrame,
        default_volume: float = 1_000_000.0,
        ts_init_delta: int = 0,
    ) -> list[nautilus_pyo3.Bar]:
        """
        Process the given pandas DataFrame into Nautilus `Bar` objects.

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
        list[nautilus_pyo3.Bar]
            A list of PyO3 [pyclass] `Bar` objects.

        """
        # Rename columns
        expected_columns = {
            "timestamp": "ts_event",
        }
        df = df.rename(columns=expected_columns)

        # Handle default volume
        if "volume" not in df.columns:
            df["volume"] = default_volume

        # Process timestamps
        ts_event = (
            pd.to_datetime(df["ts_event"], utc=True, format="mixed")
            .dt.tz_localize(None)
            .astype("int64")
        ).to_numpy(dtype="uint64")

        if "ts_init" in df.columns:
            ts_init = (
                pd.to_datetime(df["ts_init"], utc=True, format="mixed")
                .dt.tz_localize(None)
                .astype("int64")
            ).to_numpy(dtype="uint64")
        else:
            ts_init = ts_event + ts_init_delta

        # Convert prices and sizes to fixed binary
        open_price = (
            df["open"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=True))
        )
        high_price = (
            df["high"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=True))
        )
        low_price = (
            df["low"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=True))
        )
        close_price = (
            df["close"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=True))
        )
        volume = (
            df["volume"]
            .apply(lambda x: int(x * FIXED_SCALAR))
            .apply(lambda x: x.to_bytes(FIXED_PRECISION_BYTES, byteorder="little", signed=False))
        )

        fields = [
            pa.field("open", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("high", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("low", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("close", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("volume", pa.binary(FIXED_PRECISION_BYTES), nullable=False),
            pa.field("ts_event", pa.uint64(), nullable=False),
            pa.field("ts_init", pa.uint64(), nullable=False),
        ]

        arrays = [
            pa.array(open_price, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(high_price, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(low_price, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(close_price, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(volume, type=pa.binary(FIXED_PRECISION_BYTES)),
            pa.array(ts_event, type=pa.uint64()),
            pa.array(ts_init, type=pa.uint64()),
        ]

        table = pa.Table.from_arrays(arrays, schema=pa.schema(fields))
        return self.from_arrow(table)
