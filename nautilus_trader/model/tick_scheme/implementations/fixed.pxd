from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick_scheme.base cimport TickScheme


cdef class FixedTickScheme(TickScheme):
    cdef double _increment
    cdef double _min_price
    cdef double _max_price

    cdef readonly int price_precision
    """The tick scheme price precision.\n\n:returns: `int`"""
    cdef readonly Price increment
    """The tick scheme price increment.\n\n:returns: `Price`"""

    cpdef Price next_ask_price(self, double value, int n=*)
    cpdef Price next_bid_price(self, double value, int n=*)
