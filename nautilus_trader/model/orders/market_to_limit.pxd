from libc.stdint cimport uint64_t

from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class MarketToLimitOrder(Order):
    cdef readonly Price price
    """The order price (LIMIT).\n\n:returns: `Price` or ``None``"""
    cdef readonly uint64_t expire_time_ns
    """The order expiration (UNIX epoch nanoseconds), zero for no expiration.\n\n:returns: `uint64_t`"""
    cdef readonly Quantity display_qty
    """The quantity of the limit order to display on the public book (iceberg).\n\n:returns: `Quantity` or ``None``"""

    @staticmethod
    cdef MarketToLimitOrder create_c(OrderInitialized init)
