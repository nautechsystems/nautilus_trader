# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
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
