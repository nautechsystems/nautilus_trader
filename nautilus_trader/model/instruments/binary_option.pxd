from libc.stdint cimport uint64_t

from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price


cdef class BinaryOption(Instrument):
    cdef readonly str outcome
    """The binary outcome of the market.\n\n:returns: `str` or ``None``"""
    cdef readonly str description
    """The market description.\n\n:returns: `str` or ``None``"""
    cdef readonly uint64_t activation_ns
    """UNIX timestamp (nanoseconds) for contract activation.\n\n:returns: `unit64_t`"""
    cdef readonly uint64_t expiration_ns
    """UNIX timestamp (nanoseconds) for contract expiration.\n\n:returns: `unit64_t`"""

    @staticmethod
    cdef BinaryOption from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(BinaryOption obj)
