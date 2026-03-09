from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.orders.base cimport Order


cdef class OrderUnpacker:

    @staticmethod
    cdef Order unpack_c(dict values)

    @staticmethod
    cdef Order from_init_c(OrderInitialized init)
