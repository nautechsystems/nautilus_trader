from cpython.object cimport PyObject
# from libcpp.vector cimport vector
from libc.stdlib cimport malloc, free
from cython.operator cimport dereference
import os
from nautilus_trader.core.rust.persistence cimport parquet_writer_new
from nautilus_trader.core.rust.persistence cimport parquet_writer_drop
from nautilus_trader.core.rust.persistence cimport parquet_writer_write
from nautilus_trader.core.rust.persistence cimport parquet_writer_chunk_append
from nautilus_trader.core.rust.core cimport cvec_new
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.core cimport cvec_drop

from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.model cimport TradeTick_t
from nautilus_trader.persistence.catalog.rust.common import py_type_to_parquet_type
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
cdef class ParquetWriter:
    """
    Provides a parquet writer implemented in Rust under the hood.
    """
    def __init__(
        self,
        str file_path,
        type parquet_type,
        dict metadata = {"key":"value"} # TODO
    ):
        Condition.valid_string(file_path, "file_path")

        assert  all(isinstance(k, str) and isinstance(v, str)
                for k, v in metadata.items())

        # This should be on rust side?
        os.makedirs(os.path.dirname(file_path), exist_ok=True)
        os.remove(file_path) #For dev only

        self._parquet_type = py_type_to_parquet_type(parquet_type)
        self._writer = parquet_writer_new(
                            <PyObject *>file_path,
                            self._parquet_type,
                            <PyObject *>metadata)
        
        
    cpdef void write(self, list items):
        self._write(items)

    cdef void _write(self, list items):
        
        cdef:
            CVec chunk = cvec_new()
            QuoteTick tick
            void *item
        
        tick = <QuoteTick>items[0]
        item = <void *>&tick._mem
        
        chunk = parquet_writer_chunk_append(chunk, item, self._parquet_type)
        
