from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Price


cdef class CryptoOption(Instrument):
    cdef readonly Currency underlying
    """The underlying asset for the contract.\n\n:returns: `str`"""
    cdef readonly Currency settlement_currency
    """The settlement currency for the instrument.\n\n:returns: `Currency`"""
    cdef readonly OptionKind option_kind
    """The option kind (PUT | CALL) for the contract.\n\n:returns: `OptionKind`"""
    cdef readonly Price strike_price
    """The strike price for the contract.\n\n:returns: `Price`"""
    cdef readonly uint64_t activation_ns
    """UNIX timestamp (nanoseconds) for contract activation.\n\n:returns: `unit64_t`"""
    cdef readonly uint64_t expiration_ns
    """UNIX timestamp (nanoseconds) for contract expiration.\n\n:returns: `unit64_t`"""

    @staticmethod
    cdef CryptoOption from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(CryptoOption obj)

    @staticmethod
    cdef CryptoOption from_pyo3_c(pyo3_instrument)
