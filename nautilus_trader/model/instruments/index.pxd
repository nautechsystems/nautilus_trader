from nautilus_trader.model.instruments.base cimport Instrument


cdef class IndexInstrument(Instrument):
    @staticmethod
    cdef IndexInstrument from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(IndexInstrument obj)

    @staticmethod
    cdef IndexInstrument from_pyo3_c(pyo3_instrument)
