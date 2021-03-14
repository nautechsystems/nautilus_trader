from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.order cimport Order

# TODO - Some violations of DRY principal here - I can't think of a way around this with cython given the slow
#  subclassing. The best I can come up with is this shared OrderbookProxy object which is going to mean a bunch of
#  duplicated accessor code for the L1/L2/L3 Orderbook classes. Possible some code generation might be worthwhile here?

cdef class OrderbookProxy:
    cdef readonly Ladder bids
    cdef readonly Ladder asks

    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)
    cpdef void clear(self)
    cpdef bint _check_integrity(self, bint deep= *)

cdef class L3Orderbook:
    cdef readonly OrderbookProxy _orderbook
    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)
    cpdef void delete(self, Order order)
    cpdef void delete(self, Order order)
    cpdef bint _check_integrity(self, bint deep= *)


# cdef class L2Orderbook:
#     cpdef void add(self, Order order)
#     cpdef void update(self, Order order)
#     cpdef void delete(self, Order order)
#
# cdef class L1Orderbook:
#     cpdef void add(self, Order order)
#     cpdef void update(self, Order order)
#     cpdef void delete(self, Order order)
