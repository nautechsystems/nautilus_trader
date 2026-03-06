from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency


cdef class CurrencyPair(Instrument):
    cdef readonly Currency base_currency
    """The base currency for the instrument.\n\n:returns: `Currency`"""

    @staticmethod
    cdef CurrencyPair from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(CurrencyPair obj)

    @staticmethod
    cdef CurrencyPair from_pyo3_c(pyo3_instrument)
