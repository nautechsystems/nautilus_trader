from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class SpreadAnalyzer(Indicator):
    cdef object _spreads

    cdef readonly InstrumentId instrument_id
    """The indicators instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly int capacity
    """The indicators spread capacity.\n\n:returns: `int`"""
    cdef readonly double current
    """The current spread.\n\n:returns: `double`"""
    cdef readonly double average
    """The current average spread.\n\n:returns: `double`"""
