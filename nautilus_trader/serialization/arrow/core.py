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
import pyarrow.parquet as pq

from nautilus_trader.serialization.arrow.schema import SCHEMA_TO_TYPE
from nautilus_trader.serialization.arrow.schema import TYPE_TO_SCHEMA
from nautilus_trader.serialization.arrow.util import list_dicts_to_dict_lists
from nautilus_trader.serialization.arrow.util import maybe_list
from nautilus_trader.serialization.base import get_from_dict
from nautilus_trader.serialization.base import get_to_dict


_PARQUET_OBJECT_TO_DICT_MAP = {}
_PARQUET_OBJECT_FROM_DICT_MAP = {}
_chunk = {}


def register_parquet(
    cls_type,
    serializer: Optional[callable],
    deserializer: Optional[callable],
    chunk=False,
    **kwargs,
):
    assert isinstance(
        cls_type, type
    ), f"`name` should be <str> (i.e. Class.__name__) not {type(cls_type)}: {cls_type}"
    assert serializer is None or isinstance(
        serializer, Callable
    ), "Serializer must be callable"
    assert deserializer is None or isinstance(
        deserializer, Callable
    ), "Deserializer must be callable"

    if not kwargs.get("force", False):
        if serializer is not None:
            assert (
                cls_type.__name__ not in _PARQUET_OBJECT_TO_DICT_MAP
            ), f"Serializer already exists for {cls_type.__name__}: {_PARQUET_OBJECT_TO_DICT_MAP[cls_type.__name__]}"
        if deserializer is not None:
            assert (
                cls_type.__name__ not in _PARQUET_OBJECT_FROM_DICT_MAP
            ), f"Deserializer already exists for {cls_type.__name__}: {_PARQUET_OBJECT_TO_DICT_MAP[cls_type.__name__]}"
    _PARQUET_OBJECT_TO_DICT_MAP[cls_type.__name__] = serializer
    _PARQUET_OBJECT_FROM_DICT_MAP[cls_type.__name__] = deserializer
    _chunk[cls_type.__name__] = chunk


def _serialize(obj):
    name = obj.__class__.__name__
    if name in _PARQUET_OBJECT_TO_DICT_MAP:
        return _PARQUET_OBJECT_TO_DICT_MAP[name](obj)
    elif get_to_dict(name) is not None:
        return get_to_dict(name)(obj)
    else:
        try:
            return obj.__class__.to_dict(obj)
        except (AttributeError, NotImplementedError):
            raise TypeError(
                f"object {obj} cannot be serialized by ArrowSerializer, register a method via `register()`"
            )


def _deserialize(name, chunk):
    name = name.__name__
    if not isinstance(chunk, list):
        chunk = [chunk]
    if name in _PARQUET_OBJECT_FROM_DICT_MAP:
        if _chunk[name]:
            return _PARQUET_OBJECT_FROM_DICT_MAP[name](chunk)
        else:
            return [_PARQUET_OBJECT_FROM_DICT_MAP[name](c) for c in chunk]
    elif get_from_dict(name) is not None:
        return [get_from_dict(name)(c) for c in chunk]
    raise TypeError(
        f"class {name} cannot be deserialized by arrow._deserialize, register a method via `register()`"
    )


# TODO - are these methods even needed?
def to_parquet(buff, objects: list):
    schema = TYPE_TO_SCHEMA[objects[0].__class__]
    mapping = list_dicts_to_dict_lists(
        [x for obj in objects for x in maybe_list(_serialize(obj))]
    )
    table = pa.Table.from_pydict(mapping=mapping, schema=schema)

    with pq.ParquetWriter(buff, schema) as writer:
        writer.write_table(table)
    return buff


def from_parquet(message_bytes, **kwargs):
    metadata = pq.read_metadata(message_bytes)
    cls = SCHEMA_TO_TYPE[metadata.metadata[b"type"]]
    table = pq.read_table(message_bytes, **kwargs)
    data = table.to_pydict()
    values = list(
        map(dict, zip(*([(key, val) for val in data[key]] for key in data.keys())))
    )
    return _deserialize(name=cls, chunk=values)

    # TODO (bm) - Implement for IPC / streaming. See https://arrow.apache.org/docs/python/ipc.html
    # @staticmethod
    # def to_arrow(message: bytes):
    #     """
    #     Serialize the given message to `MessagePack` specification bytes.
    #
    #     Parameters
    #     ----------
    #     message : dict
    #         The message to serialize.
    #
    #     Returns
    #     -------
    #     bytes
    #
    #     """
    #     Condition.not_none(message, "message")
    #
    #     batch = pa.record_batch(data, names=['f0', 'f1', 'f2'])
    #
    #     sink = pa.BufferOutputStream()
    #
    #     writer = pa.ipc.new_stream(sink, batch.schema)
    #     return msgpack.packb(message)
    #
    # @staticmethod
    # def from_arrow(message_bytes: bytes):
    #     """
    #     Deserialize the given `MessagePack` specification bytes to a dictionary.
    #
    #     Parameters
    #     ----------
    #     message_bytes : bytes
    #         The message bytes to deserialize.
    #
    #     Returns
    #     -------
    #     dict[str, object]
    #
    #     """
    #     Condition.not_none(message_bytes, "message_bytes")
    #
    #     return msgpack.unpackb(message_bytes)


# Default nautilus implementations
from nautilus_trader.serialization.arrow.implementations.order_book import (
    order_book_register,
)


order_book_register(func=register_parquet)
