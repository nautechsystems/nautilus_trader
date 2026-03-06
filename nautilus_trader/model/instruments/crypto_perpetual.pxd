from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency


cdef class CryptoPerpetual(Instrument):
    cdef readonly Currency base_currency
    """The base currency for the instrument.\n\n:returns: `Currency`"""
    cdef readonly Currency settlement_currency
    """The settlement currency for the instrument.\n\n:returns: `Currency`"""
    cdef readonly bint is_quanto
    """If the instrument is quanto.\n\n:returns: `bool`"""

    @staticmethod
    cdef CryptoPerpetual from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(CryptoPerpetual obj)

    @staticmethod
    cdef CryptoPerpetual from_pyo3_c(pyo3_instrument)
