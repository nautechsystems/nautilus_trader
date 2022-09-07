from nautilus_trader.core.rust.persistence cimport ParquetType

cdef class ParquetWriter:
    cdef void *_writer
    cdef ParquetType _parquet_type
    cpdef void write(self, list items)
    cdef void _write(self, list items)
    # cdef list _parse(self, list items)