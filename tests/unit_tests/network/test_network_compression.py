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

import unittest
from base64 import b64encode

from nautilus_trader.network.compression import BypassCompressor, LZ4Compressor


class CompressorTests(unittest.TestCase):

    def test_compressor_bypass_returns_given_bytes(self):
        # Arrange
        message = b"hello world!"
        compressor = BypassCompressor()

        # Act
        compressed = compressor.compress(message)
        decompressed = compressor.decompress(compressed)

        # Assert
        self.assertEqual(message, decompressed)

    def test_lz4_compressor_can_compress_and_decompress_frame(self):
        # Arrange
        message = b"hello world!"
        compressor = LZ4Compressor()

        # Act
        compressed = compressor.compress(message)
        decompressed = compressor.decompress(compressed)

        # Assert
        self.assertEqual(message, decompressed)
        print(b64encode(compressed))

    def test_lz4_compressor_can_decompress_frame_from_csharp(self):
        # Arrange
        csharp_hex = "04-22-4D-18-40-40-C0-0C-00-00-80-68-65-6C-6C-6F-20-77-6F-72-6C-64-21-00-00-00-00"
        hex_from_csharp = bytes(bytearray.fromhex(csharp_hex.replace('-', ' ')))
        print(hex_from_csharp)
        compressor = LZ4Compressor()

        # Act
        decompressed = compressor.decompress(hex_from_csharp)

        # Assert
        self.assertEqual(b'hello world!', decompressed)
