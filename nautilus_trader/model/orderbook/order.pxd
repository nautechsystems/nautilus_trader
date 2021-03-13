from nautilus_trader.model.c_enums.order_side cimport OrderSide

cdef class Order:
    cdef public double price
    cdef public double volume
    cdef readonly OrderSide side
    cdef public str id

    cpdef void update_price(self, float price)
    cpdef void update_volume(self, float volume)

    @property
    cdef inline double exposure(self):
        return self.price * self.volume
