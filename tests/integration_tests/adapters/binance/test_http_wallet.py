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

import asyncio
import pkgutil

import pytest
from aiohttp import ClientResponse

from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.http.wallet import BinanceSpotWalletHttpAPI
from nautilus_trader.adapters.binance.spot.schemas.wallet import BinanceSpotTradeFee
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


class TestBinanceUserHttpAPI:
    def setup(self):
        # Fixture Setup
        clock = LiveClock()
        logger = Logger(clock=clock)
        self.client = BinanceHttpClient(
            loop=asyncio.get_event_loop(),
            clock=clock,
            logger=logger,
            key="SOME_BINANCE_API_KEY",
            secret="SOME_BINANCE_API_SECRET",
        )

        self.api = BinanceSpotWalletHttpAPI(self.client, clock)

    @pytest.mark.asyncio()
    async def test_trade_fee(self, mocker):
        # Arrange
        async def async_mock():
            return pkgutil.get_data(
                package="tests.integration_tests.adapters.binance.resources.http_responses",
                resource="http_wallet_trading_fee.json",
            )

        mock_request = mocker.patch.object(
            target=self.client,
            attribute="send_request",
            spec=ClientResponse,
            return_value=async_mock(),
        )

        # Act
        response = await self.api.query_spot_trade_fees(symbol="BTCUSDT")

        # Assert
        name, args, kwargs = mock_request.call_args[0]
        assert name == "GET"
        assert args == "/sapi/v1/asset/tradeFee"
        assert kwargs["symbol"] == "BTCUSDT"
        assert "signature" in kwargs
        assert "timestamp" in kwargs
        assert len(response) == 1
        assert isinstance(response[0], BinanceSpotTradeFee)

    @pytest.mark.asyncio()
    async def test_trade_fees(self, mocker):
        # Arrange
        async def async_mock():
            return pkgutil.get_data(
                package="tests.integration_tests.adapters.binance.resources.http_responses",
                resource="http_wallet_trading_fees.json",
            )

        mock_request = mocker.patch.object(
            target=self.client,
            attribute="send_request",
            spec=ClientResponse,
            return_value=async_mock(),
        )

        # Act
        response = await self.api.query_spot_trade_fees()

        # Assert
        name, args, kwargs = mock_request.call_args[0]
        assert name == "GET"
        assert args == "/sapi/v1/asset/tradeFee"
        assert "signature" in kwargs
        assert "timestamp" in kwargs
        assert len(response) == 2
        assert isinstance(response[0], BinanceSpotTradeFee)
        assert isinstance(response[1], BinanceSpotTradeFee)
