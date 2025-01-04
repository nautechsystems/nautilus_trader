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

import pickle

import pytest

from nautilus_trader.core.nautilus_pyo3 import AggressorSide
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.nautilus_pyo3 import TradeId
from nautilus_trader.core.nautilus_pyo3 import TradeTick
from nautilus_trader.core.nautilus_pyo3 import Venue


AUDUSD_SIM_ID = InstrumentId.from_str("AUD/USD.SIM")


class TestQuoteTick:
    def test_pickling_instrument_id_round_trip(self):
        pickled = pickle.dumps(AUDUSD_SIM_ID)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle safe here)

        assert unpickled == AUDUSD_SIM_ID

    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert (
            QuoteTick.fully_qualified_name() == "nautilus_trader.core.nautilus_pyo3.model:QuoteTick"
        )

    def test_tick_hash_str_and_repr(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

        tick = QuoteTick(
            instrument_id=instrument_id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00001"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=3,
            ts_init=4,
        )

        # Act, Assert
        assert isinstance(hash(tick), int)
        assert str(tick) == "AUD/USD.SIM,1.00000,1.00001,1,1,3"
        assert repr(tick) == "QuoteTick(AUD/USD.SIM,1.00000,1.00001,1,1,3)"

    def test_extract_price_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            instrument_id=AUDUSD_SIM_ID,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00001"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        result1 = tick.extract_price(PriceType.ASK)
        result2 = tick.extract_price(PriceType.MID)
        result3 = tick.extract_price(PriceType.BID)

        # Assert
        assert result1 == Price.from_str("1.00001")
        assert result2 == Price.from_str("1.000005")
        assert result3 == Price.from_str("1.00000")

    def test_extract_size_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            instrument_id=AUDUSD_SIM_ID,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00001"),
            bid_size=Quantity.from_int(500_000),
            ask_size=Quantity.from_int(800_000),
            ts_event=0,
            ts_init=0,
        )

        # Act
        result1 = tick.extract_size(PriceType.ASK)
        result2 = tick.extract_size(PriceType.MID)
        result3 = tick.extract_size(PriceType.BID)

        # Assert
        assert result1 == Quantity.from_int(800_000)
        assert result2 == Quantity.from_int(650_000)  # Average size
        assert result3 == Quantity.from_int(500_000)

    def test_as_dict_returns_expected_dict(self):
        # Arrange
        tick = QuoteTick(
            instrument_id=AUDUSD_SIM_ID,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00001"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = QuoteTick.as_dict(tick)

        # Assert
        assert result == {
            "type": "QuoteTick",
            "instrument_id": "AUD/USD.SIM",
            "bid_price": "1.00000",
            "ask_price": "1.00001",
            "bid_size": "1",
            "ask_size": "1",
            "ts_event": 1,
            "ts_init": 2,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        tick = QuoteTick(
            instrument_id=AUDUSD_SIM_ID,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00001"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = QuoteTick.from_dict(QuoteTick.as_dict(tick))

        # Assert
        assert result == tick

    @pytest.mark.skip(reason="Potentially don't expose through Python API")
    def test_from_raw_returns_expected_tick(self):
        # Arrange, Act
        tick = QuoteTick.from_raw(
            AUDUSD_SIM_ID,
            1000000000,
            1000010000,
            5,
            5,
            1000000000,
            2000000000,
            0,
            0,
            1,
            2,
        )

        # Assert
        assert tick.instrument_id == AUDUSD_SIM_ID
        assert tick.bid_price == Price.from_str("1.00000")
        assert tick.ask_price == Price.from_str("1.00001")
        assert tick.bid_size == Quantity.from_int(1)
        assert tick.ask_size == Quantity.from_int(2)
        assert tick.ts_event == 1
        assert tick.ts_init == 2

    def test_pickling_round_trip_results_in_expected_tick(self):
        # Arrange
        tick = QuoteTick(
            instrument_id=AUDUSD_SIM_ID,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00001"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=1,
            ts_init=2,
        )

        # Act
        pickled = pickle.dumps(tick)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert tick == unpickled


class TestTradeTick:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert (
            TradeTick.fully_qualified_name() == "nautilus_trader.core.nautilus_pyo3.model:TradeTick"
        )

    def test_hash_str_and_repr(self):
        # Arrange
        tick = TradeTick(
            instrument_id=AUDUSD_SIM_ID,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(50_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456789"),
            ts_event=1,
            ts_init=2,
        )

        # Act, Assert
        assert isinstance(hash(tick), int)
        assert str(tick) == "AUD/USD.SIM,1.00000,50000,BUYER,123456789,1"
        assert repr(tick) == "TradeTick(AUD/USD.SIM,1.00000,50000,BUYER,123456789,1)"

    def test_as_dict_returns_expected_dict(self):
        # Arrange
        tick = TradeTick(
            instrument_id=AUDUSD_SIM_ID,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(10_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456789"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = TradeTick.as_dict(tick)

        # Assert
        assert result == {
            "type": "TradeTick",
            "instrument_id": "AUD/USD.SIM",
            "price": "1.00000",
            "size": "10000",
            "aggressor_side": "BUYER",
            "trade_id": "123456789",
            "ts_event": 1,
            "ts_init": 2,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        tick = TradeTick(
            instrument_id=AUDUSD_SIM_ID,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(10_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456789"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = TradeTick.from_dict(TradeTick.as_dict(tick))

        # Assert
        assert result == tick

    def test_pickling_round_trip_results_in_expected_tick(self):
        # Arrange
        tick = TradeTick(
            instrument_id=AUDUSD_SIM_ID,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(50_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456789"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        pickled = pickle.dumps(tick)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert unpickled == tick
        assert repr(unpickled) == "TradeTick(AUD/USD.SIM,1.00000,50000,BUYER,123456789,1)"

    @pytest.mark.skip(reason="Potentially don't expose through Python API")
    def test_from_raw_returns_expected_tick(self):
        # Arrange, Act
        trade_id = TradeId("123458")

        tick = TradeTick.from_raw(
            AUDUSD_SIM_ID,
            1000010000,
            5,
            10000000000000,
            0,
            AggressorSide.BUYER,
            trade_id,
            1,
            2,
        )

        # Assert
        assert tick.instrument_id == AUDUSD_SIM_ID
        assert tick.trade_id == trade_id
        assert tick.price == Price.from_str("1.00001")
        assert tick.size == Quantity.from_int(10_000)
        assert tick.aggressor_side == AggressorSide.BUYER
        assert tick.ts_event == 1
        assert tick.ts_init == 2
