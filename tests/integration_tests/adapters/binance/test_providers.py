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

import pkgutil
from typing import Dict

import pytest

from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


class TestBinanceInstrumentProvider:
    @pytest.mark.asyncio
    async def test_load_all_async(
        self,
        binance_http_client,
        live_logger,
        monkeypatch,
    ):
        # Arrange: prepare data for monkey patch
        response1 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.responses",
            resource="wallet_trading_fee.json",
        )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.responses",
            resource="spot_market_exchange_info.json",
        )

        responses = [response2, response1]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # noqa (needed for mock)
            http_method: str,  # noqa (needed for mock)
            url_path: str,  # noqa (needed for mock)
            payload: Dict[str, str],  # noqa (needed for mock)
        ) -> bytes:
            return responses.pop()

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.provider = BinanceInstrumentProvider(
            client=binance_http_client,
            logger=live_logger,
        )

        # Act
        await self.provider.load_all_async()

        # Assert
        assert self.provider.count == 2
        assert self.provider.find(InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE"))) is not None
        assert self.provider.find(InstrumentId(Symbol("ETHUSDT"), Venue("BINANCE"))) is not None
        assert len(self.provider.currencies()) == 3
        assert "BTC" in self.provider.currencies()
        assert "ETH" in self.provider.currencies()
        assert "USDT" in self.provider.currencies()
