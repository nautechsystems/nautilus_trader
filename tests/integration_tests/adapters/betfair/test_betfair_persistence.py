# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.betfair.data_types import BetfairOrderVoided
from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.core.rust.model import BookAction
from nautilus_trader.core.rust.model import OrderSide
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.serialization.arrow.serializer import ArrowSerializer
from nautilus_trader.test_kit.mocks.data import setup_catalog
from tests.integration_tests.adapters.betfair.test_kit import betting_instrument
from tests.integration_tests.adapters.betfair.test_kit import load_betfair_data


class TestBetfairPersistence:
    @pytest.fixture(autouse=True)
    def setup_method(self, tmp_path):
        self.catalog = setup_catalog(protocol="memory", path=tmp_path / "catalog")
        self.fs = self.catalog.fs
        self.instrument = betting_instrument()

    def test_bsp_delta_serialize(self):
        # Arrange
        bsp_delta = BSPOrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                price=Price.from_str("0.990099"),
                size=Quantity.from_str("60.07"),
                side=OrderSide.BUY,
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=1635313844283000000,
            ts_init=1635313844283000000,
        )

        # Act
        self.catalog.write_data([bsp_delta, bsp_delta])
        values = self.catalog.custom_data(BSPOrderBookDelta)

        # Assert
        assert len(values) == 2
        assert values[1] == bsp_delta

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

    def test_betfair_order_voided_to_from_dict(self):
        # Arrange
        voided = BetfairOrderVoided.from_dict(
            {
                "type": "BetfairOrderVoided",
                "instrument_id": self.instrument.id.value,
                "client_order_id": "test-order-123",
                "venue_order_id": "248485109136",
                "size_voided": 50.0,
                "price": 1.50,
                "size": 100.0,
                "side": "B",
                "avg_price_matched": 1.50,
                "size_matched": 50.0,
                "reason": None,
                "ts_event": 1635313844283000000,
                "ts_init": 1635313844283000000,
            },
        )

        # Act
        values = voided.to_dict(voided)
        result = BetfairOrderVoided.from_dict(values)

        # Assert
        assert values["type"] == "BetfairOrderVoided"
        assert result.instrument_id == voided.instrument_id
        assert result.client_order_id == voided.client_order_id
        assert result.venue_order_id == voided.venue_order_id
        assert result.size_voided == voided.size_voided

    def test_betfair_order_voided_serialization(self):
        # Arrange
        voided = BetfairOrderVoided.from_dict(
            {
                "type": "BetfairOrderVoided",
                "instrument_id": self.instrument.id.value,
                "client_order_id": "test-order-123",
                "venue_order_id": "248485109136",
                "size_voided": 50.0,
                "price": 2.0,
                "size": 100.0,
                "side": "L",
                "reason": "VAR_DECISION",
                "ts_event": 1635313844283000000,
                "ts_init": 1635313844283000000,
            },
        )

        # Act
        serialized = ArrowSerializer.serialize(voided)
        [result] = ArrowSerializer.deserialize(BetfairOrderVoided, serialized)

        # Assert
        assert result.instrument_id == voided.instrument_id
        assert result.client_order_id == voided.client_order_id
        assert result.venue_order_id == voided.venue_order_id
        assert result.size_voided == voided.size_voided
        assert result.reason == voided.reason

    def test_betfair_order_voided_catalog_write_read(self):
        # Arrange
        voided1 = BetfairOrderVoided(
            instrument_id=self.instrument.id,
            client_order_id="test-order-1",
            venue_order_id="248485109136",
            size_voided=50.0,
            price=1.50,
            size=100.0,
            side="B",
            avg_price_matched=1.50,
            size_matched=50.0,
            reason=None,
            ts_event=1635313844283000000,
            ts_init=1635313844283000001,
        )
        voided2 = BetfairOrderVoided(
            instrument_id=self.instrument.id,
            client_order_id="test-order-2",
            venue_order_id="248485109137",
            size_voided=25.0,
            price=2.0,
            size=100.0,
            side="L",
            reason="VAR",
            ts_event=1635313844284000000,
            ts_init=1635313844284000001,
        )

        # Act
        self.catalog.write_data([voided1, voided2])
        values = self.catalog.custom_data(BetfairOrderVoided)

        # Assert
        assert len(values) == 2
        assert values[0].data.client_order_id == "test-order-1"
        assert values[0].data.size_voided == 50.0
        assert values[1].data.client_order_id == "test-order-2"
        assert values[1].data.size_voided == 25.0
        assert values[1].data.reason == "VAR"
