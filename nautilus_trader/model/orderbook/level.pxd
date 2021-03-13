cdef class Level:
    cdef public list orders
    cdef public dict order_index

    cpdef void add(self)
    cpdef void update(self)
    cpdef void delete(self)

    @property
    cpdef inline double volume(self):
        return sum([order.volume for order in self.orders])

    @property
    cpdef inline double exposure(self):
        return sum([order.exposure for order in self.orders])
