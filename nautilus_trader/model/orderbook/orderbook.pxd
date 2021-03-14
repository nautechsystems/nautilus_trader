from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.order cimport Order

cdef class OrderbookProxy:
    cdef readonly Ladder bids
    cdef readonly Ladder asks

    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)
    cpdef void clear(self)
    cpdef bint _check_integrity(self, bint deep= *)

# TODO - Some violations of DRY principal here -

cdef class L3Orderbook:
    cdef readonly OrderbookProxy _orderbook
    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)

# cdef class L2Orderbook:
#     cpdef void add(self, Order order)
#     cpdef void update(self, Order order)
#     cpdef void delete(self, Order order)
#
# cdef class L1Orderbook:
#     cpdef void add(self, Order order)
#     cpdef void update(self, Order order)
#     cpdef void delete(self, Order order)
