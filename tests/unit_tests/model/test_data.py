# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.data import Data
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.filters import NewsImpact


class TestDataType:
    def test_data_type_instantiation(self):
        # Arrange, Act
        data_type = DataType(Data, {"type": "NEWS_WIRE"})

        # Assert
        assert data_type.type == Data
        assert data_type.metadata == {"type": "NEWS_WIRE"}
        assert data_type.topic == "Data.type=NEWS_WIRE"
        assert str(data_type) == "Data{'type': 'NEWS_WIRE'}"
        assert repr(data_type) == "DataType(type=Data, metadata={'type': 'NEWS_WIRE'})"

    def test_data_type_instantiation_when_no_metadata(self):
        # Arrange, Act
        data_type = DataType(Data)

        # Assert
        assert data_type.type == Data
        assert data_type.metadata == {}
        assert data_type.topic == "Data*"
        assert str(data_type) == "Data"
        assert repr(data_type) == "DataType(type=Data, metadata={})"  # (P103??)

    def test_data_type_instantiation_with_multiple_metadata(self):
        # Arrange, Act
        data_type = DataType(Data, {"b": 2, "a": 1, "c": None})

        # Assert
        assert data_type.type == Data
        assert data_type.metadata == {"a": 1, "b": 2, "c": None}
        assert data_type.topic == "Data.b=2.a=1.c=*"
        assert str(data_type) == "Data{'b': 2, 'a': 1, 'c': None}"
        assert repr(data_type) == "DataType(type=Data, metadata={'b': 2, 'a': 1, 'c': None})"

    def test_data_type_equality_and_hash(self):
        # Arrange, Act
        data_type1 = DataType(Data, {"type": "NEWS_WIRE", "topic": "Earthquake"})
        data_type2 = DataType(Data, {"type": "NEWS_WIRE", "topic": "Flood"})
        data_type3 = DataType(Data, {"type": "FED_DATA", "topic": "NonFarmPayroll"})

        # Assert
        assert data_type1 == data_type1
        assert data_type1 != data_type2
        assert data_type1 != data_type2
        assert data_type1 != data_type3
        assert isinstance(hash(data_type1), int)

    def test_data_type_comparison(self):
        # Arrange, Act
        data_type1 = DataType(Data, {"type": "NEWS_WIRE", "topic": "Earthquake"})
        data_type2 = DataType(Data, {"type": "NEWS_WIRE", "topic": "Flood"})
        data_type3 = DataType(Data, {"type": "FED_DATA", "topic": "NonFarmPayroll"})

        # Assert
        assert data_type1 <= data_type1
        assert data_type1 < data_type2
        assert data_type2 > data_type1
        assert data_type1 >= data_type3

    def test_data_type_as_key_in_dict(self):
        # Arrange, Act
        data_type = DataType(Data, {"type": "NEWS_WIRE", "topic": "Earthquake"})

        hash_map = {data_type: []}

        # Assert
        assert data_type in hash_map

    def test_data_instantiation(self):
        # Arrange, Act
        data_type = DataType(NewsEvent, {"publisher": "NEWS_WIRE"})
        data = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=USD,
            ts_event=0,
            ts_init=0,
        )
        custom_data = CustomData(data_type, data)

        # Assert
        assert custom_data.data_type == data_type
        assert custom_data.data == data

    def test_equality_when_types_not_equal_returns_false(self):
        # Arrange
        data_type1 = DataType(type=QuoteTick)
        data_type2 = DataType(type=Data)

        # Act, Assert
        assert data_type1 != data_type2

    def test_equality_when_types_equal_returns_true(self):
        # Arrange
        data_type1 = DataType(type=Data)
        data_type2 = DataType(type=Data)

        # Act, Assert
        assert data_type1 == data_type2

    def test_equality_when_definitions_different_returns_false(self):
        # Arrange
        data_type1 = DataType(type=Data, metadata={"category": 1})
        data_type2 = DataType(type=Data, metadata={"category": 2})

        # Act, Assert
        assert data_type1 != data_type2

    def test_equality_when_definitions_equal_returns_false(self):
        # Arrange
        data_type1 = DataType(type=Data, metadata={"category": 1})
        data_type2 = DataType(type=Data, metadata={"category": 1})

        # Act, Assert
        assert data_type1 == data_type2

    def test_metadata(self):
        # Arrange
        data_type = DataType(type=Data, metadata={"category": 1, "code": 0})

        # Act, Assert
        assert data_type.metadata == {"category": 1, "code": 0}

    def test_hash_str_repr(self):
        # Arrange
        data_type = DataType(type=Data, metadata={"category": 1, "code": 0})

        # Act, Assert
        assert isinstance(hash(data_type), int)
        assert str(data_type) == "Data{'category': 1, 'code': 0}"
        assert repr(data_type) == "DataType(type=Data, metadata={'category': 1, 'code': 0})"
