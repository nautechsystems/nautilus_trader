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

from typing import Any

from msgspec import msgpack

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.serialization.base cimport _OBJECT_FROM_DICT_MAP
from nautilus_trader.serialization.base cimport _OBJECT_TO_DICT_MAP
from nautilus_trader.serialization.base cimport Serializer


cdef class MsgPackSerializer(Serializer):
    """
    Provides a serializer for the `MessagePack` specification.

    Parameters
    ----------
    timestamps_as_str : bool
        If the serializer converts `int64_t` timestamps to `str` on serialization,
        and back to `int64_t` on deserialization.
    """

    def __init__(self, bint timestamps_as_str=False):
        self.timestamps_as_str = timestamps_as_str

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

        delegate = _OBJECT_TO_DICT_MAP.get(type(obj).__name__)
        if delegate is None:
            raise RuntimeError("cannot serialize object: unrecognized type")

        cdef dict obj_dict = delegate(obj)
        if self.timestamps_as_str:
            ts_event = obj_dict.get("ts_event")
            if ts_event is not None:
                obj_dict["ts_event"] = str(ts_event)

            ts_init = obj_dict.get("ts_init")
            if ts_init is not None:
                obj_dict["ts_init"] = str(ts_init)

        return msgpack.encode(obj_dict)

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

        cdef dict obj_dict = msgpack.decode(obj_bytes)  # type: dict[str, Any]
        if self.timestamps_as_str:
            ts_event = obj_dict.get("ts_event")
            if ts_event is not None:
                obj_dict["ts_event"] = int(ts_event)

            ts_init = obj_dict.get("ts_init")
            if ts_init is not None:
                obj_dict["ts_init"] = int(ts_init)

        delegate = _OBJECT_FROM_DICT_MAP.get(obj_dict["type"])
        if delegate is None:
            raise RuntimeError("cannot deserialize object: unrecognized type")

        return delegate(obj_dict)
