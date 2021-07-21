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

from nautilus_trader.model.data.base import DataType


class TestDataType:
    def test_equality_when_types_not_equal_returns_false(self):
        # Arrange
        data_type1 = DataType(type=str)
        data_type2 = DataType(type=int)

        # Act, Assert
        assert data_type1 != data_type2

    def test_equality_when_types_equal_returns_true(self):
        # Arrange
        data_type1 = DataType(type=str)
        data_type2 = DataType(type=str)

        # Act, Assert
        assert data_type1 == data_type2

    def test_equality_when_definitions_different_returns_false(self):
        # Arrange
        data_type1 = DataType(type=str, metadata={"category": 1})
        data_type2 = DataType(type=str, metadata={"category": 2})

        # Act, Assert
        assert data_type1 != data_type2

    def test_equality_when_definitions_equal_returns_false(self):
        # Arrange
        data_type1 = DataType(type=str, metadata={"category": 1})
        data_type2 = DataType(type=str, metadata={"category": 1})

        # Act, Assert
        assert data_type1 == data_type2

    def test_metadata(self):
        # Arrange
        data_type = DataType(type=str, metadata={"category": 1, "code": 0})

        # Act, Assert
        assert data_type.metadata == {"category": 1, "code": 0}

    def test_hash_str_repr(self):
        # Arrange
        data_type = DataType(type=str, metadata={"category": 1, "code": 0})

        # Act, Assert
        assert isinstance(hash(data_type), int)
        assert str(data_type) == "<str> {'category': 1, 'code': 0}"
        assert repr(data_type) == "DataType(type=str, metadata={'category': 1, 'code': 0})"
