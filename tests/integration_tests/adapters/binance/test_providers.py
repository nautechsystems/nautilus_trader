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
import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


@pytest.mark.skip(reason="WIP")
class TestBinanceInstrumentProvider:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()

    @pytest.mark.asyncio()
    async def test_load_all_async_for_spot_markets(
        self,
        binance_http_client,
        live_logger,
        monkeypatch,
    ):
        # Arrange: prepare data for monkey patch
        response1 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_wallet_trading_fee.json",
        )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_market_exchange_info.json",
        )

        responses = [response2, response1]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # (needed for mock)
            http_method: str,  # (needed for mock)
            url_path: str,  # (needed for mock)
            payload: dict[str, str],  # (needed for mock)
        ) -> bytes:
            return msgspec.json.decode(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.provider = BinanceSpotInstrumentProvider(
            client=binance_http_client,
            logger=live_logger,
            clock=self.clock,
            account_type=BinanceAccountType.SPOT,
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

    @pytest.mark.asyncio()
    async def test_load_all_async_for_futures_markets(
        self,
        binance_http_client,
        live_logger,
        monkeypatch,
    ):
        # Arrange: prepare data for monkey patch
        # response1 = pkgutil.get_data(
        #     package="tests.integration_tests.adapters.binance.resources.http_responses",
        #     resource="http_wallet_trading_fee.json",
        # )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_futures_market_exchange_info.json",
        )

        responses = [response2]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # (needed for mock)
            http_method: str,  # (needed for mock)
            url_path: str,  # (needed for mock)
            payload: dict[str, str],  # (needed for mock)
        ) -> bytes:
            return msgspec.json.decode(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.provider = BinanceFuturesInstrumentProvider(
            client=binance_http_client,
            logger=live_logger,
            clock=self.clock,
            account_type=BinanceAccountType.USDT_FUTURE,
        )

        # Act
        await self.provider.load_all_async()

        # Assert
        assert self.provider.count == 3
        assert (
            self.provider.find(InstrumentId(Symbol("BTCUSDT-PERP"), Venue("BINANCE"))) is not None
        )
        assert (
            self.provider.find(InstrumentId(Symbol("ETHUSDT-PERP"), Venue("BINANCE"))) is not None
        )
        assert (
            self.provider.find(InstrumentId(Symbol("BTCUSDT_220325"), Venue("BINANCE"))) is not None
        )
        assert len(self.provider.currencies()) == 3
        assert "BTC" in self.provider.currencies()
        assert "ETH" in self.provider.currencies()
        assert "USDT" in self.provider.currencies()
