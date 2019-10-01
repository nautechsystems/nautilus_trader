# -------------------------------------------------------------------------------------------------
# <copyright file="test_serialization_data.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
from base64 import b64encode, b64decode

from nautilus_trader.common.clock import *
from nautilus_trader.model.objects import *
from nautilus_trader.serialization.data import BsonDataSerializer, DataMapper, BsonInstrumentSerializer

from test_kit.stubs import TestStubs
from test_kit.data import TestDataProvider

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()


class DataSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.mapper = DataMapper()
        self.serializer = BsonDataSerializer()
        self.test_ticks = TestDataProvider.usdjpy_test_ticks()

    def test_data_serializer_serialize_with_empty_dict_returns_empty_bson_doc(self):
        # Arrange
        serializer = BsonDataSerializer()

        # Act
        result = serializer.serialize({})

        # Assert
        self.assertEqual(b'\x05\x00\x00\x00\x00', result)

    def test_data_serializer_deserialize_with_empty_bson_doc_returns_empty_dict(self):
        # Arrange
        serializer = BsonDataSerializer()
        empty = serializer.serialize({})

        # Act
        result = serializer.deserialize(empty)

        # Assert
        self.assertEqual({}, result)

    def test_can_serialize_and_deserialize_ticks(self):
        # Arrange
        tick = Tick(AUDUSD_FXCM,
                    Price('1.00000'),
                    Price('1.00001'),
                    UNIX_EPOCH)

        data = self.mapper.map_ticks([tick])

        # Act
        serialized = self.serializer.serialize(data)

        print(type(data))
        print(data)
        print(type(serialized))
        deserialized = self.serializer.deserialize(serialized)

        print(deserialized)

        # Assert
        self.assertEqual(data, deserialized)

    def test_can_serialize_and_deserialize_bars(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        bar1 = Bar(Price('1.00001'),
                   Price('1.00004'),
                   Price('1.00002'),
                   Price('1.00003'),
                   100000,
                   UNIX_EPOCH)

        data = self.mapper.map_bars([bar1, bar1], bar_type)

        # Act
        serialized = self.serializer.serialize(data)

        print(type(data))
        print(data)
        print(type(serialized))
        deserialized = self.serializer.deserialize(serialized)

        print(deserialized)

        # Assert
        self.assertEqual(data, deserialized)

    def test_can_serialize_and_deserialize_instruments(self):
        # Arrange
        # Base64 bytes string from C# MongoDB.Bson
        base64 = 'mwEAAAJTeW1ib2wADAAAAEFVRFVTRC5GWENNAAJCcm9rZXJTeW1ib2wACAAAAEFVRC9VU0QAAkJhc2VDdXJyZW5jeQAEAAAAQVVEAAJTZWN1cml0eVR5cGUABgAAAEZPUkVYABBUaWNrUHJlY2lzaW9uAAUAAAATVGlja1NpemUAAQAAAAAAAAAAAAAAAAA2MBBSb3VuZExvdFNpemUA6AMAABBNaW5TdG9wRGlzdGFuY2VFbnRyeQAAAAAAEE1pblN0b3BEaXN0YW5jZQAAAAAAEE1pbkxpbWl0RGlzdGFuY2VFbnRyeQAAAAAAEE1pbkxpbWl0RGlzdGFuY2UAAAAAABBNaW5UcmFkZVNpemUAAQAAABBNYXhUcmFkZVNpemUAgPD6AhNSb2xsb3ZlckludGVyZXN0QnV5AAEAAAAAAAAAAAAAAAAAQDATUm9sbG92ZXJJbnRlcmVzdFNlbGwAAQAAAAAAAAAAAAAAAABAMAJUaW1lc3RhbXAAGQAAADE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWgAA'
        encoded = b64decode(base64)

        # Act
        serializer = BsonInstrumentSerializer()
        deserialized = serializer.deserialize(encoded)

        # Assert
        self.assertEqual('AUDUSD.FXCM', deserialized.symbol.value)

    def test_can_deserialize_tick_data_response_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        bar_type = TestStubs.bartype_audusd_1min_bid()
        bar1 = Bar(Price('1.00001'),
                   Price('1.00004'),
                   Price('1.00002'),
                   Price('1.00003'),
                   100000,
                   UNIX_EPOCH)

        data = self.mapper.map_bars([bar1, bar1], bar_type)

        # Act
        serialized = self.serializer.serialize(data)

        print(type(data))
        print(data)
        print(type(serialized))
        deserialized = self.serializer.deserialize(serialized)

        print(deserialized)

        # Assert
        self.assertEqual(data, deserialized)
