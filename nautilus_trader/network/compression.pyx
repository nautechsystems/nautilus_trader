# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import lz4.block
import lz4.frame
import py_snappy


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


cdef class BypassCompressor(Compressor):
    """
    Provides a compressor bypass which just returns the give data source.
    """

    cpdef bytes compress(self, bytes data):
        """
        Bypasses compression by simply returning the given data.

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
        return lz4.block.compress(data, mode='fast')

    cpdef bytes decompress(self, bytes data):
        """
        Decompress the given data.

        :param data: The data to decompress.
        :return bytes.
        """
        return lz4.block.decompress(data)

    cpdef bytes compress_frame(self, bytes data):
        """
        Compress the given data.

        :param data: The data to compress.
        :return bytes.
        """
        return lz4.frame.compress(data)

    cpdef bytes decompress_frame(self, bytes data):
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
        return py_snappy.compress(data)

    cpdef bytes decompress(self, bytes data):
        """
        Decompress the given data.

        :param data: The data to decompress.
        :return bytes.
        """
        return py_snappy.decompress(data)
