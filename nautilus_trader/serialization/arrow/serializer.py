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

from collections.abc import Callable
from io import BytesIO
from typing import Any
from typing import Union

import pyarrow as pa

from nautilus_trader.common.messages import ComponentStateChanged
from nautilus_trader.common.messages import ShutdownSystem
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import IndexPriceUpdate
from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import PositionEvent
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.persistence.wranglers_v2 import BarDataWranglerV2
from nautilus_trader.persistence.wranglers_v2 import OrderBookDeltaDataWranglerV2
from nautilus_trader.persistence.wranglers_v2 import OrderBookDepth10DataWranglerV2
from nautilus_trader.persistence.wranglers_v2 import QuoteTickDataWranglerV2
from nautilus_trader.persistence.wranglers_v2 import TradeTickDataWranglerV2
from nautilus_trader.serialization.arrow.implementations import account_state
from nautilus_trader.serialization.arrow.implementations import component_commands
from nautilus_trader.serialization.arrow.implementations import component_events
from nautilus_trader.serialization.arrow.implementations import instruments
from nautilus_trader.serialization.arrow.implementations import order_events
from nautilus_trader.serialization.arrow.implementations import position_events
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA


NautilusRustDataType = Union[  # noqa: UP007 (mypy does not like pipe operators)
    nautilus_pyo3.OrderBookDelta,
    nautilus_pyo3.OrderBookDepth10,
    nautilus_pyo3.QuoteTick,
    nautilus_pyo3.TradeTick,
    nautilus_pyo3.Bar,
    nautilus_pyo3.MarkPriceUpdate,
    nautilus_pyo3.IndexPriceUpdate,
    nautilus_pyo3.InstrumentClose,
]

_ARROW_ENCODERS: dict[type, Callable] = {}
_ARROW_DECODERS: dict[type, Callable] = {}
_SCHEMAS: dict[type, pa.Schema] = {}


def get_schema(data_cls: type) -> pa.Schema:
    return _SCHEMAS[data_cls]


def list_schemas() -> dict[type, pa.Schema]:
    return _SCHEMAS


def register_arrow(
    data_cls: type,
    schema: pa.Schema | None,
    encoder: Callable | None = None,
    decoder: Callable | None = None,
) -> None:
    """
    Register a new class for serialization to parquet.

    Parameters
    ----------
    data_cls : type
        The data type to register serialization for.
    schema : pa.Schema or None
        If the schema cannot be correctly inferred from a subset of the data
        (i.e. if certain values may be missing in the first chunk).
    encoder : Callable, optional
        The callable to encode instances of type `cls_type` to Arrow record batches.
    decoder : Callable, optional
        The callable to decode rows from Arrow record batches into `cls_type`.
    table : type, optional
        An optional table override for `cls`. Used if `cls` is going to be
        transformed and stored in a table other than its own.

    """
    PyCondition.type(schema, pa.Schema, "schema")
    PyCondition.type_or_none(encoder, Callable, "encoder")
    PyCondition.type_or_none(decoder, Callable, "decoder")

    if encoder is not None:
        _ARROW_ENCODERS[data_cls] = encoder
    if decoder is not None:
        _ARROW_DECODERS[data_cls] = decoder
    if schema is not None:
        _SCHEMAS[data_cls] = schema


