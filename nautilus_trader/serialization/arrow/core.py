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

from typing import Callable, Dict, Optional

import pyarrow as pa

from nautilus_trader.serialization.base import get_from_dict
from nautilus_trader.serialization.base import get_to_dict


_PARQUET_OBJECT_TO_DICT_MAP: Dict[str, object] = {}
_PARQUET_OBJECT_FROM_DICT_MAP: Dict[str, object] = {}
_chunk = {}
_partition_keys = {}
_schemas = {}


def register_parquet(
    cls_type,
    serializer: Optional[Callable] = None,
    deserializer: Optional[Callable] = None,
    schema: Optional[pa.Schema] = None,
    partition_keys=None,
    chunk=None,
    **kwargs,
):
    """
    Register a new class for serialization to parquet.

    :param cls_type: The type to register serialization for
    :param serializer (callable): The callable to serialize instances of type `cls_type` to something parquet can write
    :param deserializer (callable): The callable to deserialize rows from parquet into `cls_type`.
    :param schema (dict): Optional dict if the schema cannot be correctly inferred from a subset of the data (ie if
                          certain values may be missing in the first chunk)
    :param chunk (bool): Whether to group objects by timestamp and operate together (Used for complex objects where
                         we write each object as multiple rows in parquet, ie OrderBook or AccountState)
    :param partition_key (optional): Optional partition key for data written to parquet (typically an id)
    """
    assert isinstance(
        cls_type, type
    ), f"`name` should be <str> (i.e. Class.__name__) not {type(cls_type)}: {cls_type}"
    assert serializer is None or isinstance(serializer, Callable), "Serializer must be callable"  # type: ignore
    assert deserializer is None or isinstance(
        deserializer, Callable  # type: ignore
    ), "Deserializer must be callable"
    assert schema is None or isinstance(schema, pa.Schema), "partition_keys must be tuple"
    assert partition_keys is None or isinstance(
        partition_keys, tuple
    ), "partition_keys must be tuple"

    cls_name = cls_type.__name__

    # secret kwarg that allows overriding an existing (de)serialization method.
    if not kwargs.get("force", False):
        if serializer is not None:
            assert (
                cls_name not in _PARQUET_OBJECT_TO_DICT_MAP
            ), f"Serializer already exists for {cls_name}: {_PARQUET_OBJECT_TO_DICT_MAP[cls_name]}"
        if deserializer is not None:
            assert (
                cls_name not in _PARQUET_OBJECT_FROM_DICT_MAP
            ), f"Deserializer already exists for {cls_name}: {_PARQUET_OBJECT_TO_DICT_MAP[cls_name]}"

    if serializer is not None:
        _PARQUET_OBJECT_TO_DICT_MAP[cls_name] = serializer
    if deserializer is not None:
        _PARQUET_OBJECT_FROM_DICT_MAP[cls_name] = deserializer
    if chunk is not None:
        _chunk[cls_name] = chunk
    if partition_keys is not None:
        _partition_keys[cls_name] = partition_keys
    if schema is not None:
        _schemas[cls_name] = schema


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
            e = (
                f"object {type(obj)} cannot be serialized by `arrow.core._serialize`, register a serialization "
                f"method via `arrow.core.register_parquet()`"
            )
            raise TypeError(e)


def _deserialize(cls, chunk):
    name = cls.__name__
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
from nautilus_trader.serialization.arrow.implementations.order_book import order_book_register


order_book_register(func=register_parquet)
