from libc.stdint cimport uint64_t

from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency


cdef class CryptoFuture(Instrument):
    cdef readonly Currency underlying
    """The underlying asset for the contract.\n\n:returns: `Currency`"""
    cdef readonly Currency settlement_currency
    """The settlement currency for the contract.\n\n:returns: `Currency`"""
    cdef readonly bint is_quanto
    """If the instrument is quanto.\n\n:returns: `bool`"""
    cdef readonly uint64_t activation_ns
    """UNIX timestamp (nanoseconds) for contract activation.\n\n:returns: `unit64_t`"""
    cdef readonly uint64_t expiration_ns
    """UNIX timestamp (nanoseconds) for contract expiration.\n\n:returns: `unit64_t`"""

    @staticmethod
    cdef CryptoFuture from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(CryptoFuture obj)

    @staticmethod
    cdef CryptoFuture from_pyo3_c(pyo3_instrument)
