from nautilus_trader.model.orderbook.order cimport Order

cdef class Level:
    cdef readonly list orders
    cdef readonly dict order_index

    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)
    cpdef _check_price(self, Order order)
    cpdef _get_order(self, str order_id)

    #TODO - make property?
    cpdef public double volume(self)
    cpdef public double price(self)

    # @property
    # cdef inline double exposure(self):
    #     return sum([order.exposure for order in self.orders])
