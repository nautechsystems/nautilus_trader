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

import os

from cpython.object cimport PyObject

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.model cimport TradeTick_t
from nautilus_trader.core.rust.persistence cimport parquet_writer_chunk_append
from nautilus_trader.core.rust.persistence cimport parquet_writer_drop
from nautilus_trader.core.rust.persistence cimport parquet_writer_new
from nautilus_trader.core.rust.persistence cimport parquet_writer_write
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick

from nautilus_trader.persistence.catalog.rust.common import py_type_to_parquet_type

from nautilus_trader.persistence.catalog.rust.vec cimport create_vector


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

        os.makedirs(os.path.dirname(file_path), exist_ok=True)

        self._parquet_type = py_type_to_parquet_type(parquet_type)
        self._writer = parquet_writer_new(
            <PyObject *>file_path,
            self._parquet_type,
            <PyObject *>metadata,
        )

    def write(self, list items):
        parquet_writer_write(
            self._writer,
            <ParquetType>self._parquet_type,
            <void *>create_vector(items),
            len(items)
        )

    def drop(self):
        parquet_writer_drop(self._writer, self._parquet_type)
