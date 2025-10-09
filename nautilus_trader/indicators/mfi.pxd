

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class MoneyFlowIndex(Indicator):
    cdef object _inner

    cdef readonly int period
    """The rolling window period.\n\n:returns: `int`"""
    cdef readonly double value
    """The current value.\n\n:returns: `double`"""
    
    cpdef void update_raw(self, double typical_price, double volume)
    cpdef void _update_readonly_attributes(self)


