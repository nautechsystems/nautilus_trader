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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelRejected
from nautilus_trader.model.events cimport OrderCanceled
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderPendingCancel
from nautilus_trader.model.events cimport OrderPendingReplace
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderTriggered
from nautilus_trader.model.events cimport OrderUpdateRejected
from nautilus_trader.model.events cimport OrderUpdated
from nautilus_trader.model.instruments.base cimport Instrument


_OBJECT_MAP = {
    CancelOrder.__name__: CancelOrder.from_dict_c,
    SubmitBracketOrder.__name__: SubmitBracketOrder.from_dict_c,
    SubmitOrder.__name__: SubmitOrder.from_dict_c,
    UpdateOrder.__name__: UpdateOrder.from_dict_c,
    AccountState.__name__: AccountState.from_dict_c,
    OrderAccepted.__name__: OrderAccepted.from_dict_c,
    OrderCancelRejected.__name__: OrderCancelRejected.from_dict_c,
    OrderCanceled.__name__: OrderCanceled.from_dict_c,
    OrderDenied.__name__: OrderDenied.from_dict_c,
    OrderExpired.__name__: OrderExpired.from_dict_c,
    OrderFilled.__name__: OrderFilled.from_dict_c,
    OrderInitialized.__name__: OrderInitialized.from_dict_c,
    OrderInvalid.__name__: OrderInvalid.from_dict_c,
    OrderPendingCancel.__name__: OrderPendingCancel.from_dict_c,
    OrderPendingReplace.__name__: OrderPendingReplace.from_dict_c,
    OrderRejected.__name__: OrderRejected.from_dict_c,
    OrderSubmitted.__name__: OrderSubmitted.from_dict_c,
    OrderTriggered.__name__: OrderTriggered.from_dict_c,
    OrderUpdateRejected.__name__: OrderUpdateRejected.from_dict_c,
    OrderUpdated.__name__: OrderUpdated.from_dict_c,
}

cpdef inline void register_serializable_object(object obj) except *:
    """
    Register the given object with the global serialization object map.

    The object must implement ``to_dict()`` and ``from_dict()`` methods.

    Parameters
    ----------
    obj : object
        The object to register.

    Raises
    ------
    ValueError
        If obj does not implement the `to_dict` method.
    ValueError
        If obj does not implement the `from_dict` method.
    KeyError
        If obj already registered with the global object map.

    """
    Condition.true(hasattr(obj, "to_dict"), "The given object does not implement `to_dict`.")
    Condition.true(hasattr(obj, "from_dict"), "The given object does not implement `from_dict`.")
    Condition.not_in(obj.__name__, _OBJECT_MAP, "obj", "_OBJECT_MAP")

    _OBJECT_MAP[obj.__name__] = obj.from_dict


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
