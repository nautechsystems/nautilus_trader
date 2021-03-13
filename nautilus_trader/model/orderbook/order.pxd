from nautilus_trader.model.c_enums.order_side cimport OrderSide

cdef class Order:
    cdef public float price
    cdef public float volume
    cdef readonly OrderSide side
    cdef public str id
