# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.core.uuid import UUID


class TestUUID:
    def test_instantiate_with_invalid_bytes_length_raises_exception(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            UUID(b"\x12\x34\x56\x78" * 8)

    @pytest.mark.parametrize(
        "value",
        ["", "12345678-1234-5678-1234-567812345678-99"],
    )
    def test_from_str_with_invalid_strings_raises_exception(self, value):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            UUID.from_str(value)

    def test_instantiate_with_valid_bytes(self):
        # Arrange, Act
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Assert
        assert isinstance(uuid, UUID)
        assert repr(uuid) == "UUID('12345678-1234-5678-1234-567812345678')"
        assert str(uuid) == "12345678-1234-5678-1234-567812345678"
        assert uuid.int_val == 24197857161011715162171839636988778104

    def test_equality(self):
        # Arrange, Act
        uuid1 = UUID(value=b"\x12\x34\x56\x78" * 4)
        uuid2 = UUID(value=b"\x12\x34\x56\x78" * 4)
        uuid3 = UUID(value=b"\x34\x56\x78\x99" * 4)

        # Assert
        assert uuid1 == uuid1
        assert uuid1 == uuid2
        assert uuid2 != uuid3

    def test_comparison(self):
        # Arrange, Act
        uuid1 = UUID(value=b"\x12\x34\x56\x78" * 4)
        uuid2 = UUID(value=b"\x34\x56\x78\x99" * 4)

        # Assert
        assert uuid1 <= uuid1
        assert uuid1 < uuid2
        assert uuid2 >= uuid2
        assert uuid2 > uuid1

    def test_hash(self):
        # Arrange
        uuid1 = UUID(value=b"\x12\x34\x56\x78" * 4)
        uuid2 = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert isinstance((hash(uuid1)), int)
        assert hash(uuid1) == hash(uuid2)

    def test_int(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert int(uuid) == 24197857161011715162171839636988778104
        assert uuid.int_val == 24197857161011715162171839636988778104

    def test_bytes(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert b"\x124Vx\x124Vx\x124Vx\x124Vx" == uuid.bytes

    def test_bytes_le(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert b"xV4\x124\x12xV\x124Vx\x124Vx" == uuid.bytes_le

    def test_fields(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.fields == (305419896, 4660, 22136, 18, 52, 95073701484152)

    def test_time_low(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.time_low == 305419896

    def test_time_mid(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.time_mid == 4660

    def test_time_high_version(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.time_hi_version == 22136

    def test_clock_seq_hi_variant(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.clock_seq_hi_variant == 18

    def test_clock_seq_low(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.clock_seq_low == 52

    def test_time(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.time == 466142576285865592

    def test_clock_seq(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.clock_seq == 4660

    def test_node(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.node == 95073701484152

    def test_hex(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.hex == "12345678123456781234567812345678"

    def test_urn(self):
        # Arrange
        uuid = UUID(value=b"\x12\x34\x56\x78" * 4)

        # Act, Assert
        assert uuid.urn == "urn:uuid:12345678-1234-5678-1234-567812345678"
