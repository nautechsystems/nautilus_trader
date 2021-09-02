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

from typing import Any

import msgpack

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.serialization.base cimport _OBJECT_FROM_DICT_MAP
from nautilus_trader.serialization.base cimport _OBJECT_TO_DICT_MAP
from nautilus_trader.serialization.base cimport Serializer


cdef class MsgPackSerializer(Serializer):
    """
    Provides a serializer for the `MessagePack` specification.
    """

    cpdef bytes serialize(self, object obj):
        """
        Serialize the given instrument to `MessagePack` specification bytes.

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
            If object cannot be serialized.

        """
        Condition.not_none(obj, "obj")

        delegate = _OBJECT_TO_DICT_MAP.get(type(obj).__name__)
        if delegate is None:
            raise RuntimeError("cannot serialize instrument: unrecognized type")

        return msgpack.packb(delegate(obj))

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
            If object cannot be deserialized.

        """
        Condition.not_none(obj_bytes, "obj_bytes")

        cdef dict unpacked = msgpack.unpackb(obj_bytes)  # type: dict[str, Any]

        delegate = _OBJECT_FROM_DICT_MAP.get(unpacked["type"])
        if delegate is None:
            raise RuntimeError("cannot deserialize instrument: unrecognized type")

        return delegate(unpacked)
