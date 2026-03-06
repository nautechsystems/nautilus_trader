from nautilus_trader.model.instruments.base cimport Instrument


cdef class Equity(Instrument):
    cdef readonly str isin
    """The instruments International Securities Identification Number (ISIN).\n\n:returns: `str` or ``None``"""

    @staticmethod
    cdef Equity from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(Equity obj)

    @staticmethod
    cdef Equity from_pyo3_c(pyo3_instrument)
