# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import pyarrow as pa

from nautilus_trader.common.messages import ComponentStateChanged
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.core.nautilus_pyo3 import DataTransformer
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import PositionEvent
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.persistence.wranglers_v2 import BarDataWrangler
from nautilus_trader.persistence.wranglers_v2 import OrderBookDeltaDataWrangler
from nautilus_trader.persistence.wranglers_v2 import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers_v2 import TradeTickDataWrangler
from nautilus_trader.serialization.arrow.implementations import account_state
from nautilus_trader.serialization.arrow.implementations import component_events
from nautilus_trader.serialization.arrow.implementations import instruments
from nautilus_trader.serialization.arrow.implementations import order_events
from nautilus_trader.serialization.arrow.implementations import position_events
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA


_ARROW_SERIALIZER: dict[type, Callable] = {}
_ARROW_DESERIALIZER: dict[type, Callable] = {}
_SCHEMAS: dict[type, pa.Schema] = {}


def get_schema(data_cls: type) -> pa.Schema:
    return _SCHEMAS[data_cls]


def list_schemas() -> dict[type, pa.Schema]:
    return _SCHEMAS


def register_arrow(
    data_cls: type,
    schema: pa.Schema | None,
    serializer: Callable | None = None,
    deserializer: Callable | None = None,
) -> None:
    """
    Register a new class for serialization to parquet.

    Parameters
    ----------
    data_cls : type
        The data type to register serialization for.
    serializer : Callable, optional
        The callable to serialize instances of type `cls_type` to something
        parquet can write.
    deserializer : Callable, optional
        The callable to deserialize rows from parquet into `cls_type`.
    schema : pa.Schema, optional
        If the schema cannot be correctly inferred from a subset of the data
        (i.e. if certain values may be missing in the first chunk).
    table : type, optional
        An optional table override for `cls`. Used if `cls` is going to be
        transformed and stored in a table other than its own.

    """
    PyCondition.type(schema, pa.Schema, "schema")
    PyCondition.type_or_none(serializer, Callable, "serializer")
    PyCondition.type_or_none(deserializer, Callable, "deserializer")

    if serializer is not None:
        _ARROW_SERIALIZER[data_cls] = serializer
    if deserializer is not None:
        _ARROW_DESERIALIZER[data_cls] = deserializer
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
    def rust_objects_to_record_batch(data: list[Data], data_cls: type) -> pa.Table | pa.RecordBatch:
        data = sorted(data, key=lambda x: x.ts_init)
        processed = ArrowSerializer._unpack_container_objects(data_cls, data)

        if data_cls == OrderBookDelta:
            pyo3_deltas = OrderBookDelta.to_pyo3_list(processed)
            batch_bytes = DataTransformer.pyo3_order_book_deltas_to_record_batch_bytes(pyo3_deltas)
        elif data_cls == QuoteTick:
            pyo3_quotes = QuoteTick.to_pyo3_list(processed)
            batch_bytes = DataTransformer.pyo3_quote_ticks_to_record_batch_bytes(pyo3_quotes)
        elif data_cls == TradeTick:
            pyo3_trades = TradeTick.to_pyo3_list(processed)
            batch_bytes = DataTransformer.pyo3_trade_ticks_to_record_batch_bytes(pyo3_trades)
        elif data_cls == Bar:
            pyo3_bars = Bar.to_pyo3_list(processed)
            batch_bytes = DataTransformer.pyo3_bars_to_record_batch_bytes(pyo3_bars)
        else:
            batch_bytes = DataTransformer.pyobjects_to_record_batch_bytes(processed)

        reader = pa.ipc.open_stream(BytesIO(batch_bytes))
        table: pa.Table = reader.read_all()
        return table

    @staticmethod
    def serialize(
        data: Data | Event,
        data_cls: type[Data | Event] | None = None,
    ) -> pa.RecordBatch:
        if isinstance(data, GenericData):
            data = data.data
        data_cls = data_cls or type(data)
        if data_cls is None:
            raise RuntimeError("`cls` was `None` when a value was expected")

        delegate = _ARROW_SERIALIZER.get(data_cls)
        if delegate is None:
            if data_cls in RUST_SERIALIZERS:
                return ArrowSerializer.rust_objects_to_record_batch([data], data_cls=data_cls)
            raise TypeError(
                f"Cannot serialize object `{data_cls}`. Register a "
                f"serialization method via `nautilus_trader.persistence.catalog.parquet.serializers.register_parquet()`",
            )

        batch = delegate(data)
        assert isinstance(batch, pa.RecordBatch)
        return batch

    @staticmethod
    def serialize_batch(data: list[Data | Event], data_cls: type[Data | Event]) -> pa.Table:
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
            return ArrowSerializer.rust_objects_to_record_batch(data, data_cls=data_cls)
        batches = [ArrowSerializer.serialize(obj, data_cls) for obj in data]
        return pa.Table.from_batches(batches, schema=batches[0].schema)

    @staticmethod
    def deserialize(data_cls: type, batch: pa.RecordBatch | pa.Table) -> Data:
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
        delegate = _ARROW_DESERIALIZER.get(data_cls)
        if delegate is None:
            if data_cls in RUST_SERIALIZERS:
                if isinstance(batch, pa.RecordBatch):
                    batch = pa.Table.from_batches([batch])
                return ArrowSerializer._deserialize_rust(data_cls=data_cls, table=batch)
            raise TypeError(
                f"Cannot deserialize object `{data_cls}`. Register a "
                f"deserialization method via `arrow.serializer.register_parquet()`",
            )

        return delegate(batch)

    @staticmethod
    def _deserialize_rust(data_cls: type, table: pa.Table) -> list[Data | Event]:
        Wrangler = {
            QuoteTick: QuoteTickDataWrangler,
            TradeTick: TradeTickDataWrangler,
            Bar: BarDataWrangler,
            OrderBookDelta: OrderBookDeltaDataWrangler,
            OrderBookDeltas: OrderBookDeltaDataWrangler,
        }[data_cls]
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
    QuoteTick,
    TradeTick,
    Bar,
    OrderBookDelta,
    OrderBookDeltas,
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
            serializer=make_dict_serializer(NAUTILUS_ARROW_SCHEMA[_data_cls]),
            deserializer=make_dict_deserializer(_data_cls),
        )


