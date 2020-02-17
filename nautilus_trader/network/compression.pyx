# -------------------------------------------------------------------------------------------------
# <copyright file="compression.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import snappy


cdef class Compressor:
    """
    The base class for all data compressors.
    """

    cpdef bytes compress(self, bytes source):
        """
        Compress the given data.

        :param source: The data source to compress.
        :return bytes.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bytes decompress(self, bytes source):
        """
        Decompress the given data.

        :param source: The data source to decompress.
        :return bytes.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class SnappyCompressor(Compressor):
    """
    Provides a compressor for the Snappy specification.
    """

    cpdef bytes compress(self, bytes source):
        """
        Compress the given data.

        :param source: The data source to compress.
        :return bytes.
        """
        return snappy.compress(source)

    cpdef bytes decompress(self, bytes source):
        """
        Decompress the given data.

        :param source: The data source to decompress.
        :return bytes.
        """
        return snappy.decompress(source)
