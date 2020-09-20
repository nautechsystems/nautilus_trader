# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.message cimport Request
from nautilus_trader.core.message cimport Response
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.order cimport Order


cdef class Serializer:
    """
    The base class for all serializers.
    """

    def __init__(self):
        """
        Initialize a new instance of the Serializer class.
        """
        self._re_camel_to_snake = re.compile(r'(?<!^)(?=[A-Z])')

    cdef str convert_camel_to_snake(self, str value):
        return self._re_camel_to_snake.sub('_', value).upper()

    cdef str convert_snake_to_camel(self, str value):
        cdef list components = value.split('_')
        cdef str x
        return ''.join(x.title() for x in components)

    cpdef str py_convert_camel_to_snake(self, str value):
        return self.convert_camel_to_snake(value)

    cpdef str py_convert_snake_to_camel(self, str value):
        return self.convert_snake_to_camel(value)


cdef class DictionarySerializer(Serializer):
    """
    The base class for all dictionary serializers.
    """

    def __init__(self):
        """
        Initialize a new instance of the DictionarySerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, dict dictionary):
        """
        Serialize the given dictionary with string keys and values to bytes.

        :param dictionary: The dictionary to serialize.
        :return bytes.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict deserialize(self, bytes dictionary_bytes):
        """
        Deserialize the given bytes to a dictionary with string keys and values.

        :param dictionary_bytes: The dictionary bytes to deserialize.
        :return Dict.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class DataSerializer(Serializer):
    """
    The base class for all data serializers.
    """

    def __init__(self):
        """
        Initialize a new instance of the DataSerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, dict data):
        """
        Serialize the given data mapping to bytes.

        :param data: The data to serialize.
        :return bytes.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict deserialize(self, bytes data_bytes):
        """
        Deserialize the given bytes to a mapping of data.

        :param data_bytes: The data bytes to deserialize.
        :return Dict.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class InstrumentSerializer(Serializer):
    """
    The base class for all instrument serializers.
    """

    def __init__(self):
        """
        Initialize a new instance of the InstrumentSerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, Instrument instrument):
        """
        Serialize the given event to bytes.

        :param instrument: The instrument to serialize.
        :return bytes.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Instrument deserialize(self, bytes instrument_bytes):
        """
        Deserialize the given instrument bytes to an instrument.

        :param instrument_bytes: The bytes to deserialize.
        :return Instrument.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class OrderSerializer(Serializer):
    """
    The base class for all order serializers.
    """

    def __init__(self):
        """
        Initialize a new instance of the OrderSerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, Order order):
        """
        Serialize the given order to bytes.

        :param order: The order to serialize.
        :return bytes.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Order deserialize(self, bytes order_bytes):
        """
        Deserialize the given bytes to an order.

        :param order_bytes: The bytes to deserialize.
        :return Order.
        """
        raise NotImplementedError("method must be implemented in the subclass ")


cdef class CommandSerializer(Serializer):
    """
    The base class for all command serializers.
    """

    def __init__(self):
        """
        Initialize a new instance of the CommandSerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, Command command):
        """
        Serialize the given command to bytes.

        :param command: The command to serialize.
        :return bytes.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Command deserialize(self, bytes command_bytes):
        """
        Deserialize the given bytes to a command.

        :param command_bytes: The command bytes to deserialize.
        :return Command.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class EventSerializer(Serializer):
    """
    The base class for all event serializers.
    """

    def __init__(self):
        """
        Initialize a new instance of the EventSerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, Event event):
        """
        Serialize the given event to bytes.

        :param event: The event to serialize.
        :return bytes.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Event deserialize(self, bytes event_bytes):
        """
        Deserialize the given bytes to an event.

        :param event_bytes: The bytes to deserialize.
        :return Event.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class RequestSerializer(Serializer):
    """
    The base class for all request serializers.
    """

    def __init__(self):
        """
        Initialize a new instance of the RequestSerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, Request request):
        """
        Serialize the given request to bytes.

        :param request: The request to serialize.
        :return bytes.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Request deserialize(self, bytes request_bytes):
        """
        Deserialize the given bytes to a request.

        :param request_bytes: The bytes to deserialize.
        :return Request.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class ResponseSerializer(Serializer):
    """
    The base class for all response serializers.
    """

    def __init__(self):
        """
        Initialize a new instance of the ResponseSerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, Response response):
        """
        Serialize the given response to bytes.

        :param response: The response to serialize.
        :return bytes.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Response deserialize(self, bytes response_bytes):
        """
        Deserialize the given bytes to a response.

        :param response_bytes: The bytes to deserialize.
        :return Response.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class LogSerializer(Serializer):
    """
    The base class for all log message serializers.
    """

    def __init__(self):
        """
        Initialize a new instance of the LogSerializer class.
        """
        super().__init__()

    cpdef bytes serialize(self, LogMessage message):
        """
        Serialize the given message to bytes.

        :param message: The message to serialize.
        :return bytes.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef LogMessage deserialize(self, bytes message_bytes):
        """
        Deserialize the given bytes to a response.

        :param message_bytes: The bytes to deserialize.
        :return LogMessage.
        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")
