from libc.math cimport fabs
from libc.math cimport fmax
from libc.math cimport fmin

from nautilus_trader.model.objects cimport Price


cdef dict[str, TickScheme] TICK_SCHEMES

cdef class TickScheme:
    cdef readonly str name
    """The name of the scheme.\n\n:returns: `str`"""
    cdef readonly Price min_price
    """The minimum valid price for the scheme.\n\n:returns: `Price`"""
    cdef readonly Price max_price
    """The maximum valid price for the scheme.\n\n:returns: `Price`"""

    cpdef Price next_ask_price(self, double value, int n=*)
    cpdef Price next_bid_price(self, double value, int n=*)


cpdef double round_down(double value, double base)
cpdef double round_up(double value, double base)

cpdef void register_tick_scheme(TickScheme tick_scheme)
cpdef TickScheme get_tick_scheme(str name)


cdef inline bint is_close(double a, double b) noexcept nogil:
    # Check if two floating point values are approximately equal:
    # uses relative tolerance scaled with magnitude but capped to prevent
    # treating values multiple ticks away as "on boundary".
    cdef double diff = fabs(a - b)
    cdef double largest = fmax(fabs(a), fabs(b))
    cdef double rel_tol = 1e-12 * largest  # RELATIVE_TOLERANCE
    cdef double tolerance = fmin(rel_tol, 0.001)  # MAX_TICK_DELTA
    tolerance = fmax(tolerance, 1e-14)  # ABSOLUTE_TOLERANCE
    return diff <= tolerance
