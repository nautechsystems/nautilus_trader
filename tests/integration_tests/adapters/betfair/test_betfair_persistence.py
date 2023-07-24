# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDeltas
from nautilus_trader.persistence.catalog.parquet.serializers import ParquetSerializer
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.providers import TestInstrumentProvider


@pytest.mark.skip(reason="Reimplementing")
class TestBetfairPersistence:
    def setup(self):
        self.catalog = data_catalog_setup(protocol="memory")
        self.fs = self.catalog.fs
        self.instrument = TestInstrumentProvider.betting_instrument()

    def test_bsp_delta_serialize(self):
        # Arrange
        bsp_delta = BSPOrderBookDeltas.from_dict(
            {
                "type": "BSPOrderBookDelta",
                "instrument_id": self.instrument.id.value,
                "action": "UPDATE",
                "price": 0.990099,
                "size": 60.07,
                "side": "BUY",
                "order_id": 1635313844283000000,
                "ts_event": 1635313844283000000,
                "ts_init": 1635313844283000000,
            },
        )

        # Act
        values = bsp_delta.to_dict(bsp_delta)

        # Assert
        assert bsp_delta.from_dict(values) == bsp_delta
        assert values["type"] == "BSPOrderBookDelta"

    def test_betfair_starting_price_to_from_dict(self):
        # Arrange
        bsp = BetfairStartingPrice.from_dict(
            {
                "type": "BetfairStartingPrice",
                "instrument_id": self.instrument.id.value,
                "bsp": 1.20,
                "ts_event": 1635313844283000000,
                "ts_init": 1635313844283000000,
            },
        )

        # Act
        values = bsp.to_dict()
        result = bsp.from_dict(values)

        # Assert
        assert values["type"] == "BetfairStartingPrice"
        assert result.bsp == bsp.bsp

    def test_betfair_starting_price_serialization(self):
        # Arrange
        bsp = BetfairStartingPrice.from_dict(
            {
                "type": "BetfairStartingPrice",
                "instrument_id": self.instrument.id.value,
                "bsp": 1.20,
                "ts_event": 1635313844283000000,
                "ts_init": 1635313844283000000,
            },
        )

        # Act
        serialized = ParquetSerializer.serialize(bsp)
        [result] = ParquetSerializer.deserialize(BetfairStartingPrice, [serialized])

        # Assert
        assert result.bsp == bsp.bsp

    def test_bsp_deltas(self, load_betfair_data):
        # Arrange

        # Act
        data = self.catalog.query(BSPOrderBookDeltas)

        # Assert
        assert len(data) == 2824
