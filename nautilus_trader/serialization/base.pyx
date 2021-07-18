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
from nautilus_trader.model.commands.trading cimport CancelOrder
from nautilus_trader.model.commands.trading cimport SubmitBracketOrder
from nautilus_trader.model.commands.trading cimport SubmitOrder
from nautilus_trader.model.commands.trading cimport UpdateOrder
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.venue cimport InstrumentStatusUpdate
from nautilus_trader.model.data.venue cimport VenueStatusUpdate
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderPendingCancel
from nautilus_trader.model.events.order cimport OrderPendingUpdate
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderSubmitted
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdateRejected
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.events.position cimport PositionChanged
from nautilus_trader.model.events.position cimport PositionClosed
from nautilus_trader.model.events.position cimport PositionOpened
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.betting cimport BettingInstrument
from nautilus_trader.model.instruments.crypto_swap cimport CryptoSwap
from nautilus_trader.model.instruments.currency cimport CurrencySpot


# Default mappings for Nautilus objects
_OBJECT_TO_DICT_MAP = {
    CancelOrder.__name__: CancelOrder.to_dict_c,
    SubmitBracketOrder.__name__: SubmitBracketOrder.to_dict_c,
    SubmitOrder.__name__: SubmitOrder.to_dict_c,
    UpdateOrder.__name__: UpdateOrder.to_dict_c,
    AccountState.__name__: AccountState.to_dict_c,
    OrderAccepted.__name__: OrderAccepted.to_dict_c,
    OrderCancelRejected.__name__: OrderCancelRejected.to_dict_c,
    OrderCanceled.__name__: OrderCanceled.to_dict_c,
    OrderDenied.__name__: OrderDenied.to_dict_c,
    OrderExpired.__name__: OrderExpired.to_dict_c,
    OrderFilled.__name__: OrderFilled.to_dict_c,
    OrderInitialized.__name__: OrderInitialized.to_dict_c,
    OrderPendingCancel.__name__: OrderPendingCancel.to_dict_c,
    OrderPendingUpdate.__name__: OrderPendingUpdate.to_dict_c,
    OrderRejected.__name__: OrderRejected.to_dict_c,
    OrderSubmitted.__name__: OrderSubmitted.to_dict_c,
    OrderTriggered.__name__: OrderTriggered.to_dict_c,
    OrderUpdateRejected.__name__: OrderUpdateRejected.to_dict_c,
    OrderUpdated.__name__: OrderUpdated.to_dict_c,
    PositionOpened.__name__: PositionOpened.to_dict_c,
    PositionChanged.__name__: PositionChanged.to_dict_c,
    PositionClosed.__name__: PositionClosed.to_dict_c,
    Instrument.__name__: Instrument.base_to_dict_c,
    BettingInstrument.__name__: BettingInstrument.to_dict_c,
    CryptoSwap.__name__: CryptoSwap.to_dict_c,
    CurrencySpot.__name__: CurrencySpot.to_dict_c,
    TradeTick.__name__: TradeTick.to_dict_c,
    InstrumentStatusUpdate.__name__: InstrumentStatusUpdate.to_dict_c,
    VenueStatusUpdate.__name__: VenueStatusUpdate.to_dict_c,
}


# Default mappings for Nautilus objects
_OBJECT_FROM_DICT_MAP = {
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
    OrderPendingCancel.__name__: OrderPendingCancel.from_dict_c,
    OrderPendingUpdate.__name__: OrderPendingUpdate.from_dict_c,
    OrderRejected.__name__: OrderRejected.from_dict_c,
    OrderSubmitted.__name__: OrderSubmitted.from_dict_c,
    OrderTriggered.__name__: OrderTriggered.from_dict_c,
    OrderUpdateRejected.__name__: OrderUpdateRejected.from_dict_c,
    OrderUpdated.__name__: OrderUpdated.from_dict_c,
    PositionOpened.__name__: PositionOpened.from_dict_c,
    PositionChanged.__name__: PositionChanged.from_dict_c,
    PositionClosed.__name__: PositionClosed.from_dict_c,
    Instrument.__name__: Instrument.base_from_dict_c,
    BettingInstrument.__name__: BettingInstrument.from_dict_c,
    CryptoSwap.__name__: CryptoSwap.from_dict_c,
    CurrencySpot.__name__: CurrencySpot.from_dict_c,
    TradeTick.__name__: TradeTick.from_dict_c,
    InstrumentStatusUpdate.__name__: InstrumentStatusUpdate.from_dict_c,
    VenueStatusUpdate.__name__: VenueStatusUpdate.from_dict_c,
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
    Condition.not_in(obj.__name__, _OBJECT_TO_DICT_MAP, "obj.__name__", "_OBJECT_TO_DICT_MAP")
    Condition.not_in(obj.__name__, _OBJECT_FROM_DICT_MAP, "obj.__name__", "_OBJECT_FROM_DICT_MAP")

    _OBJECT_TO_DICT_MAP[obj.__name__] = to_dict
    _OBJECT_FROM_DICT_MAP[obj.__name__] = from_dict


cpdef inline object get_to_dict(str obj_name):
    return _OBJECT_TO_DICT_MAP.get(obj_name)

cpdef inline object get_from_dict(str obj_name):
    return _OBJECT_FROM_DICT_MAP.get(obj_name)


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
