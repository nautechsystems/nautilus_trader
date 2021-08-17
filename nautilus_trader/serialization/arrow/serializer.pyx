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

from typing import Callable, Optional

import pyarrow as pa

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.data.base cimport GenericData
from nautilus_trader.model.data.venue cimport InstrumentClosePrice
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.position cimport PositionChanged
from nautilus_trader.model.events.position cimport PositionClosed
from nautilus_trader.model.events.position cimport PositionOpened
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.data cimport OrderBookData
from nautilus_trader.serialization.base cimport _OBJECT_FROM_DICT_MAP
from nautilus_trader.serialization.base cimport _OBJECT_TO_DICT_MAP

from nautilus_trader.serialization.arrow.implementations import account_state
from nautilus_trader.serialization.arrow.implementations import closing_prices
from nautilus_trader.serialization.arrow.implementations import order_book
from nautilus_trader.serialization.arrow.implementations import order_events
from nautilus_trader.serialization.arrow.implementations import position_events
from nautilus_trader.serialization.arrow.schema import NAUTILUS_PARQUET_SCHEMA


cdef dict _PARQUET_TO_DICT_MAP = {}    # type: dict[type, object]
cdef dict _PARQUET_FROM_DICT_MAP = {}  # type: dict[type, object]
cdef dict _PARTITION_KEYS = {}
cdef dict _SCHEMAS = {}
cdef dict _CLS_TO_TABLE = {}  # type: dict[type, type]
cdef set _CHUNK = set()


def get_partition_keys(cls: type):
    return _PARTITION_KEYS.get(cls)


def get_schema(cls: type):
    return _SCHEMAS[get_cls_table(cls)]


def list_schemas():
    return _SCHEMAS


def get_cls_table(cls: type):
    return _CLS_TO_TABLE.get(cls, cls)


def _clear_all(**kwargs):
    """
    Used for testing
    """
    global _CLS_TO_TABLE, _SCHEMAS, _PARTITION_KEYS, _CHUNK
    if kwargs.get("force", False):
        _PARTITION_KEYS = {}
        _SCHEMAS = {}
        _CLS_TO_TABLE = {}  # type: dict[type, type]
        _CHUNK = set()


def register_parquet(
    type cls,
    serializer: Optional[Callable] = None,
    deserializer: Optional[Callable] = None,
    schema: Optional[pa.Schema] = None,
    tuple partition_keys=None,
    bint chunk=False,
    type table=None,
    **kwargs,
):
    """
    Register a new class for serialization to parquet.

    Parameters
    ----------
    cls : type
        The type to register serialization for.
    serializer : Optional[Callable]
        The callable to serialize instances of type `cls_type` to something parquet can write.
    deserializer : Optional[Callable]
        The callable to deserialize rows from parquet into `cls_type`.
    schema : Optional[pa.Schema]
        If the schema cannot be correctly inferred from a subset of the data
        (i.e. if certain values may be missing in the first chunk).
    partition_keys : tuple, optional
        The partition key for data written to parquet (typically an ID).
    chunk : bool, optional
        Whether to group objects by timestamp and operate together (Used for complex objects where
        we write each object as multiple rows in parquet, ie OrderBook or AccountState).
    table : type, optional
        Optional table override for `cls`. Used if `cls` is going to be transformed and stored in a table other than
        its own. (for example, OrderBookSnapshots are stored as OrderBookDeltas, so we use table=OrderBookDeltas).

    """
    Condition.type_or_none(serializer, Callable, "serializer")
    Condition.type_or_none(deserializer, Callable, "deserializer")
    Condition.type_or_none(schema, pa.Schema, "schema")
    Condition.type_or_none(partition_keys, tuple, "partition_keys")
    Condition.type_or_none(table, type, "table")

    # secret kwarg that allows overriding an existing (de)serialization method.
    if not kwargs.get("force", False):
        if serializer is not None:
            assert (
                cls not in _PARQUET_TO_DICT_MAP
            ), f"Serializer already exists for {cls}: {_PARQUET_TO_DICT_MAP[cls]}"
        if deserializer is not None:
            assert (
                cls not in _PARQUET_FROM_DICT_MAP
            ), f"Deserializer already exists for {cls}: {_PARQUET_TO_DICT_MAP[cls]}"

    if serializer is not None:
        _PARQUET_TO_DICT_MAP[cls] = serializer
    if deserializer is not None:
        _PARQUET_FROM_DICT_MAP[cls] = deserializer
    if partition_keys is not None:
        _PARTITION_KEYS[cls] = partition_keys
    if schema is not None:
        _SCHEMAS[table or cls] = schema
    if chunk:
        _CHUNK.add(cls)
    _CLS_TO_TABLE[cls] = table or cls


cdef class ParquetSerializer:
    """
    Provides an object serializer for the `Parquet` specification.
    """

    @staticmethod
    def serialize(object obj):
        if isinstance(obj, GenericData):
            obj = obj.data
        cdef type cls = type(obj)

        delegate = _PARQUET_TO_DICT_MAP.get(cls)
        if delegate is None:
            delegate = _OBJECT_TO_DICT_MAP.get(cls.__name__)
        if delegate is None:
            raise TypeError(
                f"Cannot serialize object `{cls}`. Please register a "
                f"serialization method via `arrow.serializer.register_parquet()`"
            )

        return delegate(obj)

    @staticmethod
    def deserialize(type cls, chunk):
        delegate = _PARQUET_FROM_DICT_MAP.get(cls)
        if delegate is None:
            delegate = _OBJECT_FROM_DICT_MAP.get(cls.__name__)
        if delegate is None:
            raise TypeError(
                f"Cannot deserialize object `{cls}`. Please register a "
                f"deserialization method via `arrow.serializer.register_parquet()`"
            )

        if cls in _CHUNK:
            return delegate(chunk)
        else:
            return [delegate(c) for c in chunk]


#################################################
# Objects requiring special handling in parquet
#################################################

for cls in OrderBookData.__subclasses__():
    register_parquet(
        cls=cls,
        serializer=order_book.serialize,
        deserializer=order_book.deserialize,
        table=OrderBookData,
        chunk=True,
    )

for cls in Instrument.__subclasses__():
    register_parquet(cls, partition_keys=tuple())

register_parquet(
    AccountState,
    serializer=account_state.serialize,
    deserializer=account_state.deserialize,
    chunk=True,
)
for cls in (PositionOpened, PositionChanged, PositionClosed):
    register_parquet(
        cls,
        serializer=position_events.serialize,
        deserializer=position_events.deserialize,
    )

register_parquet(OrderFilled, serializer=order_events.serialize)
register_parquet(OrderInitialized, serializer=order_events.serialize_order_initialized)
register_parquet(InstrumentClosePrice, serializer=closing_prices.serialize)


# Other defined schemas
for cls, schema in NAUTILUS_PARQUET_SCHEMA.items():
    register_parquet(cls, schema=schema)
