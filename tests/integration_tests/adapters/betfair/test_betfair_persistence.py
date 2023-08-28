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
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDeltas
from nautilus_trader.core.rust.model import BookAction
from nautilus_trader.core.rust.model import OrderSide
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.serialization.arrow.serializer import ArrowSerializer
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from tests.integration_tests.adapters.betfair.test_kit import betting_instrument
from tests.integration_tests.adapters.betfair.test_kit import load_betfair_data


class TestBetfairPersistence:
    def setup(self):
        self.catalog = data_catalog_setup(protocol="memory", path="/catalog")
        self.fs = self.catalog.fs
        self.instrument = betting_instrument()

    def test_bsp_delta_serialize(self):
        # Arrange
        bsp_delta = BSPOrderBookDeltas(
            instrument_id=self.instrument.id,
            deltas=[
                BSPOrderBookDelta(
                    instrument_id=self.instrument.id,
                    action=BookAction.UPDATE,
                    order=BookOrder(
                        price=Price.from_str("0.990099"),
                        size=Quantity.from_str("60.07"),
                        side=OrderSide.BUY,
                        order_id=1,
                    ),
                    ts_event=1635313844283000000,
                    ts_init=1635313844283000000,
                ),
            ],
        )

        # Act
        values = bsp_delta.to_dict(bsp_delta)

        # Assert
        assert bsp_delta.from_dict(values) == bsp_delta
        assert values["type"] == "BSPOrderBookDeltas"

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
        values = bsp.to_dict(bsp)
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
        serialized = ArrowSerializer.serialize(bsp)
        [result] = ArrowSerializer.deserialize(BetfairStartingPrice, serialized)

        # Assert
        assert result.bsp == bsp.bsp

    def test_query_custom_type(self):
        # Arrange
        load_betfair_data(self.catalog)

        # Act
        data = self.catalog.query(BetfairTicker)

        # Assert
        assert len(data) == 210
