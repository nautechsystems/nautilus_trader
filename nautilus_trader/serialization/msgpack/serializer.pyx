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

import msgpack

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.serialization.base cimport _OBJECT_FROM_DICT_MAP
from nautilus_trader.serialization.base cimport _OBJECT_TO_DICT_MAP
from nautilus_trader.serialization.base cimport CommandSerializer
from nautilus_trader.serialization.base cimport EventSerializer
from nautilus_trader.serialization.base cimport InstrumentSerializer


cdef class MsgPackInstrumentSerializer(InstrumentSerializer):
    """
    Provides an `Instrument` serializer for the `MessagePack` specification.

    """

    cpdef bytes serialize(self, Instrument instrument):
        """
        Serialize the given instrument to `MessagePack` specification bytes.

        Parameters
        ----------
        instrument : Instrument
            The instrument to serialize.

        Returns
        -------
        bytes

        """
        Condition.not_none(instrument, "instrument")

        delegate = _OBJECT_TO_DICT_MAP.get(type(instrument).__name__)
        if delegate is None:
            raise RuntimeError("cannot serialize instrument: unrecognized type")

        return msgpack.packb(delegate(instrument))

    cpdef Instrument deserialize(self, bytes instrument_bytes):
        """
        Deserialize the given `MessagePack` specification bytes to an instrument.

        Parameters
        ----------
        instrument_bytes : bytes
            The instrument bytes to deserialize.

        Returns
        -------
        Instrument

        Raises
        ------
        ValueError
            If instrument_bytes is empty.

        """
        Condition.not_empty(instrument_bytes, "instrument_bytes")

        cdef dict unpacked = msgpack.unpackb(instrument_bytes)  # type: dict[str, object]

        delegate = _OBJECT_FROM_DICT_MAP.get(unpacked["type"])
        if delegate is None:
            raise RuntimeError("cannot deserialize instrument: unrecognized type")

        return delegate(unpacked)


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a `Command` serializer for the MessagePack specification.

    """

    cpdef bytes serialize(self, Command command):
        """
        Return the serialized `MessagePack` specification bytes from the given command.

        Parameters
        ----------
        command : Command
            The command to serialize.

        Returns
        -------
        bytes

        Raises
        ------
        RuntimeError
            If the command cannot be serialized.

        """
        Condition.not_none(command, "command")

        delegate = _OBJECT_TO_DICT_MAP.get(type(command).__name__)
        if delegate is None:
            raise RuntimeError("cannot serialize command: unrecognized type")

        return msgpack.packb(delegate(command))

    cpdef Command deserialize(self, bytes command_bytes):
        """
        Return the command deserialize from the given MessagePack specification command_bytes.

        Parameters
        ----------
        command_bytes : bytes
            The command to deserialize.

        Returns
        -------
        Command

        Raises
        ------
        ValueError
            If command_bytes is empty.
        RuntimeError
            If command cannot be deserialized.

        """
        Condition.not_empty(command_bytes, "command_bytes")

        cdef dict unpacked = msgpack.unpackb(command_bytes)  # type: dict[str, object]

        delegate = _OBJECT_FROM_DICT_MAP.get(unpacked["type"])
        if delegate is None:
            raise RuntimeError("cannot deserialize command: unrecognized type")

        return delegate(unpacked)


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an `Event` serializer for the `MessagePack` specification.

    """

    cpdef bytes serialize(self, Event event):
        """
        Return the MessagePack specification bytes serialized from the given event.

        Parameters
        ----------
        event : Event
            The event to serialize.

        Returns
        -------
        bytes

        Raises
        ------
        RuntimeError
            If the event cannot be serialized.

        """
        Condition.not_none(event, "event")

        delegate = _OBJECT_TO_DICT_MAP.get(type(event).__name__)
        if delegate is None:
            raise RuntimeError("cannot serialize event: unrecognized type")

        return msgpack.packb(delegate(event))

    cpdef Event deserialize(self, bytes event_bytes):
        """
        Return the event deserialized from the given MessagePack specification event_bytes.

        Parameters
        ----------
        event_bytes
            The bytes to deserialize.

        Returns
        -------
        Event

        Raises
        ------
        ValueError
            If event_bytes is empty.
        RuntimeError
            If event cannot be deserialized.

        """
        Condition.not_empty(event_bytes, "event_bytes")

        cdef dict unpacked = msgpack.unpackb(event_bytes)  # type: dict[str, object]

        delegate = _OBJECT_FROM_DICT_MAP.get(unpacked["type"])
        if delegate is None:
            raise RuntimeError("cannot deserialize command: unrecognized type")

        return delegate(unpacked)
