cdef class L2Level:
    cdef public list orders
    cdef public dict order_index

    cpdef void add(self, order)
    cpdef void update(self, float volume)
    cpdef void delete(self, float volume)
    cpdef double volume(self)

    #TODO - How to do cython properties?

    # @property
    # cdef inline double volume(self):
    #     return sum([order.volume for order in self.orders])
    #
    # @property
    # cdef inline double exposure(self):
    #     return sum([order.exposure for order in self.orders])
