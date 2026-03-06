cdef dict _OBJECT_TO_DICT_MAP
cdef dict _OBJECT_FROM_DICT_MAP
cdef set[type] _EXTERNAL_PUBLISHABLE_TYPES


cdef class Serializer:
    cpdef bytes serialize(self, object obj)
    cpdef object deserialize(self, bytes obj_bytes)
