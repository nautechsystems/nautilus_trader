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

from typing import Callable, Optional, Union

import pyarrow as pa

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.persistence.catalog.parquet.schema import NAUTILUS_PARQUET_SCHEMA


_PARQUET_SERIALIZER: dict[type, Callable] = {}
_PARQUET_DESERIALIZER: dict[type, Callable] = {}
_SCHEMAS: dict[type, pa.Schema] = {}

DATA_OR_EVENTS = Union[Data, Event]


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


def register_parquet(
    cls: type,
    schema: Optional[pa.Schema],
    serializer: Optional[Callable],
    deserializer: Optional[Callable] = None,
    # partition_keys: Optional[tuple[str]] = None,
    # table: Optional[type] = None
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
    PyCondition.type(serializer, Callable, "serializer")
    PyCondition.type_or_none(deserializer, Callable, "deserializer")

    if serializer is not None:
        _PARQUET_SERIALIZER[cls] = serializer
    if deserializer is not None:
        _PARQUET_DESERIALIZER[cls] = deserializer
    if schema is not None:
        _SCHEMAS[cls] = schema


class ParquetSerializer:
    """
    Provides an object serializer for the `Parquet` specification.
    """

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
        if cls is GenericData:
            data = [obj.data for obj in data]

        delegate = _PARQUET_SERIALIZER.get(cls)
        if delegate is None:
            raise TypeError(
                f"Cannot serialize object `{cls}`. Register a "
                f"serialization method via `nautilus_trader.persistence.catalog.parquet.serializers.register_parquet()`",
            )

        table = delegate(data)
        assert isinstance(table, pa.Table)
        return table

    @staticmethod
    def deserialize(cls: type, table: pa.Table):
        """
        Deserialize the given `Parquet` specification bytes to an object.

        Parameters
        ----------
        cls : type
            The type to deserialize to.
        table : pyarrow.Table
            The table to deserialize.

        Returns
        -------
        object

        Raises
        ------
        TypeError
            If `chunk` cannot be deserialized.

        """
        delegate = _PARQUET_DESERIALIZER.get(cls)
        if delegate is None:
            raise TypeError(
                f"Cannot deserialize object `{cls}`. Register a "
                f"deserialization method via `arrow.serializer.register_parquet()`",
            )

        return delegate(table)


def make_dict_serializer(schema: pa.Schema):
    def inner(data: list[DATA_OR_EVENTS]):
        dicts = [d.to_dict(d) for d in data]
        return dicts_to_table(dicts, schema=schema)

    return inner


def dicts_to_table(data: list[dict], schema: pa.Schema) -> pa.Table:
    return pa.Table.from_pylist(data, schema=schema)


RUST_SERIALIZERS = {
    QuoteTick,
    TradeTick,
    OrderBookDelta,
    OrderBookDeltas,
}

assert not set(NAUTILUS_PARQUET_SCHEMA).intersection(RUST_SERIALIZERS)
assert not RUST_SERIALIZERS.intersection(set(NAUTILUS_PARQUET_SCHEMA))

for cls in NAUTILUS_PARQUET_SCHEMA:
    schema = NAUTILUS_PARQUET_SCHEMA[cls]
    register_parquet(cls=cls, schema=schema, serializer=make_dict_serializer(schema))
