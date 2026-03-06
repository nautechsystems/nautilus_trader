cimport numpy as np

from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick_scheme.base cimport TickScheme


cdef class TieredTickScheme(TickScheme):
    cdef list tiers
    cdef int max_ticks_per_tier
    cdef int price_precision
    cdef int tick_count

    cdef readonly np.ndarray ticks

    cpdef _build_ticks(self)

    cpdef int find_tick_index(self, double value)
    cpdef Price next_ask_price(self, double value, int n=*)
    cpdef Price next_bid_price(self, double value, int n=*)
