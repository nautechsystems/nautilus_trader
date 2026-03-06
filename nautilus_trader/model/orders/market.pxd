from libc.stdint cimport uint64_t

from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.orders.base cimport Order


cdef class MarketOrder(Order):
    @staticmethod
    cdef MarketOrder create_c(OrderInitialized init)

    @staticmethod
    cdef MarketOrder transform(Order order, uint64_t ts_init)

    @staticmethod
    cdef MarketOrder from_pyo3_c(pyo3_order)
