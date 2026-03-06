from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport SyntheticInstrument_API
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price


cdef class SyntheticInstrument(Data):
    cdef SyntheticInstrument_API _mem

    cdef readonly InstrumentId id
    """The instrument ID.\n\n:returns: `InstrumentId`"""

    cpdef void change_formula(self, str formula)
    cpdef Price calculate(self, list[double] inputs)

    @staticmethod
    cdef SyntheticInstrument from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(SyntheticInstrument obj)
