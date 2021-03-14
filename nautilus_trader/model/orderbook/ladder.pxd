from nautilus_trader.model.orderbook.order cimport Order

cdef class Ladder:
    # TODO cdef list[Level] levels -> Level must be a struct
    cdef readonly list levels
    cdef readonly bint reverse
    cdef dict price_levels
    cdef dict order_id_prices

    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)
    cpdef depth(self, int n=*)
