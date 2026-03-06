from libc.stdint cimport uint64_t


cdef class LatencyModel:
    cdef readonly uint64_t base_latency_nanos
    """The default latency to the exchange.\n\n:returns: `int`"""
    cdef readonly uint64_t insert_latency_nanos
    """The latency (nanoseconds) for order insert messages to reach the exchange.\n\n:returns: `int`"""
    cdef readonly uint64_t update_latency_nanos
    """The latency (nanoseconds) for order update messages to reach the exchange.\n\n:returns: `int`"""
    cdef readonly uint64_t cancel_latency_nanos
    """The latency (nanoseconds) for order cancel messages to reach the exchange.\n\n:returns: `int`"""
