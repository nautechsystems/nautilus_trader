from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency


cdef class Cfd(Instrument):
    cdef readonly Currency base_currency
    """The base currency for the instrument.\n\n:returns: `Currency` or ``None``"""
    cdef readonly str isin
    """The instruments International Securities Identification Number (ISIN).\n\n:returns: `str` or ``None``"""

    @staticmethod
    cdef Cfd from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(Cfd obj)

    @staticmethod
    cdef Cfd from_pyo3_c(pyo3_instrument)
