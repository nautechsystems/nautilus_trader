# -------------------------------------------------------------------------------------------------
# <copyright file="test_network_compression.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
from base64 import b64encode

from nautilus_trader.network.compression import SnappyCompressor


class CompressorTests(unittest.TestCase):

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
