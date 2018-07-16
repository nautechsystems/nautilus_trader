#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_messaging.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import msgpack

UTF8 = 'utf8'


class MessagingTests(unittest.TestCase):

    def test_can_serialize_event(self):
        # Arrange
        message = {
            'header': 'order_cancelled',
            'symbol': 'AUDUSD.FXCM',
            'order_id': 'O123456',
            'timestamp': '1970-01-01T00:00:00.000Z'}

        # Act
        packed = msgpack.packb(message)
        unpacked = msgpack.unpackb(packed)

        print('\n')
        print(packed.hex())
        self.assertEqual(4, len(unpacked))
        self.assertEqual(unpacked['header'.encode(UTF8)].decode(UTF8), 'order_cancelled')

    def test_can_deserialize_from_c_sharp_msg_pack(self):
        # Arrange
        # From C# MsgPack.Cli
        # {b'header': b'order_cancelled'}
        hex_string = '81a6686561646572af6f726465725f63616e63656c6c6564'
        data = bytes.fromhex(hex_string)

        # Act
        unpacked = msgpack.unpackb(data)

        # Assert
        self.assertEqual(1, len(unpacked))
        self.assertEqual(unpacked['header'.encode(UTF8)].decode(UTF8), 'order_cancelled')