class ArrowSerializer:
    """
    Serialize Nautilus objects to arrow RecordBatches.
    """

    @staticmethod
    def _unpack_container_objects(data_cls: type, data: list[Any]) -> list[Data]:
        if data_cls == OrderBookDeltas:
            return [delta for deltas in data for delta in deltas.deltas]

        return data

    @staticmethod
    def rust_defined_to_record_batch(  # noqa: C901 (too complex)
        data: list[Data],
        data_cls: type,
    ) -> pa.Table | pa.RecordBatch:
        data = sorted(data, key=lambda x: x.ts_init)
        data = ArrowSerializer._unpack_container_objects(data_cls, data)

        match data_cls:
            case nautilus_pyo3.OrderBookDelta:
                batch_bytes = nautilus_pyo3.book_deltas_to_arrow_record_batch_bytes(
                    data,
                )
            case nautilus_pyo3.OrderBookDepth10:
                batch_bytes = nautilus_pyo3.book_depth10_to_arrow_record_batch_bytes(
                    data,
                )
            case nautilus_pyo3.QuoteTick:
                batch_bytes = nautilus_pyo3.quotes_to_arrow_record_batch_bytes(data)
            case nautilus_pyo3.TradeTick:
                batch_bytes = nautilus_pyo3.trades_to_arrow_record_batch_bytes(data)
            case nautilus_pyo3.Bar:
                batch_bytes = nautilus_pyo3.bars_to_arrow_record_batch_bytes(data)
            case _:
                if data_cls == OrderBookDelta or data_cls == OrderBookDeltas:
                    pyo3_deltas = OrderBookDelta.to_pyo3_list(data)
                    batch_bytes = nautilus_pyo3.book_deltas_to_arrow_record_batch_bytes(
                        pyo3_deltas,
                    )
                elif data_cls == QuoteTick:
                    pyo3_quotes = QuoteTick.to_pyo3_list(data)
                    batch_bytes = nautilus_pyo3.quotes_to_arrow_record_batch_bytes(
                        pyo3_quotes,
                    )
                elif data_cls == TradeTick:
                    pyo3_trades = TradeTick.to_pyo3_list(data)
                    batch_bytes = nautilus_pyo3.trades_to_arrow_record_batch_bytes(
                        pyo3_trades,
                    )
                elif data_cls == Bar:
                    pyo3_bars = Bar.to_pyo3_list(data)
                    batch_bytes = nautilus_pyo3.bars_to_arrow_record_batch_bytes(pyo3_bars)
                elif data_cls == MarkPriceUpdate:
                    pyo3_mark_prices = MarkPriceUpdate.to_pyo3_list(data)
                    batch_bytes = nautilus_pyo3.mark_prices_to_arrow_record_batch_bytes(
                        pyo3_mark_prices,
                    )
                elif data_cls == IndexPriceUpdate:
                    pyo3_index_prices = IndexPriceUpdate.to_pyo3_list(data)
                    batch_bytes = nautilus_pyo3.index_prices_to_arrow_record_batch_bytes(
                        pyo3_index_prices,
                    )
                elif data_cls == InstrumentClose:
                    pyo3_instrument_closes = InstrumentClose.to_pyo3_list(data)
                    batch_bytes = nautilus_pyo3.instrument_closes_to_arrow_record_batch_bytes(
                        pyo3_instrument_closes,
                    )
                elif data_cls == OrderBookDepth10:
                    data = [
                        nautilus_pyo3.OrderBookDepth10.from_dict(OrderBookDepth10.to_dict(item))
                        for item in data
                    ]
                    batch_bytes = nautilus_pyo3.book_depth10_to_arrow_record_batch_bytes(
                        data,
                    )
                else:
                    raise RuntimeError(
                        f"Unsupported Rust defined data type for catalog write, was `{data_cls}`",
                    )

        reader = pa.ipc.open_stream(BytesIO(batch_bytes))
        table: pa.Table = reader.read_all()
        return table

    @staticmethod
    def serialize(
        data: Data | Event,
        data_cls: type[Data | Event] | None = None,
    ) -> pa.RecordBatch:
        if isinstance(data, CustomData):
            data = data.data

        data_cls = data_cls or type(data)
        if data_cls is None:
            raise RuntimeError("`cls` was `None` when a value was expected")

        delegate = _ARROW_ENCODERS.get(data_cls)
        if delegate is None:
            if data_cls in RUST_SERIALIZERS:
                return ArrowSerializer.rust_defined_to_record_batch([data], data_cls=data_cls)
            raise TypeError(
                f"Cannot serialize object `{data_cls}`. Register a "
                f"serialization method via `nautilus_trader.serialization.arrow.serializer.register_arrow()`",
            )

        batch = delegate(data)
        assert isinstance(batch, pa.RecordBatch)

        return batch

    @staticmethod
    def serialize_batch(
        data: list[Data | Event] | list[NautilusRustDataType],
        data_cls: type[Data | Event | NautilusRustDataType],
    ) -> pa.Table:
        """
        Serialize the given instrument to `Parquet` specification bytes.

        Parameters
        ----------
        data : list[Any]
            The object to serialize.
        data_cls: type
            The data type for the serialization.

        Returns
        -------
        bytes

        Raises
        ------
        TypeError
            If `obj` cannot be serialized.

        """
        if data_cls in RUST_SERIALIZERS or data_cls.__name__ in RUST_STR_SERIALIZERS:
            return ArrowSerializer.rust_defined_to_record_batch(data, data_cls=data_cls)

        batches = [ArrowSerializer.serialize(obj, data_cls) for obj in data]

        return pa.Table.from_batches(batches, schema=batches[0].schema)

    @staticmethod
    def deserialize(data_cls: type, batch: pa.RecordBatch | pa.Table) -> list[Data | Event]:
        """
        Deserialize the given `Parquet` specification bytes to an object.

        Parameters
        ----------
        data_cls : type
            The data type to deserialize to.
        batch : pyarrow.RecordBatch or pyarrow.Table
            The RecordBatch to deserialize.

        Returns
        -------
        object

        Raises
        ------
        TypeError
            If `chunk` cannot be deserialized.

        """
        delegate = _ARROW_DECODERS.get(data_cls)
        if delegate is None:
            if data_cls in RUST_SERIALIZERS:
                if isinstance(batch, pa.RecordBatch):
                    batch = pa.Table.from_batches([batch])

                return ArrowSerializer._deserialize_rust(data_cls=data_cls, table=batch)
            raise TypeError(
                f"Cannot deserialize object `{data_cls}`. Register a "
                f"deserialization method via `nautilus_trader.serialization.arrow.serializer.register_arrow()`",
            )

        return delegate(batch)

    @staticmethod
    def _deserialize_rust(data_cls: type, table: pa.Table) -> list[Data | Event]:
        Wrangler = {
            OrderBookDelta: OrderBookDeltaDataWranglerV2,
            OrderBookDeltas: OrderBookDeltaDataWranglerV2,
            OrderBookDepth10: OrderBookDepth10DataWranglerV2,
            QuoteTick: QuoteTickDataWranglerV2,
            TradeTick: TradeTickDataWranglerV2,
            Bar: BarDataWranglerV2,
            MarkPriceUpdate: None,
            IndexPriceUpdate: None,
            InstrumentClose: None,
        }[data_cls]

        if Wrangler is None:
            raise NotImplementedError

        wrangler = Wrangler.from_schema(table.schema)
        ticks = wrangler.from_arrow(table)

        return ticks


