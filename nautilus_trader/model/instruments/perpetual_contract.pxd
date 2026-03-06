from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency


cdef class PerpetualContract(Instrument):
    cdef readonly str underlying
    """The underlying asset identifier.\n\n:returns: `str`"""
    cdef readonly Currency base_currency
    """The base currency for the instrument.\n\n:returns: `Currency` or ``None``"""
    cdef readonly Currency settlement_currency
    """The settlement currency for the instrument.\n\n:returns: `Currency`"""
    cdef readonly bint is_quanto
    """If the instrument is a quanto perpetual.\n\n:returns: `bool`"""

    @staticmethod
    cdef PerpetualContract from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(PerpetualContract obj)

    @staticmethod
    cdef PerpetualContract from_pyo3_c(pyo3_instrument)
