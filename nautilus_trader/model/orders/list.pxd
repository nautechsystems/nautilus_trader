from libc.stdint cimport uint64_t

from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.orders.base cimport Order


cdef class OrderList:
    cdef readonly OrderListId id
    """The order list ID.\n\n:returns: `OrderListId`"""
    cdef readonly InstrumentId instrument_id
    """The instrument ID associated with the list.\n\n:returns: `InstrumentId`"""
    cdef readonly StrategyId strategy_id
    """The strategy ID associated with the list.\n\n:returns: `StrategyId`"""
    cdef readonly list orders
    """The contained orders list.\n\n:returns: `list[Order]`"""
    cdef readonly Order first
    """The first order in the list (typically the parent).\n\n:returns: `list[Order]`"""
    cdef readonly uint64_t ts_init
    """UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""

    cpdef bint is_bracket(self)
