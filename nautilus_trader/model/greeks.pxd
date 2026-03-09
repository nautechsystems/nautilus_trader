from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price


cdef class GreeksCalculator:
    cdef Clock _clock
    cdef Logger _log
    cdef CacheFacade _cache
    cdef object _get_price(self, InstrumentId instrument_id)
