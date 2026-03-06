from nautilus_trader.core.rust.core cimport UUID4_t


cdef class UUID4:
    cdef UUID4_t _mem

    cdef str to_str(self)

    @staticmethod
    cdef UUID4 from_mem_c(UUID4_t raw)

    @staticmethod
    cdef UUID4 from_str_c(str value)
