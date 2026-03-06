from nautilus_trader.cache.facade cimport CacheDatabaseFacade
from nautilus_trader.serialization.base cimport Serializer


cdef class CacheDatabaseAdapter(CacheDatabaseFacade):
    cdef Serializer _serializer
    cdef object _backing
