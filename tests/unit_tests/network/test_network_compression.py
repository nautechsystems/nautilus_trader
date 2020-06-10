# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import unittest
from base64 import b64encode

from nautilus_trader.network.compression import CompressorBypass, SnappyCompressor, LZ4Compressor


class CompressorTests(unittest.TestCase):

    def test_compressor_bypass_returns_given_bytes(self):
        # Arrange
        message = b'hello world!'
        compressor = CompressorBypass()

        # Act
        compressed = compressor.compress(message)
        decompressed = compressor.decompress(compressed)

        # Assert
        self.assertEqual(message, decompressed)

    def test_snappy_compressor_can_compress_and_decompress(self):
        # Arrange
        message = b'hello world!'
        compressor = SnappyCompressor()

        # Act
        compressed = compressor.compress(message)
        decompressed = compressor.decompress(compressed)

        # Assert
        self.assertEqual(message, decompressed)
        print(b64encode(compressed))

    def test_snappy_compressor_can_decompress_from_csharp(self):
        # Arrange
        hex_from_csharp = bytes(bytearray.fromhex('0C-2C-68-65-6C-6C-6F-20-77-6F-72-6C-64-21'.replace('-', ' ')))
        compressor = SnappyCompressor()

        # Act
        decompressed = compressor.decompress(hex_from_csharp)

        # Assert
        self.assertEqual(b'hello world!', decompressed)

    def test_lz4_compressor_can_compress_and_decompress(self):
        # Arrange
        message = b'hello world!'
        compressor = LZ4Compressor()

        # Act
        compressed = compressor.compress(message)
        decompressed = compressor.decompress(compressed)

        # Assert
        self.assertEqual(message, decompressed)
        print(b64encode(compressed))

    # def test_lz4_compressor_can_decompress_from_csharp(self):
    #     # Arrange
    #     hex_from_csharp = bytes(bytearray.fromhex('00-68-65-6C-6C-6F-20-77-6F-72-6C-64-21'.replace('-', ' ')))
    #     compressor = LZ4Compressor()
    #
    #     # Act
    #     decompressed = compressor.decompress(hex_from_csharp)
    #
    #     # Assert
    #     self.assertEqual(b'hello world!', decompressed)
