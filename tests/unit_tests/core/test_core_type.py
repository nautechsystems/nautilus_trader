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

from frozendict import frozendict

from nautilus_trader.core.type import DataType
from nautilus_trader.core.type import MessageType
from nautilus_trader.core.type import TypeKey


class TestKeyType:
    def test_equality_when_types_not_equal_returns_false(self):
        # Arrange
        key1 = TypeKey(type=str)
        key2 = TypeKey(type=int)

        # Act, Assert
        assert key1 != key2

    def test_equality_when_types_equal_returns_true(self):
        # Arrange
        key1 = TypeKey(type=str)
        key2 = TypeKey(type=str)

        # Act, Assert
        assert key1 == key2

    def test_equality_when_definitions_different_returns_false(self):
        # Arrange
        key1 = TypeKey(type=str, spec={"category": 1})
        key2 = TypeKey(type=str, spec={"category": 2})

        # Act, Assert
        assert key1 != key2

    def test_equality_when_definitions_equal_returns_false(self):
        # Arrange
        key1 = TypeKey(type=str, spec={"category": 1})
        key2 = TypeKey(type=str, spec={"category": 1})

        # Act, Assert
        assert key1 == key2

    def test_key_is_immutable(self):
        # Arrange
        spec = {"category": 1}
        key = TypeKey(type=str, spec=spec)

        spec["category"] = 2  # <-- attempt to modify category

        # Assert
        assert key.key == frozenset({("category", 1)})  # <-- category immutable

    def test_hash(self):
        # Arrange
        key = TypeKey(type=str, spec={"category": 1})

        assert isinstance(hash(key), int)


class TestMessageType:
    def test_key(self):
        # Arrange
        msg_type = MessageType(type=str, header={"category": 1, "code": 0})

        # Act, Assert
        assert msg_type.key == frozenset({("code", 0), ("category", 1)})

    def test_header(self):
        # Arrange
        msg_type = MessageType(type=str, header={"category": 1, "code": 0})

        # Act, Assert
        assert msg_type.header == frozendict({"category": 1, "code": 0})

    def test_key_is_immutable(self):
        # Arrange
        spec = {"category": 1, "code": 0}
        msg_type = MessageType(type=str, header=spec)

        spec["category"] = 2  # <-- attempt to modify category

        # Assert
        assert msg_type.key == frozenset({("category", 1), ("code", 0)})  # <-- category immutable

    def test_hash_str_repr(self):
        # Arrange
        msg_type = MessageType(type=str, header={"category": 1, "code": 0})

        assert isinstance(hash(msg_type), int)
        assert str(msg_type) == "<str> {'category': 1, 'code': 0}"
        assert repr(msg_type) == "MessageType(type=str, header={'category': 1, 'code': 0})"


class TestDataType:
    def test_key(self):
        # Arrange
        data_type = DataType(type=str, metadata={"category": 1, "code": 0})

        # Act, Assert
        assert data_type.key == frozenset({("code", 0), ("category", 1)})

    def test_metadata(self):
        # Arrange
        data_type = DataType(type=str, metadata={"category": 1, "code": 0})

        # Act, Assert
        assert data_type.metadata == frozendict({"category": 1, "code": 0})

    def test_key_is_immutable(self):
        # Arrange
        spec = {"category": 1, "code": 0}
        data_type = DataType(type=str, metadata=spec)

        spec["category"] = 2  # <-- attempt to modify category

        # Assert
        assert data_type.key == frozenset({("category", 1), ("code", 0)})  # <-- category immutable

    def test_hash_str_repr(self):
        # Arrange
        data_type = DataType(type=str, metadata={"category": 1, "code": 0})

        assert isinstance(hash(data_type), int)
        assert str(data_type) == "<str> {'category': 1, 'code': 0}"
        assert repr(data_type) == "DataType(type=str, metadata={'category': 1, 'code': 0})"
