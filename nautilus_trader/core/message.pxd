from libc.stdint cimport uint64_t

from nautilus_trader.core.uuid cimport UUID4


cdef class Command:
    cdef readonly UUID4 id
    """The command message ID.\n\n:returns: `UUID4`"""
    cdef readonly uint64_t ts_init
    """UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""
    cdef readonly UUID4 correlation_id
    """The command correlation ID.\n\n:returns: `UUID4` or ``None``"""


cdef class Document:
    cdef readonly UUID4 id
    """The document message ID.\n\n:returns: `UUID4`"""
    cdef readonly uint64_t ts_init
    """UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""


cdef class Event:
    pass


cdef class Request:
    cdef readonly UUID4 id
    """The request message ID.\n\n:returns: `UUID4`"""
    cdef readonly uint64_t ts_init
    """UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""
    cdef readonly object callback
    """The callback for the response.\n\n:returns: `Callable`"""
    cdef readonly UUID4 correlation_id
    """The request correlation ID.\n\n:returns: `UUID4` or ``None``"""


cdef class Response:
    cdef readonly UUID4 id
    """The response message ID.\n\n:returns: `UUID4`"""
    cdef readonly uint64_t ts_init
    """UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""
    cdef readonly UUID4 correlation_id
    """The response correlation ID.\n\n:returns: `UUID4`"""
