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

import re
from typing import Any

from libc.stdint cimport uint64_t

import pandas as pd
import pytz
from msgspec import msgpack

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.serialization.base cimport _OBJECT_FROM_DICT_MAP
from nautilus_trader.serialization.base cimport _OBJECT_TO_DICT_MAP
from nautilus_trader.serialization.base cimport Serializer


cdef tuple[str, int, float, bool] _PRIMITIVES = (str, int, float, bool)


cdef class MsgSpecSerializer(Serializer):
    """
    Provides a serializer for either the 'MessagePack' or 'JSON' specifications.

    Parameters
    ----------
    encoding : Callable
        The msgspec encoding type.
    timestamps_as_str : bool, default False
        If the serializer converts `uint64_t` timestamps to integer strings on serialization,
        and back to `uint64_t` on deserialization.
    timestamps_as_iso8601 : bool, default False
        If the serializer converts `uint64_t` timestamps to ISO 8601 strings on serialization,
        and back to `uint64_t` on deserialization.
    """

    def __init__(
        self,
        encoding,
        bint timestamps_as_str = False,
        bint timestamps_as_iso8601 = False,
    ):
        self._encode = encoding.encode
        self._decode = encoding.decode
        self.timestamps_as_str = timestamps_as_str
        self.timestamps_as_iso8601 = timestamps_as_iso8601

    cpdef bytes serialize(self, object obj):
        """
        Serialize the given object to `MessagePack` specification bytes.

        Parameters
        ----------
        obj : object
            The object to serialize.

        Returns
        -------
        bytes

        Raises
        ------
        RuntimeError
            If `obj` cannot be serialized.

        """
        Condition.not_none(obj, "obj")

        cdef dict obj_dict
        if isinstance(obj, dict):
            obj_dict = obj
        else:
            delegate = _OBJECT_TO_DICT_MAP.get(type(obj).__name__)
            if delegate is None:
                if isinstance(obj, _PRIMITIVES):
                    return self._encode(obj)
                else:
                    raise RuntimeError(f"cannot serialize object: unrecognized type {type(obj)}")
            obj_dict = delegate(obj)

        cdef dict timestamp_kvs = {
            k: v for k, v in obj_dict.items() if k in ("expire_time_ns") or re.match(r"^ts_", k)
        }

        cdef str key
        if self.timestamps_as_iso8601:
            for key, value in timestamp_kvs.items():
                if value is None:
                    continue
                timestamp = pd.Timestamp(value, unit="ns", tz=pytz.utc)
                obj_dict[key] = timestamp.isoformat().replace("+00:00", "Z")
        elif self.timestamps_as_str:
            for key, value in timestamp_kvs.items():
                if value is not None:
                    obj_dict[key] = str(value)

        return self._encode(obj_dict)

    cpdef object deserialize(self, bytes obj_bytes):
        """
        Deserialize the given `MessagePack` specification bytes to an object.

        Parameters
        ----------
        obj_bytes : bytes
            The object bytes to deserialize.

        Returns
        -------
        Instrument

        Raises
        ------
        RuntimeError
            If `obj_bytes` cannot be deserialized.

        """
        Condition.not_none(obj_bytes, "obj_bytes")

        cdef dict obj_dict = self._decode(obj_bytes)  # type: dict[str, Any]
        cdef dict timestamp_kvs = {
            k: v for k, v in obj_dict.items() if k in ("expire_time_ns") or re.match(r"^ts_", k)
        }

        cdef:
            str key
            uint64_t value_uint64
        if self.timestamps_as_iso8601 or self.timestamps_as_str:
            for key, value in timestamp_kvs.items():
                if value is None:
                    continue
                if re.match(r"^\d+$", value):  # Check if value is an integer-like string
                    value_uint64 = int(value)
                    obj_dict[key] = value_uint64
                else:  # Else assume the value is ISO 8601 format
                    value_uint64 = pd.Timestamp(value, tz=pytz.utc).value
                    obj_dict[key] = value_uint64

        cdef str obj_type = obj_dict.get("type")
        if obj_type is None:
            return obj_dict

        delegate = _OBJECT_FROM_DICT_MAP.get(obj_type)
        if delegate is None:
            return obj_dict

        return delegate(obj_dict)
