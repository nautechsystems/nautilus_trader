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
from nautilus_trader.model.events cimport InstrumentStatusEvent
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
from nautilus_trader.model.instruments.betting cimport BettingInstrument
from nautilus_trader.model.tick cimport TradeTick


def to_dict(obj):
    return obj.to_dict()


OBJECT_TO_DICT_MAP = {
    CancelOrder: to_dict,
    SubmitBracketOrder: to_dict,
    SubmitOrder: to_dict,
    UpdateOrder: to_dict,
    AccountState: to_dict,
    OrderAccepted: to_dict,
    OrderCancelRejected: to_dict,
    OrderCanceled: to_dict,
    OrderDenied: to_dict,
    OrderExpired: to_dict,
    OrderFilled: to_dict,
    OrderInitialized: to_dict,
    OrderInvalid: to_dict,
    OrderPendingCancel: to_dict,
    OrderPendingReplace: to_dict,
    OrderRejected: to_dict,
    OrderSubmitted: to_dict,
    OrderTriggered: to_dict,
    OrderUpdateRejected: to_dict,
    OrderUpdated: to_dict,
}

OBJECT_FROM_DICT_MAP = {
    CancelOrder: CancelOrder.from_dict_c,
    SubmitBracketOrder: SubmitBracketOrder.from_dict_c,
    SubmitOrder: SubmitOrder.from_dict_c,
    UpdateOrder: UpdateOrder.from_dict_c,
    AccountState: AccountState.from_dict_c,
    OrderAccepted: OrderAccepted.from_dict_c,
    OrderCancelRejected: OrderCancelRejected.from_dict_c,
    OrderCanceled: OrderCanceled.from_dict_c,
    OrderDenied: OrderDenied.from_dict_c,
    OrderExpired: OrderExpired.from_dict_c,
    OrderFilled: OrderFilled.from_dict_c,
    OrderInitialized: OrderInitialized.from_dict_c,
    OrderInvalid: OrderInvalid.from_dict_c,
    OrderPendingCancel: OrderPendingCancel.from_dict_c,
    OrderPendingReplace: OrderPendingReplace.from_dict_c,
    OrderRejected: OrderRejected.from_dict_c,
    OrderSubmitted: OrderSubmitted.from_dict_c,
    OrderTriggered: OrderTriggered.from_dict_c,
    OrderUpdateRejected: OrderUpdateRejected.from_dict_c,
    OrderUpdated: OrderUpdated.from_dict_c,
    TradeTick: TradeTick.from_dict_c,
    InstrumentStatusEvent: InstrumentStatusEvent.from_dict_c,
    BettingInstrument: BettingInstrument.from_dict_c,
}


cpdef inline void register_serializable_object(
    object obj,
    to_dict: callable,
    from_dict: callable,
) except *:
    """
    Register the given object with the global serialization object maps.
    Parameters
    ----------
    obj : object
        The object to register.
    to_dict : callable
        The delegate to instantiate a dict of primitive types from the object.
    from_dict : callable
        The delegate to instantiate the object from a dict of primitive types.
    Raises
    ------
    TypeError
        If `to_dict` or `from_dict` are not of type callable.
    KeyError
        If obj already registered with the global object maps.
    """
    Condition.callable(to_dict, "to_dict")
    Condition.callable(from_dict, "from_dict")
    Condition.not_in(obj.__class__, OBJECT_TO_DICT_MAP, "obj.__class__", "_OBJECT_TO_DICT_MAP")
    Condition.not_in(obj.__class__, OBJECT_FROM_DICT_MAP, "obj.__class__", "_OBJECT_FROM_DICT_MAP")
    OBJECT_TO_DICT_MAP[obj.__class__.__name__] = to_dict
    OBJECT_FROM_DICT_MAP[obj.__class__.__name__] = from_dict


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
