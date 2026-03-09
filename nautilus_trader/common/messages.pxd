from libc.stdint cimport uint64_t

from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.model cimport TradingState
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport Identifier
from nautilus_trader.model.identifiers cimport TraderId


cdef class ShutdownSystem(Command):
    cdef UUID4 _command_id
    cdef uint64_t _ts_init

    cdef readonly TraderId trader_id
    """The trader ID associated with the event.\n\n:returns: `TraderId`"""
    cdef readonly Identifier component_id
    """The component ID associated with the event.\n\n:returns: `Identifier`"""
    cdef readonly str reason
    """The reason for the shutdown command.\n\n:returns: `str` or ``None``"""

    @staticmethod
    cdef ShutdownSystem from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(ShutdownSystem obj)


cdef class ComponentStateChanged(Event):
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

    cdef readonly TraderId trader_id
    """The trader ID associated with the event.\n\n:returns: `TraderId`"""
    cdef readonly Identifier component_id
    """The component ID associated with the event.\n\n:returns: `Identifier`"""
    cdef readonly str component_type
    """The component type associated with the event.\n\n:returns: `str`"""
    cdef readonly ComponentState state
    """The component state.\n\n:returns: `ComponentState`"""
    cdef readonly dict config
    """The component configuration.\n\n:returns: `dict[str, Any]`"""

    @staticmethod
    cdef ComponentStateChanged from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(ComponentStateChanged obj)


cdef class RiskEvent(Event):
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

    cdef readonly TraderId trader_id
    """The trader ID associated with the event.\n\n:returns: `TraderId`"""


cdef class TradingStateChanged(RiskEvent):
    cdef readonly TradingState state
    """The trading state for the event.\n\n:returns: `TradingState`"""
    cdef readonly dict config
    """The risk engine configuration.\n\n:returns: `dict[str, Any]`"""

    @staticmethod
    cdef TradingStateChanged from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(TradingStateChanged obj)
