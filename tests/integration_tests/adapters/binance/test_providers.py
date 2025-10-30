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

import json
import pkgutil

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAccountInfo
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


class TestBinanceInstrumentProvider:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()

    @pytest.mark.skip(reason="WIP - test data format mismatch")
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
            ratelimiter_keys: list[str] | None = None,  # (needed for mock)
        ) -> bytes:
            return responses.pop()

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.provider = BinanceSpotInstrumentProvider(
            client=binance_http_client,
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
        exchange_info_response = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_futures_market_exchange_info.json",
        )

        account_info = BinanceFuturesAccountInfo(
            feeTier=0,
            canTrade=True,
            canDeposit=True,
            canWithdraw=True,
            updateTime=1234567890000,
            assets=[],
        )

        responses = [exchange_info_response]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # (needed for mock)
            http_method: str,  # (needed for mock)
            url_path: str,  # (needed for mock)
            payload: dict[str, str],  # (needed for mock)
            ratelimiter_keys: list[str] | None = None,  # (needed for mock)
        ) -> bytes:
            return responses.pop()

        async def mock_query_account_info(recv_window: str):
            return account_info

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.provider = BinanceFuturesInstrumentProvider(
            client=binance_http_client,
            clock=self.clock,
            account_type=BinanceAccountType.USDT_FUTURES,
        )

        monkeypatch.setattr(
            self.provider._http_account,
            "query_futures_account_info",
            mock_query_account_info,
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

    @pytest.mark.asyncio()
    async def test_futures_instrument_info_dict_is_json_serializable(
        self,
        binance_http_client,
        live_logger,
        monkeypatch,
    ):
        """
        Test that the instrument info dict contains only JSON-serializable primitives.

        This regression test ensures that enums (like BinanceFuturesContractStatus,
        BinanceOrderType, BinanceTimeInForce) are converted to their string values
        in the info dict, preventing JSON serialization errors in PyO3 interop.

        See: https://github.com/nautechsystems/nautilus_trader/issues/3128

        """
        # Arrange
        exchange_info_response = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_futures_market_exchange_info.json",
        )

        account_info = BinanceFuturesAccountInfo(
            feeTier=0,
            canTrade=True,
            canDeposit=True,
            canWithdraw=True,
            updateTime=1234567890000,
            assets=[],
        )

        responses = [exchange_info_response]

        async def mock_send_request(
            self,
            http_method: str,
            url_path: str,
            payload: dict[str, str],
            ratelimiter_keys: list[str] | None = None,
        ) -> bytes:
            return responses.pop()

        async def mock_query_account_info(recv_window: str):
            return account_info

        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.provider = BinanceFuturesInstrumentProvider(
            client=binance_http_client,
            clock=self.clock,
            account_type=BinanceAccountType.USDT_FUTURES,
        )

        monkeypatch.setattr(
            self.provider._http_account,
            "query_futures_account_info",
            mock_query_account_info,
        )

        # Act
        await self.provider.load_all_async()

        # Assert - verify instruments were loaded
        btc_perp = self.provider.find(InstrumentId(Symbol("BTCUSDT-PERP"), Venue("BINANCE")))
        assert btc_perp is not None

        # Assert - verify info dict is JSON-serializable (no enum objects)
        info_dict = btc_perp.info
        assert info_dict is not None

        # This should not raise TypeError about enum not being JSON serializable
        json_str = json.dumps(info_dict)
        assert json_str is not None

        # Verify enum fields were converted to strings
        assert info_dict["status"] == "TRADING"
        assert isinstance(info_dict["status"], str)
        assert all(isinstance(ot, str) for ot in info_dict["orderTypes"])
        assert all(isinstance(tif, str) for tif in info_dict["timeInForce"])

    @pytest.mark.skip(reason="WIP - test data missing allowTrailingStop field")
    @pytest.mark.asyncio()
    async def test_spot_instrument_info_dict_is_json_serializable(
        self,
        binance_http_client,
        live_logger,
        monkeypatch,
    ):
        """
        Test that the Spot instrument info dict contains only JSON-serializable
        primitives.

        This regression test ensures that enums (like BinanceOrderType) are converted
        to their string values in the info dict, preventing JSON serialization errors.

        See: https://github.com/nautechsystems/nautilus_trader/issues/3128

        """
        # Arrange
        exchange_info_response = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_spot_market_exchange_info.json",
        )

        trade_fees_response = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.http_responses",
            resource="http_wallet_trading_fees.json",
        )

        responses = [exchange_info_response, trade_fees_response]

        async def mock_send_request(
            self,
            http_method: str,
            url_path: str,
            payload: dict[str, str],
            ratelimiter_keys: list[str] | None = None,
        ) -> bytes:
            return responses.pop()

        monkeypatch.setattr(
            target=BinanceHttpClient,
            name="send_request",
            value=mock_send_request,
        )

        self.provider = BinanceSpotInstrumentProvider(
            client=binance_http_client,
            clock=self.clock,
            account_type=BinanceAccountType.SPOT,
        )

        # Act
        await self.provider.load_all_async()

        # Assert - verify instruments were loaded
        btc_usdt = self.provider.find(InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")))
        assert btc_usdt is not None

        # Assert - verify info dict is JSON-serializable (no enum objects)
        info_dict = btc_usdt.info
        assert info_dict is not None

        # This should not raise TypeError about enum not being JSON serializable
        json_str = json.dumps(info_dict)
        assert json_str is not None

        # Verify enum fields were converted to strings
        assert info_dict["status"] == "TRADING"
        assert isinstance(info_dict["status"], str)
        assert all(isinstance(ot, str) for ot in info_dict["orderTypes"])
