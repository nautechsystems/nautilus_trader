from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick


cdef class Indicator:
    cdef list _params

    cdef readonly str name
    """The name of the indicator.\n\n:returns: `str`"""
    cdef readonly bint has_inputs
    """If the indicator has received inputs.\n\n:returns: `bool`"""
    cdef readonly bint initialized
    """If the indicator is warmed up and initialized.\n\n:returns: `bool`"""

    cdef str _params_str(self)

    cpdef void handle_quote_tick(self, QuoteTick tick)
    cpdef void handle_trade_tick(self, TradeTick tick)
    cpdef void handle_bar(self, Bar bar)
    cpdef void reset(self)

    cpdef void _set_has_inputs(self, bint setting)
    cpdef void _set_initialized(self, bint setting)
    cpdef void _reset(self)
