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

from parameterized import parameterized

from nautilus_trader.core.uuid import UUID


class UUIDTests(unittest.TestCase):

    def test_instantiate_with_invalid_bytes_length_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, UUID, b'\x12\x34\x56\x78' * 8)

    @parameterized.expand([
        [""],
        ["12345678-1234-5678-1234-567812345678-99"]
    ])
    def test_from_str_with_invalid_strings_raises_exception(self, value):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, UUID.from_str, value)

    def test_instantiate_with_valid_bytes(self):
        # Arrange
        # Act
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Assert
        self.assertTrue(isinstance(uuid, UUID))
        self.assertEqual("UUID(\'12345678-1234-5678-1234-567812345678\')", repr(uuid))
        self.assertEqual("12345678-1234-5678-1234-567812345678", str(uuid))
        self.assertEqual(24197857161011715162171839636988778104, uuid.int_val)

    def test_equality(self):
        # Arrange
        # Act
        uuid1 = UUID(value=b'\x12\x34\x56\x78' * 4)
        uuid2 = UUID(value=b'\x12\x34\x56\x78' * 4)
        uuid3 = UUID(value=b'\x34\x56\x78\x99' * 4)

        # Assert
        self.assertEqual(uuid1, uuid1)
        self.assertEqual(uuid1, uuid2)
        self.assertNotEqual(uuid2, uuid3)

    def test_comparison(self):
        # Arrange
        # Act
        uuid1 = UUID(value=b'\x12\x34\x56\x78' * 4)
        uuid2 = UUID(value=b'\x34\x56\x78\x99' * 4)

        # Assert
        self.assertTrue(uuid1 <= uuid1)
        self.assertTrue(uuid1 < uuid2)
        self.assertTrue(uuid2 >= uuid2)
        self.assertTrue(uuid2 > uuid1)

    def test_hash(self):
        # Arrange
        uuid1 = UUID(value=b'\x12\x34\x56\x78' * 4)
        uuid2 = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(int, type(hash(uuid1)))
        self.assertEqual(hash(uuid1), hash(uuid2))

    def test_int(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(24197857161011715162171839636988778104, int(uuid))
        self.assertEqual(24197857161011715162171839636988778104, uuid.int_val)

    def test_bytes(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(b'\x124Vx\x124Vx\x124Vx\x124Vx', uuid.bytes)

    def test_bytes_le(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(b'xV4\x124\x12xV\x124Vx\x124Vx', uuid.bytes_le)

    def test_fields(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual((305419896, 4660, 22136, 18, 52, 95073701484152), uuid.fields)

    def test_time_low(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(305419896, uuid.time_low)

    def test_time_mid(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(4660, uuid.time_mid)

    def test_time_high_version(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(22136, uuid.time_hi_version)

    def test_clock_seq_hi_variant(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(18, uuid.clock_seq_hi_variant)

    def test_clock_seq_low(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(52, uuid.clock_seq_low)

    def test_time(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(466142576285865592, uuid.time)

    def test_clock_seq(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(4660, uuid.clock_seq)

    def test_node(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual(95073701484152, uuid.node)

    def test_hex(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual("12345678123456781234567812345678", uuid.hex)

    def test_urn(self):
        # Arrange
        uuid = UUID(value=b'\x12\x34\x56\x78' * 4)

        # Act
        # Assert
        self.assertEqual("urn:uuid:12345678-1234-5678-1234-567812345678", uuid.urn)
