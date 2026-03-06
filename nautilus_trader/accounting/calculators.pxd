from cpython.datetime cimport date

from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Currency


cdef class RolloverInterestCalculator:
    cdef dict _rate_data

    cpdef object get_rate_data(self)
    cpdef object calc_overnight_rate(self, InstrumentId instrument_id, date timestamp)
