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

import asyncio

import pytest

from nautilus_trader.adapters.binance.http.api.wallet import BinanceWalletHttpAPI
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


class TestBinanceUserHttpAPI:
    def setup(self):
        # Fixture Setup
        clock = LiveClock()
        logger = Logger(clock=clock)
        self.client = BinanceHttpClient(  # noqa: S106 (no hardcoded password)
            loop=asyncio.get_event_loop(),
            clock=clock,
            logger=logger,
            key="SOME_BINANCE_API_KEY",
            secret="SOME_BINANCE_API_SECRET",
        )

        self.api = BinanceWalletHttpAPI(self.client)

    @pytest.mark.asyncio
    async def test_trade_fee(self, mocker):
        # Arrange
        await self.client.connect()
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.api.trade_fee()

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "GET"
        assert request["url"] == "https://api.binance.com/sapi/v1/asset/tradeFee"
