# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestQuoteTick:
    def test_pickling_instrument_id_round_trip(self):
        pickled = pickle.dumps(AUDUSD_SIM.id)
        unpickled = pickle.loads(pickled)  # noqa

        assert unpickled == AUDUSD_SIM.id

    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert QuoteTick.fully_qualified_name() == "nautilus_trader.model.data:QuoteTick"

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
            instrument_id=AUDUSD_SIM.id,
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

    def test_extract_volume_with_various_price_types_returns_expected_values(self):
        # Arrange
        tick = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00001"),
            bid_size=Quantity.from_int(500_000),
            ask_size=Quantity.from_int(800_000),
            ts_event=0,
            ts_init=0,
        )

        # Act
        result1 = tick.extract_volume(PriceType.ASK)
        result2 = tick.extract_volume(PriceType.MID)
        result3 = tick.extract_volume(PriceType.BID)

        # Assert
        assert result1 == Quantity.from_int(800_000)
        assert result2 == Quantity.from_int(650_000)  # Average size
        assert result3 == Quantity.from_int(500_000)

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        tick = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00001"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = QuoteTick.to_dict(tick)

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
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00001"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = QuoteTick.from_dict(QuoteTick.to_dict(tick))

        # Assert
        assert result == tick

    def test_from_raw_returns_expected_tick(self):
        # Arrange, Act
        tick = QuoteTick.from_raw(
            AUDUSD_SIM.id,
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
        assert tick.instrument_id == AUDUSD_SIM.id
        assert tick.bid_price == Price.from_str("1.00000")
        assert tick.ask_price == Price.from_str("1.00001")
        assert tick.bid_size == Quantity.from_int(1)
        assert tick.ask_size == Quantity.from_int(2)
        assert tick.ts_event == 1
        assert tick.ts_init == 2

    def test_pickling_round_trip_results_in_expected_tick(self):
        # Arrange
        tick = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00000"),
            ask_price=Price.from_str("1.00001"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=1,
            ts_init=2,
        )

        # Act
        pickled = pickle.dumps(tick)
        unpickled = pickle.loads(pickled)  # noqa S301 (pickle is safe here)

        # Assert
        assert tick == unpickled

    def test_to_pyo3_list(self):
        # Arrange
        wrangler = QuoteTickDataWrangler(instrument=AUDUSD_SIM)

        quotes = wrangler.process(
            data=TestDataProvider().read_csv_ticks("truefx/audusd-ticks.csv"),
            default_volume=1_000_000,
        )

        # Act
        pyo3_quotes = QuoteTick.to_pyo3_list(quotes)

        # Assert
        assert len(pyo3_quotes)
        assert isinstance(pyo3_quotes[0], nautilus_pyo3.QuoteTick)


class TestTradeTick:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert TradeTick.fully_qualified_name() == "nautilus_trader.model.data:TradeTick"

    def test_hash_str_and_repr(self):
        # Arrange
        tick = TradeTick(
            instrument_id=AUDUSD_SIM.id,
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

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        tick = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(10_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456789"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = TradeTick.to_dict(tick)

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
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(10_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456789"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = TradeTick.from_dict(TradeTick.to_dict(tick))

        # Assert
        assert result == tick

    def test_pickling_round_trip_results_in_expected_tick(self):
        # Arrange
        tick = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(50_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456789"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        pickled = pickle.dumps(tick)
        unpickled = pickle.loads(pickled)  # noqa S301 (pickle is safe here)

        # Assert
        assert unpickled == tick
        assert repr(unpickled) == "TradeTick(AUD/USD.SIM,1.00000,50000,BUYER,123456789,1)"

    def test_from_raw_returns_expected_tick(self):
        # Arrange, Act
        trade_id = TradeId("123458")

        tick = TradeTick.from_raw(
            AUDUSD_SIM.id,
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
        assert tick.instrument_id == AUDUSD_SIM.id
        assert tick.trade_id == trade_id
        assert tick.price == Price.from_str("1.00001")
        assert tick.size == Quantity.from_int(10_000)
        assert tick.aggressor_side == AggressorSide.BUYER
        assert tick.ts_event == 1
        assert tick.ts_init == 2