# Custom implementations
for instrument_cls in Instrument.__subclasses__():
    register_arrow(
        data_cls=instrument_cls,
        schema=instruments.SCHEMAS[instrument_cls],
        serializer=instruments.serialize,
        deserializer=instruments.deserialize,
    )


register_arrow(
    AccountState,
    schema=account_state.SCHEMA,
    serializer=account_state.serialize,
    deserializer=account_state.deserialize,
)


register_arrow(
    OrderInitialized,
    schema=NAUTILUS_ARROW_SCHEMA[OrderInitialized],
    serializer=order_events.serialize,
    deserializer=order_events.deserialize(OrderInitialized),
)


register_arrow(
    OrderFilled,
    schema=NAUTILUS_ARROW_SCHEMA[OrderFilled],
    serializer=order_events.serialize,
    deserializer=order_events.deserialize(OrderFilled),
)


register_arrow(
    ComponentStateChanged,
    schema=NAUTILUS_ARROW_SCHEMA[ComponentStateChanged],
    serializer=component_events.serialize,
    deserializer=component_events.deserialize(ComponentStateChanged),
)


register_arrow(
    TradingStateChanged,
    schema=NAUTILUS_ARROW_SCHEMA[TradingStateChanged],
    serializer=component_events.serialize,
    deserializer=component_events.deserialize(TradingStateChanged),
)


for position_cls in PositionEvent.__subclasses__():
    register_arrow(
        position_cls,
        schema=position_events.SCHEMAS[position_cls],
        serializer=position_events.serialize,
        deserializer=position_events.deserialize(position_cls),
    )
