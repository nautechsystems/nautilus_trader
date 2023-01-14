# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.persistence cimport ParquetReaderType
from nautilus_trader.core.rust.persistence cimport ParquetType


cdef class ParquetReader:
    cdef ParquetType _parquet_type
    cdef ParquetReaderType _reader_type
    cdef void* _reader
    cdef CVec _chunk

    cdef void _drop_chunk(self) except *

cdef class ParquetFileReader(ParquetReader):
    cdef str _file_path


# cdef class ParquetBufferReader(ParquetReader):
#     pass


cdef list _parse_quote_tick_chunk(CVec chunk)
cdef list _parse_trade_tick_chunk(CVec chunk)
