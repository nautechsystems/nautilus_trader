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
from nautilus_trader.core.rust.persistence cimport parquet_reader_new
from nautilus_trader.core.rust.persistence cimport parquet_reader_drop
from nautilus_trader.core.rust.persistence cimport parquet_reader_drop_chunk
from nautilus_trader.core.rust.persistence cimport parquet_reader_index_chunk
from nautilus_trader.core.rust.persistence cimport parquet_reader_next_chunk
from nautilus_trader.core.rust.persistence cimport ParquetType
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.model.data.tick cimport QuoteTick
from cpython.object cimport PyObject
from libc.stdint cimport uint8_t, uint64_t, uintptr_t

def py_type_to_parquet_type(cls: type):
    if cls == QuoteTick:
        return ParquetType.QuoteTick
    else:
        raise RuntimeError(f"Type {cls} not supported as a ParquetType yet.")

cdef _parse_quote_tick_chunk(CVec chunk):
    cdef QuoteTick_t _mem
    cdef QuoteTick tick
    cdef list ticks = []
    for i in range(0, chunk.len):
        _mem = (<QuoteTick_t *>(parquet_reader_index_chunk(chunk, ParquetType.QuoteTick, i)))[0]
        tick = QuoteTick.__new__(QuoteTick)
        tick.ts_event = _mem.ts_event
        tick.ts_init = _mem.ts_init
        tick._mem = _mem
        ticks.append(tick)
    return ticks

cdef class ParquetReader:
    def __init__(self, file_path, parquet_type: type):
        self.file_path = file_path
        self.parquet_type = py_type_to_parquet_type(parquet_type)
        # TODO Check if file exists
        # TODO Check parquet_type is valid type
        self.reader = parquet_reader_new(<PyObject *>self.file_path, self.parquet_type)

    def __iter__(self):
        chunk = self._next_chunk()
        while chunk:
            yield chunk
            chunk = self._next_chunk()

    cpdef list _next_chunk(self):
        self._drop_chunk()
        self.chunk = parquet_reader_next_chunk(self.reader, self.parquet_type)
        
        if self.chunk.len == 0:
            return None # stop iteration

        return self._parse_chunk(self.chunk)
        
            

    cdef list _parse_chunk(self, CVec chunk):
        # Initialize Python objects from the rust vector.
        if self.parquet_type == ParquetType.QuoteTick:
            return _parse_quote_tick_chunk(chunk)
        else:
            raise RuntimeError("")

    def __del__(self) -> None:
        self._drop()

    cpdef void _drop(self):
        parquet_reader_drop(self.reader, self.parquet_type)
        self._drop_chunk()

    cpdef void _drop_chunk(self):
        # Drop the previous chunk
        parquet_reader_drop_chunk(self.chunk, self.parquet_type)




# cdef class Matrix:
#     cdef Py_ssize_t ncols
#     cdef Py_ssize_t shape[2]
#     cdef Py_ssize_t strides[2]
#     cdef vector[float] v

#     def __cinit__(self, Py_ssize_t ncols):
#         self.ncols = ncols

#     def add_row(self):
#         """Adds a row, initially zero-filled."""
#         self.v.resize(self.v.size() + self.ncols)

#     def __getbuffer__(self, Py_buffer *buffer, int flags):
#         cdef Py_ssize_t itemsize = sizeof(QuoteTick_t)

#         self.shape[0] = self.v.size() / self.ncols
#         self.shape[1] = self.ncols

#         # Stride 1 is the distance, in bytes, between two items in a row;
#         # this is the distance between two adjacent items in the vector.
#         # Stride 0 is the distance between the first elements of adjacent rows.
#         self.strides[1] = <Py_ssize_t>(  <char *>&(self.v[1])
#                                        - <char *>&(self.v[0]))
#         self.strides[0] = self.ncols * self.strides[1]

#         buffer.buf = <char *>&(self.v[0])
#         buffer.format = 'f'                     # float
#         buffer.internal = NULL                  # see References
#         buffer.itemsize = itemsize
#         buffer.len = self.v.size() * itemsize   # product(shape) * itemsize
#         buffer.ndim = 1
#         buffer.obj = self
#         buffer.readonly = 0
#         buffer.shape = self.shape
#         buffer.strides = self.strides
#         buffer.suboffsets = NULL                # for pointer arrays only

#     def __releasebuffer__(self, Py_buffer *buffer):
#         pass

