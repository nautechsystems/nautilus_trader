from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.order cimport Order

cdef class Orderbook:
    cdef public Ladder bids
    cdef public Ladder asks

    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)
    cpdef bint _check_integrity(self)
