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

import platform

import pytest

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from tests.test_kit.providers import TestDataProvider


class TestBacktestLoaders:
    def test_default_fx_with_5_dp_returns_expected_instrument(self):
        # Arrange
        loader = TestInstrumentProvider()

        # Act
        instrument = loader.default_fx_ccy("AUD/USD")

        # Assert
        assert instrument.id == InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        assert instrument.price_precision == 5
        assert instrument.price_increment == Price.from_str("0.00001")
        assert instrument.base_currency.code == "AUD"
        assert instrument.quote_currency.code == "USD"

    def test_default_fx_with_3_dp_returns_expected_instrument(self):
        # Arrange
        loader = TestInstrumentProvider()

        # Act
        instrument = loader.default_fx_ccy("USD/JPY", Venue("SIM"))

        # Assert
        assert instrument.id == InstrumentId(Symbol("USD/JPY"), Venue("SIM"))
        assert instrument.price_precision == 3
        assert instrument.price_increment == Price.from_str("0.001")
        assert instrument.base_currency.code == "USD"
        assert instrument.quote_currency.code == "JPY"


@pytest.mark.skipif(
    platform.system() == "Windows",
    reason="Windows does not support `pyarrow`",
)
class TestParquetTickDataLoaders:
    def test_btcusdt_trade_ticks_from_parquet_loader_return_expected_row(self):
        # Arrange, Act
        trade_ticks = TestDataProvider.parquet_btcusdt_trades()

        # Assert
        assert len(trade_ticks), 2001
        assert "trade_id" in trade_ticks.columns
        assert "price" in trade_ticks.columns
        assert "quantity" in trade_ticks.columns
        assert "buyer_maker" in trade_ticks.columns
        assert trade_ticks.iloc[0]["trade_id"] == 553287559

    def test_btcusdt_quote_ticks_from_parquet_loader_return_expected_row(self):
        # Arrange, Act
        quote_ticks = TestDataProvider.parquet_btcusdt_quotes()

        # Assert
        assert len(quote_ticks), 451
        assert "symbol" in quote_ticks.columns
        assert "ask_size" in quote_ticks.columns
        assert "ask" in quote_ticks.columns
        assert "bid_size" in quote_ticks.columns
        assert "bid" in quote_ticks.columns
        assert quote_ticks.iloc[0]["ask"] == 39433.62
        assert quote_ticks.iloc[0]["bid"] == 39432.99