# cdef class SimplestBuffer:
#     cdef:
#         void * ptr
#         uintptr_t len

#     def __getbuffer__(self, Py_buffer *buffer, int flags):
#         # if the requested buffer type is not PyBUF_SIMPLE then error out
#         # we will allow either readonly or writeable buffers however
#         # if flags != PyBUF_SIMPLE and flags != PyBUF_SIMPLE | PyBUF_WRITEABLE:
#         #     raise BufferError
#         buffer.buf = &(<char *>self.ptr)[0]           # points to our buffer memory
#         buffer.format = NULL                 # NULL format means bytes
#         buffer.internal = NULL               # this is for our own use if needed
#         buffer.itemsize = sizeof(QuoteTick_t)
#         buffer.len = self.len
#         buffer.ndim = 1
#         buffer.obj = self
#         buffer.readonly = True # not (flags & PyBUF_WRITEABLE)
#         buffer.shape = NULL                  # none of shapes, strides or offsets are used for PyBUF_SIMPLE
#         buffer.strides = NULL
#         buffer.suboffsets = NULL

#     # the buffer protocol requires this method
#     def __releasebuffer__(self, Py_buffer *buffer):
#         pass

# cdef class ArrayWrapper:
#     cdef void* data_ptr
#     cdef int size

#     cdef set_data(self, int size, void* data_ptr):
#         """ Constructor for the class.
#         Mallocs a memory buffer of size (n*sizeof(int)) and sets up
#         the numpy array.
#         Parameters:
#         -----------
#         n -- Length of the array.
#         Data attributes:
#         ----------------
#         data -- Pointer to an integer array.
#         alloc -- Size of the data buffer allocated.
#         """
#         self.data_ptr = data_ptr
#         self.size = size

#     def __array__(self):
#         cdef np.npy_intp shape[1]
#         shape[0] = <np.npy_intp> self.size
#         ndarray = np.PyArray_SimpleNewFromData(1, shape, np.NPY_INT, self.data_ptr)
#         return ndarray

#     def __dealloc__(self):
#         """ Frees the array. """
#         free(<void*>self.data_ptr)


# cdef list parse_quote_tick_vector(Vec_QuoteTick tick_vec):
#     cdef list ticks = []

#     cdef:
#         QuoteTick_t _mem
#         QuoteTick tick
#         uint64_t i
#     for i in range(0, tick_vec.len - 1):
#         tick = QuoteTick.__new__(QuoteTick)
#         tick.ts_event = _mem.ts_event
#         tick.ts_init = _mem.ts_init
#         tick._mem = index_quote_tick_vector(&tick_vec, i)[0]
#         ticks.append(tick)

#     return ticks

#     # _mem =
#
#     # _mem = (<char *>chunk.ptr) + (<char *>(i*sizeof(QuoteTick_t)))
#     # _mem = (<char *>((<char *>chunk.ptr) + i*sizeof(QuoteTick_t)))[0]
#     # iterate_chunk[0]
#     ticks.append(tick)

# tick._mem = (<QuoteTick_t  *>chunk.ptr)[0]

    # ticks.append(tick)

# QuoteTick_t tick
# def _initialize_objects() -> List[Data]:
#     if self.parquet_type == ParquetReaderType.QuoteTick:

# from cpython cimport Py_buffer
# from libcpp.vector cimport vector

# TODO(cs): Implement
# from libc.stdint cimport uint64_t
#
# from nautilus_trader.core.rust.model cimport Bar_t
# from nautilus_trader.core.rust.model cimport QuoteTick_t
# from nautilus_trader.core.rust.persistence cimport Vec_Bar
# from nautilus_trader.core.rust.persistence cimport Vec_QuoteTick
# from nautilus_trader.core.rust.persistence cimport index_bar_vector
# from nautilus_trader.core.rust.persistence cimport index_quote_tick_vector
# from nautilus_trader.model.data.bar cimport Bar
# from nautilus_trader.model.data.tick cimport QuoteTick
#
#

#
#
# cdef list parse_bar_vector(Vec_Bar bar_vec):
#     cdef list bars = []
#
#     cdef:
#         Bar_t _mem
#         Bar bar
#         uint64_t i
#     for i in range(0, bar_vec.len - 1):
#         bar = Bar.__new__(Bar)
#         bar.ts_event = _mem.ts_event
#         bar.ts_init = _mem.ts_init
#         bar._mem = index_bar_vector(&bar_vec, i)[0]
#         bars.append(bar)
#
#     return bars
