from nautilus_trader.serialization.base cimport Serializer


cdef class MsgSpecSerializer(Serializer):
    cdef object _encode
    cdef object _decode

    cdef readonly bint timestamps_as_str
    """If the serializer converts timestamp `int64_t` to integer strings.\n\n:returns: `bool`"""
    cdef readonly bint timestamps_as_iso8601
    """If the serializer converts timestamp `int64_t` to ISO 8601 strings.\n\n:returns: `bool`"""