def make_dict_serializer(schema: pa.Schema) -> Callable[[list[Data | Event]], pa.RecordBatch]:
    def inner(data: list[Data | Event]) -> pa.RecordBatch:
        if not isinstance(data, list):
            data = [data]

        dicts = [d.to_dict(d) for d in data]

        return dicts_to_record_batch(dicts, schema=schema)

    return inner


def make_dict_deserializer(data_cls):
    def inner(table: pa.Table) -> list[Data | Event]:
        assert isinstance(table, pa.Table | pa.RecordBatch)
        return [data_cls.from_dict(d) for d in table.to_pylist()]

    return inner


def dicts_to_record_batch(data: list[dict], schema: pa.Schema) -> pa.RecordBatch:
    try:
        return pa.RecordBatch.from_pylist(data, schema=schema)
    except Exception as e:
        print(e)


RUST_SERIALIZERS = {
    OrderBookDelta,
    OrderBookDeltas,
    OrderBookDepth10,
    QuoteTick,
    TradeTick,
    Bar,
    MarkPriceUpdate,
    IndexPriceUpdate,
    # InstrumentClose,  # TODO: Not implemented yet
}
RUST_STR_SERIALIZERS = {s.__name__ for s in RUST_SERIALIZERS}

# TODO - breaking while we don't have access to rust schemas
# Check we have each type defined only once (rust or python)
# assert not set(NAUTILUS_ARROW_SCHEMA).intersection(RUST_SERIALIZERS)
# assert not RUST_SERIALIZERS.intersection(set(NAUTILUS_ARROW_SCHEMA))

for _data_cls in NAUTILUS_ARROW_SCHEMA:
    if _data_cls in RUST_SERIALIZERS:
        register_arrow(
            data_cls=_data_cls,
            schema=NAUTILUS_ARROW_SCHEMA[_data_cls],
        )
    else:
        register_arrow(
            data_cls=_data_cls,
            schema=NAUTILUS_ARROW_SCHEMA[_data_cls],
            encoder=make_dict_serializer(NAUTILUS_ARROW_SCHEMA[_data_cls]),
            decoder=make_dict_deserializer(_data_cls),
        )


# Custom implementations
for instrument_cls in Instrument.__subclasses__():
    register_arrow(
        data_cls=instrument_cls,
        schema=instruments.SCHEMAS[instrument_cls],
        encoder=instruments.serialize,
        decoder=instruments.deserialize,
    )


register_arrow(
    AccountState,
    schema=account_state.SCHEMA,
    encoder=account_state.serialize,
    decoder=account_state.deserialize,
)


register_arrow(
    OrderInitialized,
    schema=NAUTILUS_ARROW_SCHEMA[OrderInitialized],
    encoder=order_events.serialize,
    decoder=order_events.deserialize(OrderInitialized),
)


register_arrow(
    OrderFilled,
    schema=NAUTILUS_ARROW_SCHEMA[OrderFilled],
    encoder=order_events.serialize,
    decoder=order_events.deserialize(OrderFilled),
)


register_arrow(
    ComponentStateChanged,
    schema=NAUTILUS_ARROW_SCHEMA[ComponentStateChanged],
    encoder=component_events.serialize,
    decoder=component_events.deserialize(ComponentStateChanged),
)


register_arrow(
    ShutdownSystem,
    schema=NAUTILUS_ARROW_SCHEMA[ShutdownSystem],
    encoder=component_commands.serialize,
    decoder=component_commands.deserialize(ShutdownSystem),
)


register_arrow(
    TradingStateChanged,
    schema=NAUTILUS_ARROW_SCHEMA[TradingStateChanged],
    encoder=component_events.serialize,
    decoder=component_events.deserialize(TradingStateChanged),
)


for position_cls in PositionEvent.__subclasses__():
    register_arrow(
        position_cls,
        schema=position_events.SCHEMAS[position_cls],
        encoder=position_events.serialize,
        decoder=position_events.deserialize(position_cls),
    )
