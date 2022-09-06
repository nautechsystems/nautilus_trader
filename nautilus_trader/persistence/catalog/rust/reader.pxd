from nautilus_trader.core.rust.persistence cimport ParquetType
from nautilus_trader.core.rust.core cimport CVec
cdef class ParquetReader:
    cdef str file_path
    cdef ParquetType parquet_type
    cdef CVec chunk
    cdef void *reader

    cpdef list _next_chunk(self)
    cdef list _parse_chunk(self, CVec chunk)
    cpdef void _drop(self)
    cpdef void _drop_chunk(self)