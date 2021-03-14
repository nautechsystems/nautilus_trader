from nautilus_trader.model.orderbook.order cimport Order

cdef class Ladder:
    cdef readonly list levels
    # TODO cdef list[Level] levels -> Level must be a struct
    cdef bint reverse
    cdef dict price_levels
    cdef dict order_id_prices

    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)
    cpdef top(self, int n=*)
