from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price


cdef class OptionContract(Instrument):
    cdef readonly str exchange
    """The exchange ISO 10383 Market Identifier Code (MIC) where the instrument trades.\n\n:returns: `str` or ``None``"""
    cdef readonly str underlying
    """The underlying asset for the contract.\n\n:returns: `str`"""
    cdef readonly OptionKind option_kind
    """The option kind (PUT | CALL) for the contract.\n\n:returns: `OptionKind`"""
    cdef readonly Price strike_price
    """The strike price for the contract.\n\n:returns: `Price`"""
    cdef readonly uint64_t activation_ns
    """UNIX timestamp (nanoseconds) for contract activation.\n\n:returns: `unit64_t`"""
    cdef readonly uint64_t expiration_ns
    """UNIX timestamp (nanoseconds) for contract expiration.\n\n:returns: `unit64_t`"""

    @staticmethod
    cdef OptionContract from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OptionContract obj)

    @staticmethod
    cdef OptionContract from_pyo3_c(pyo3_instrument)
