from libc.stdint cimport uint64_t

from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class LimitOrder(Order):
    cdef readonly Price price
    """The order price (LIMIT).\n\n:returns: `Price`"""
    cdef readonly uint64_t expire_time_ns
    """The order expiration (UNIX epoch nanoseconds), zero for no expiration.\n\n:returns: `uint64_t`"""
    cdef readonly Quantity display_qty
    """The quantity of the order to display on the public book (iceberg).\n\n:returns: `Quantity` or ``None``"""

    @staticmethod
    cdef LimitOrder create_c(OrderInitialized init)

    @staticmethod
    cdef LimitOrder transform(Order order, uint64_t ts_init, Price price=*)

    @staticmethod
    cdef LimitOrder from_pyo3_c(pyo3_order)
