from nautilus_trader.core.rust.persistence cimport ParquetType


cdef class ParquetWriter:
    cdef void *_writer
    cdef ParquetType _parquet_type
