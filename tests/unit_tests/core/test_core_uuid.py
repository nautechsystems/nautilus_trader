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

from nautilus_trader.core.uuid import UUID, uuid1, uuid3, uuid4, uuid5


class UUIDTests(unittest.TestCase):

    def test_create_uuid_from_hex_string_value(self):
        # Arrange
        # Act
        uuid_object1 = UUID("{12345678-1234-5678-1234-567812345678}")
        uuid_object2 = UUID("12345678123456781234567812345678")
        uuid_object3 = UUID("urn:uuid:12345678-1234-5678-1234-567812345678")

        # Assert
        self.assertEqual("UUID(\'12345678-1234-5678-1234-567812345678\')", repr(uuid_object1))
        self.assertEqual("UUID(\'12345678-1234-5678-1234-567812345678\')", repr(uuid_object2))
        self.assertEqual("UUID(\'12345678-1234-5678-1234-567812345678\')", repr(uuid_object3))

    def test_create_uuid_from_bytes_val(self):
        # Arrange
        # Act
        uuid_object = UUID(bytes_val=b'\x12\x34\x56\x78' * 4)

        # Assert
        self.assertEqual("UUID(\'12345678-1234-5678-1234-567812345678\')", repr(uuid_object))

    def test_create_uuid_from_little_endian_bytes_le(self):
        # Arrange
        # Act
        uuid_object = UUID(bytes_le=b'\x78\x56\x34\x12\x34\x12\x78\x56\x12\x34\x56\x78\x12\x34\x56\x78')

        # Assert
        self.assertEqual("UUID(\'12345678-1234-5678-1234-567812345678\')", repr(uuid_object))

    def test_create_uuid_from_fields_tuple(self):
        # Arrange
        # Act
        uuid_object = UUID(fields=(0x12345678, 0x1234, 0x5678, 0x12, 0x34, 0x567812345678))

        # Assert
        self.assertEqual("UUID(\'12345678-1234-5678-1234-567812345678\')", repr(uuid_object))

    def test_create_uuid_from_int_val(self):
        # Arrange
        # Act
        uuid_object = UUID(int_val=0x12345678123456781234567812345678)

        # Assert
        self.assertEqual("UUID(\'12345678-1234-5678-1234-567812345678\')", repr(uuid_object))

    def test_create_uuid1(self):
        # Arrange
        # Act
        uuid_object = uuid1()

        # Assert
        self.assertTrue(isinstance(uuid_object, UUID))

    def test_create_uuid3(self):
        # Arrange
        # Act
        uuid_object = uuid3(UUID("6ba7b810-9dad-11d1-80b4-00c04fd430c8"), "some_name")

        # Assert
        self.assertTrue(isinstance(uuid_object, UUID))

    def test_create_uuid4(self):
        # Arrange
        # Act
        uuid_object1 = uuid4()
        uuid_object2 = uuid4()

        # Assert
        self.assertTrue(isinstance(uuid_object1, UUID))
        self.assertTrue(isinstance(uuid_object2, UUID))
        self.assertNotEqual(uuid_object1, uuid_object2)

    def test_create_uuid5(self):
        # Arrange
        # Act
        uuid_object = uuid5(UUID("6ba7b810-9dad-11d1-80b4-00c04fd430c8"), "some_name")

        # Assert
        self.assertTrue(isinstance(uuid_object, UUID))
