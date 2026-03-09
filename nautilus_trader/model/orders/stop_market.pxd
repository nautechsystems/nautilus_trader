from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class StopMarketOrder(Order):
    cdef readonly Price trigger_price
    """The order trigger price (STOP).\n\n:returns: `Price`"""
    cdef readonly TriggerType trigger_type
    """The trigger type for the order.\n\n:returns: `TriggerType`"""
    cdef readonly uint64_t expire_time_ns
    """The order expiration (UNIX epoch nanoseconds), zero for no expiration.\n\n:returns: `uint64_t`"""

    @staticmethod
    cdef StopMarketOrder create_c(OrderInitialized init)
