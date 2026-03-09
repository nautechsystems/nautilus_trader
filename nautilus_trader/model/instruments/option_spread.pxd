from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price


cdef class OptionSpread(Instrument):
    cdef readonly str exchange
    """The exchang ISO 10383 Market Identifier Code (MIC) where the instrument trades.\n\n:returns: `str` or ``None``"""
    cdef readonly str underlying
    """The underlying asset for the contract.\n\n:returns: `str`"""
    cdef readonly str strategy_type
    """The strategy type of the spread.\n\n:returns: `str`"""
    cdef readonly uint64_t activation_ns
    """UNIX timestamp (nanoseconds) for contract activation.\n\n:returns: `unit64_t`"""
    cdef readonly uint64_t expiration_ns
    """UNIX timestamp (nanoseconds) for contract expiration.\n\n:returns: `unit64_t`"""

    @staticmethod
    cdef OptionSpread from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OptionSpread obj)

    @staticmethod
    cdef OptionSpread from_pyo3_c(pyo3_instrument)
