# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from cpython.object cimport PyObject

from nautilus_trader.persistence.catalog.rust.common import parquet_type_to_struct_size
from nautilus_trader.persistence.catalog.rust.common import py_type_to_parquet_type

from nautilus_trader.core.rust.core cimport cvec_free
from nautilus_trader.core.rust.core cimport cvec_new
from nautilus_trader.core.rust.persistence cimport parquet_writer_flush
from nautilus_trader.core.rust.persistence cimport parquet_writer_new
from nautilus_trader.core.rust.persistence cimport parquet_writer_write
from nautilus_trader.persistence.catalog.rust.vec cimport create_vector


cdef class ParquetWriter:
    """
    Provides a parquet writer implemented in Rust under the hood.
    """
    def __init__(self, type parquet_type, dict metadata):
        assert  all(isinstance(k, str) and isinstance(v, str)
                for k, v in metadata.items())

        self._parquet_type = py_type_to_parquet_type(parquet_type)
        self._struct_size = parquet_type_to_struct_size(self._parquet_type)
        self._writer = parquet_writer_new(
            parquet_type=self._parquet_type,
            metadata=<PyObject *>metadata,
        )
        self._vec = cvec_new()

    def __del__(self):
        cvec_free(self._vec)
        # TODO(cs): Writer already freed when flushed, although we need a way
        #  to free the writer if flush was never called

    @property
    def struct_size(self) -> int:
        return self._struct_size

    cpdef void write(self, list items) except *:
        parquet_writer_write(
            writer=self._writer,
            parquet_type=<ParquetType>self._parquet_type,
            data=<void *>create_vector(items),
            len=len(items),
        )

    cpdef bytes flush(self):
        self._vec = parquet_writer_flush(self._writer, self._parquet_type)
        cdef char *buffer = <char *>self._vec.ptr
        return <bytes>buffer[:self._vec.len]
