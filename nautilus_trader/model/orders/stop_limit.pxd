from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class StopLimitOrder(Order):
    cdef readonly Price price
    """The order price (LIMIT).\n\n:returns: `Price`"""
    cdef readonly Price trigger_price
    """The order trigger price (STOP).\n\n:returns: `Price`"""
    cdef readonly TriggerType trigger_type
    """The trigger type for the order.\n\n:returns: `TriggerType`"""
    cdef readonly uint64_t expire_time_ns
    """The order expiration (UNIX epoch nanoseconds), zero for no expiration.\n\n:returns: `uint64_t`"""
    cdef readonly Quantity display_qty
    """The quantity of the ``LIMIT`` order to display on the public book (iceberg).\n\n:returns: `Quantity` or ``None``"""  # noqa
    cdef readonly bint is_triggered
    """If the order has been triggered.\n\n:returns: `bool`"""
    cdef readonly uint64_t ts_triggered
    """UNIX timestamp (nanoseconds) when the order was triggered (0 if not triggered).\n\n:returns: `uint64_t`"""

    @staticmethod
    cdef StopLimitOrder create_c(OrderInitialized init)

    @staticmethod
    cdef StopLimitOrder from_pyo3_c(pyo3_order)
