from nautilus_trader.model.instruments.base cimport Instrument


cdef class Commodity(Instrument):
    cdef readonly str isin
    """The instruments International Securities Identification Number (ISIN).\n\n:returns: `str` or ``None``"""

    @staticmethod
    cdef Commodity from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(Commodity obj)

    @staticmethod
    cdef Commodity from_pyo3_c(pyo3_instrument)
