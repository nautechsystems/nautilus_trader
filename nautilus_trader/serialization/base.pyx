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

from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orders.base cimport Order


cdef class InstrumentSerializer:
    """
    The abstract base class for all instrument serializers.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``InstrumentSerializer`` class.

        """
        super().__init__()

    cpdef bytes serialize(self, Instrument instrument):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Instrument deserialize(self, bytes instrument_bytes):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")


cdef class OrderSerializer:
    """
    The abstract base class for all order serializers.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``OrderSerializer`` class.

        """
        super().__init__()

    cpdef bytes serialize(self, Order order):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Order deserialize(self, bytes order_bytes):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass ")


cdef class CommandSerializer:
    """
    The abstract base class for all command serializers.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``CommandSerializer`` class.
        """
        super().__init__()

    cpdef bytes serialize(self, Command command):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Command deserialize(self, bytes command_bytes):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")


cdef class EventSerializer:
    """
    The abstract base class for all event serializers.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``EventSerializer`` class.
        """
        super().__init__()

    cpdef bytes serialize(self, Event event):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Event deserialize(self, bytes event_bytes):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")
