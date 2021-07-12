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

from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestQuoteTick:
    def test_tick_hash_str_and_repr(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act, Assert
        assert isinstance(hash(tick), int)
        assert str(tick) == "AUD/USD.SIM,1.00000,1.00001,1,1,0"
        assert repr(tick) == "QuoteTick(AUD/USD.SIM,1.00000,1.00001,1,1,0)"

    def test_extract_price_with_invalid_price_raises_value_error(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act, Assert
        with pytest.raises(ValueError):
            tick.extract_price(0)

    def test_extract_price_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        result1 = tick.extract_price(PriceType.ASK)
        result2 = tick.extract_price(PriceType.MID)
        result3 = tick.extract_price(PriceType.BID)

        # Assert
        assert result1 == Price.from_str("1.00001")
        assert result2 == Price.from_str("1.000005")
        assert result3 == Price.from_str("1.00000")

    def test_extract_volume_with_invalid_price_raises_value_error(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act, Assert
        with pytest.raises(ValueError):
            tick.extract_volume(0)

    def test_extract_volume_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(500000),
            Quantity.from_int(800000),
            0,
            0,
        )

        # Act
        result1 = tick.extract_volume(PriceType.ASK)
        result2 = tick.extract_volume(PriceType.MID)
        result3 = tick.extract_volume(PriceType.BID)

        # Assert
        assert result1 == Quantity.from_int(800000)
        assert result2 == Quantity.from_int(650000)  # Average size
        assert result3 == Quantity.from_int(500000)

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        result = QuoteTick.to_dict(tick)
        print(result)
        # Assert
        assert result == {
            "type": "QuoteTick",
            "instrument_id": "AUD/USD.SIM",
            "bid": "1.00000",
            "ask": "1.00001",
            "bid_size": "1",
            "ask_size": "1",
            "ts_event_ns": 0,
            "ts_recv_ns": 0,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        # Act
        result = QuoteTick.from_dict(QuoteTick.to_dict(tick))

        # Assert
        assert tick == result


class TestTradeTick:
    def test_hash_str_and_repr(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(50000),
            AggressorSide.BUY,
            "123456789",
            0,
            0,
        )

        # Act, Assert
        assert isinstance(hash(tick), int)
        assert str(tick) == "AUD/USD.SIM,1.00000,50000,BUY,123456789,0"
        assert repr(tick) == "TradeTick(AUD/USD.SIM,1.00000,50000,BUY,123456789,0)"

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(10000),
            AggressorSide.BUY,
            "123456789",
            0,
            0,
        )

        # Act
        result = TradeTick.to_dict(tick)

        # Assert
        assert result == {
            "type": "TradeTick",
            "instrument_id": "AUD/USD.SIM",
            "price": "1.00000",
            "size": "10000",
            "aggressor_side": "BUY",
            "match_id": "123456789",
            "ts_event_ns": 0,
            "ts_recv_ns": 0,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(10000),
            AggressorSide.BUY,
            "123456789",
            0,
            0,
        )

        # Act
        result = TradeTick.from_dict(TradeTick.to_dict(tick))

        # Assert
        assert tick == result
