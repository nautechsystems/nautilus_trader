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

import pkgutil

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceExecutionType
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTickerData
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesTradeLiteMsg
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotOrderUpdateWrapper
from nautilus_trader.test_kit.providers import TestInstrumentProvider


ETHUSDT = TestInstrumentProvider.ethusdt_binance()


class TestBinanceWebSocketParsing:
    def test_parse_ticker(self):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_ticker_24hr.json",
        )
        assert raw

        # Act
        decoder = msgspec.json.Decoder(BinanceTickerData)
        data = decoder.decode(raw)
        result = data.parse_to_binance_ticker(
            instrument_id=ETHUSDT.id,
            ts_init=9999999999999991,
        )

        # Assert
        assert result.instrument_id == ETHUSDT.id

    def test_parse_trade_lite(self):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_futures_trade_lite.json",
        )
        assert raw

        # Act
        decoder = msgspec.json.Decoder(BinanceFuturesTradeLiteMsg)
        data = decoder.decode(raw)

        # Assert
        assert data.s == "ETHUSDT"

    def test_parse_spot_execution_report_binance_us(self):
        # Arrange: Load Binance US execution report with W and V fields
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_binance_us.json",
        )
        assert raw

        # Act
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Assert
        assert wrapper.data.e.value == "executionReport"
        assert wrapper.data.s == "BTCUSD"
        assert wrapper.data.x == BinanceExecutionType.TRADE
        assert wrapper.data.l == "0.00042000"
        assert wrapper.data.L == "117290.77000000"
        assert wrapper.data.W == 1759347763167  # Working time field
        assert wrapper.data.V == "EXPIRE_MAKER"  # Self-Trade Prevention Mode
