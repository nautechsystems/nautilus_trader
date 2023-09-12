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

from io import BytesIO
from typing import Any, Callable, Optional, Union

import pyarrow as pa

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.core.nautilus_pyo3.persistence import DataTransformer
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import PositionEvent
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.persistence.wranglers_v2 import BarDataWrangler
from nautilus_trader.persistence.wranglers_v2 import OrderBookDeltaDataWrangler
from nautilus_trader.persistence.wranglers_v2 import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers_v2 import TradeTickDataWrangler
from nautilus_trader.serialization.arrow.implementations import account_state
from nautilus_trader.serialization.arrow.implementations import instruments
from nautilus_trader.serialization.arrow.implementations import position_events
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA


_ARROW_SERIALIZER: dict[type, Callable] = {}
_ARROW_DESERIALIZER: dict[type, Callable] = {}
_SCHEMAS: dict[type, pa.Schema] = {}

DATA_OR_EVENTS = Union[Data, Event]
TABLE_OR_BATCH = Union[pa.Table, pa.RecordBatch]


def get_schema(cls: type):
    return _SCHEMAS[cls]


def list_schemas():
    return _SCHEMAS


def _clear_all(**kwargs):
    # Used for testing
    global _CLS_TO_TABLE, _SCHEMAS, _PARTITION_KEYS, _CHUNK
    if kwargs.get("force", False):
        _PARTITION_KEYS = {}
        _SCHEMAS = {}
        _CLS_TO_TABLE = {}  # type: dict[type, type]
        _CHUNK = set()


def register_arrow(
    cls: type,
    schema: Optional[pa.Schema],
    serializer: Optional[Callable] = None,
    deserializer: Optional[Callable] = None,
):
    """
    Register a new class for serialization to parquet.

    Parameters
    ----------
    cls : type
        The type to register serialization for.
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
        transformed and stored in a table other than
        its own.

    """
    PyCondition.type(schema, pa.Schema, "schema")
    PyCondition.type_or_none(serializer, Callable, "serializer")
    PyCondition.type_or_none(deserializer, Callable, "deserializer")

    if serializer is not None:
        _ARROW_SERIALIZER[cls] = serializer
    if deserializer is not None:
        _ARROW_DESERIALIZER[cls] = deserializer
    if schema is not None:
        _SCHEMAS[cls] = schema


class ArrowSerializer:
    """
    Serialize nautilus objects to arrow RecordBatches.
    """

    @staticmethod
    def _unpack_container_objects(cls: type, data: list[Any]):
        if cls == OrderBookDeltas:
            return [delta for deltas in data for delta in deltas.deltas]
        return data

    @staticmethod
    def rust_objects_to_record_batch(data: list[Data], cls: type) -> TABLE_OR_BATCH:
        processed = ArrowSerializer._unpack_container_objects(cls, data)
        batches_bytes = DataTransformer.pyobjects_to_batches_bytes(processed)
        reader = pa.ipc.open_stream(BytesIO(batches_bytes))
        table: pa.Table = reader.read_all()
        return table

    @staticmethod
    def serialize(
        data: DATA_OR_EVENTS,
        cls: Optional[type[DATA_OR_EVENTS]] = None,
    ) -> pa.RecordBatch:
        if isinstance(data, GenericData):
            data = data.data
        cls = cls or type(data)
        delegate = _ARROW_SERIALIZER.get(cls)
        if delegate is None:
            if cls in RUST_SERIALIZERS:
                return ArrowSerializer.rust_objects_to_record_batch([data], cls=cls)
            raise TypeError(
                f"Cannot serialize object `{cls}`. Register a "
                f"serialization method via `nautilus_trader.persistence.catalog.parquet.serializers.register_parquet()`",
            )

        batch = delegate(data)
        assert isinstance(batch, pa.RecordBatch)
        return batch

    @staticmethod
    def serialize_batch(data: list[DATA_OR_EVENTS], cls: type[DATA_OR_EVENTS]) -> pa.Table:
        """
        Serialize the given instrument to `Parquet` specification bytes.

        Parameters
        ----------
        data : list[Any]
            The object to serialize.
        cls: type
            The class of the data

        Returns
        -------
        bytes

        Raises
        ------
        TypeError
            If `obj` cannot be serialized.

        """
        if cls in RUST_SERIALIZERS or cls.__name__ in RUST_STR_SERIALIZERS:
            return ArrowSerializer.rust_objects_to_record_batch(data, cls=cls)
        batches = [ArrowSerializer.serialize(obj, cls) for obj in data]
        return pa.Table.from_batches(batches, schema=batches[0].schema)

    @staticmethod
    def deserialize(cls: type, batch: Union[pa.RecordBatch, pa.Table]):
        """
        Deserialize the given `Parquet` specification bytes to an object.

        Parameters
        ----------
        cls : type
            The type to deserialize to.
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
        delegate = _ARROW_DESERIALIZER.get(cls)
        if delegate is None:
            if cls in RUST_SERIALIZERS:
                if isinstance(batch, pa.RecordBatch):
                    batch = pa.Table.from_batches([batch])
                return ArrowSerializer._deserialize_rust(cls=cls, table=batch)
            raise TypeError(
                f"Cannot deserialize object `{cls}`. Register a "
                f"deserialization method via `arrow.serializer.register_parquet()`",
            )

        return delegate(batch)

    @staticmethod
    def _deserialize_rust(cls: type, table: pa.Table) -> list[DATA_OR_EVENTS]:
        Wrangler = {
            QuoteTick: QuoteTickDataWrangler,
            TradeTick: TradeTickDataWrangler,
            Bar: BarDataWrangler,
            OrderBookDelta: OrderBookDeltaDataWrangler,
            OrderBookDeltas: OrderBookDeltaDataWrangler,
        }[cls]
        wrangler = Wrangler.from_schema(table.schema)
        ticks = wrangler.from_arrow(table)
        return ticks


def make_dict_serializer(schema: pa.Schema):
    def inner(data: list[DATA_OR_EVENTS]):
        if not isinstance(data, list):
            data = [data]
        dicts = [d.to_dict(d) for d in data]
        return dicts_to_record_batch(dicts, schema=schema)

    return inner


def make_dict_deserializer(cls):
    def inner(table: pa.Table) -> list[DATA_OR_EVENTS]:
        assert isinstance(table, (pa.Table, pa.RecordBatch))
        return [cls.from_dict(d) for d in table.to_pylist()]

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

for _cls in NAUTILUS_ARROW_SCHEMA:
    if _cls in RUST_SERIALIZERS:
        register_arrow(
            cls=_cls,
            schema=NAUTILUS_ARROW_SCHEMA[_cls],
        )
    else:
        register_arrow(
            cls=_cls,
            schema=NAUTILUS_ARROW_SCHEMA[_cls],
            serializer=make_dict_serializer(NAUTILUS_ARROW_SCHEMA[_cls]),
            deserializer=make_dict_deserializer(_cls),
        )


# Custom implementations
for ins_cls in Instrument.__subclasses__():
    register_arrow(
        cls=ins_cls,
        schema=instruments.SCHEMAS[ins_cls],
        serializer=instruments.serialize,
        deserializer=instruments.deserialize,
    )

register_arrow(
    AccountState,
    schema=account_state.SCHEMA,
    serializer=account_state.serialize,
    deserializer=account_state.deserialize,
)
for pos_cls in PositionEvent.__subclasses__():
    register_arrow(
        pos_cls,
        schema=position_events.SCHEMAS[pos_cls],
        serializer=position_events.serialize,
        deserializer=position_events.deserialize(pos_cls),
    )
