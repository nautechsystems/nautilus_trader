# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import lz4.frame
import snappy


cdef class Compressor:
    """
    The base class for all data compressors.
    """

    cpdef bytes compress(self, bytes data):
        """
        Compress the given data.

        :param data: The data to compress.
        :return bytes.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bytes decompress(self, bytes data):
        """
        Decompress the given data.

        :param data: The data to decompress.
        :return bytes.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class CompressorBypass(Compressor):
    """
    Provides a compressor bypass which just returns the give data source.
    """

    cpdef bytes compress(self, bytes data):
        """
        Compress the given data.

        :param data: The data source to compress.
        :return bytes.
        """
        return data

    cpdef bytes decompress(self, bytes data):
        """
        Bypasses compression by simply returning the given data.

        :param data: The data source to decompress.
        :return bytes.
        """
        return data


cdef class LZ4Compressor(Compressor):
    """
    Provides a compressor for the LZ4 block specification.
    """

    cpdef bytes compress(self, bytes data):
        """
        Compress the given data.

        :param data: The data to compress.
        :return bytes.
        """
        return lz4.frame.compress(data)

    cpdef bytes decompress(self, bytes data):
        """
        Decompress the given data.

        :param data: The data to decompress.
        :return bytes.
        """
        return lz4.frame.decompress(data)


cdef class SnappyCompressor(Compressor):
    """
    Provides a compressor for the Snappy specification.
    """

    cpdef bytes compress(self, bytes data):
        """
        Compress the given data.

        :param data: The data to compress.
        :return bytes.
        """
        return snappy.compress(data)

    cpdef bytes decompress(self, bytes data):
        """
        Decompress the given data.

        :param data: The data to decompress.
        :return bytes.
        """
        return snappy.decompress(data)
