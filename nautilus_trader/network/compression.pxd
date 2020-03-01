# -------------------------------------------------------------------------------------------------
# <copyright file="compression.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cdef class Compressor:
    cpdef bytes compress(self, bytes data)
    cpdef bytes decompress(self, bytes data)


cdef class CompressorBypass(Compressor):
    cpdef bytes compress(self, bytes data)
    cpdef bytes decompress(self, bytes data)


cdef class LZ4Compressor(Compressor):
    cpdef bytes compress(self, bytes data)
    cpdef bytes decompress(self, bytes data)


cdef class SnappyCompressor(Compressor):
    cpdef bytes compress(self, bytes data)
    cpdef bytes decompress(self, bytes data)
