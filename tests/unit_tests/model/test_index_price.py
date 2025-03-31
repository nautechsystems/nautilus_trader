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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import IndexPriceUpdate
from nautilus_trader.model.objects import Price
from nautilus_trader.test_kit.providers import TestInstrumentProvider


BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestTradeTick:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert (
            IndexPriceUpdate.fully_qualified_name() == "nautilus_trader.model.data:IndexPriceUpdate"
        )

    def test_hash_str_and_repr(self):
        # Arrange
        index_price = IndexPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act, Assert
        assert isinstance(hash(index_price), int)
        assert str(index_price) == "BTCUSDT.BINANCE,100000.00,1,2"
        assert repr(index_price) == "IndexPriceUpdate(BTCUSDT.BINANCE,100000.00,1,2)"

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        index_price = IndexPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = IndexPriceUpdate.to_dict(index_price)

        # Assert
        assert result == {
            "type": "IndexPriceUpdate",
            "instrument_id": "BTCUSDT.BINANCE",
            "value": "100000.00",
            "ts_event": 1,
            "ts_init": 2,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        index_price = IndexPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = IndexPriceUpdate.from_dict(IndexPriceUpdate.to_dict(index_price))

        # Assert
        assert result == index_price

    def test_from_pyo3(self):
        # Arrange
        index_price = IndexPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        pyo3_index_price = index_price.to_pyo3()
        result = IndexPriceUpdate.from_pyo3(pyo3_index_price)

        # Assert
        assert result == index_price

    def test_to_pyo3(self):
        # Arrange
        index_price = IndexPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        pyo3_index_price = index_price.to_pyo3()

        # Assert
        assert isinstance(pyo3_index_price, nautilus_pyo3.IndexPriceUpdate)
        assert pyo3_index_price.value == nautilus_pyo3.Price.from_str("100_000.00")
        assert pyo3_index_price.ts_event == 1
        assert pyo3_index_price.ts_init == 2
