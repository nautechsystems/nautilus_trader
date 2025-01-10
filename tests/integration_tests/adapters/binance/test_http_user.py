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


import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.http.user import BinanceSpotUserDataHttpAPI
from nautilus_trader.common.component import LiveClock


@pytest.mark.skip(reason="WIP")
class TestBinanceUserHttpAPI:
    def setup(self):
        # Fixture Setup
        clock = LiveClock()
        self.client = BinanceHttpClient(
            clock=clock,
            api_key="SOME_BINANCE_API_KEY",
            api_secret="SOME_BINANCE_API_SECRET",
            base_url="https://api.binance.com/",  # Spot/Margin
        )
        self.test_symbol = "ETHUSDT"
        self.spot_api = BinanceSpotUserDataHttpAPI(self.client, BinanceAccountType.SPOT)
        self.isolated_margin_api = BinanceSpotUserDataHttpAPI(
            self.client,
            BinanceAccountType.ISOLATED_MARGIN,
        )

    @pytest.mark.asyncio()
    async def test_create_listen_key_spot(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.spot_api.create_listen_key()

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "POST"
        assert request["url"] == "https://api.binance.com/api/v3/userDataStream"

    @pytest.mark.asyncio()
    async def test_keepalive_listen_key_spot(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.spot_api.keepalive_listen_key(
            listen_key="JUdsZc8CSmMUxg1wJha23RogrT3EuC8eV5UTbAOVTkF3XWofMzWoXtWmDAhy",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "PUT"
        assert request["url"] == "https://api.binance.com/api/v3/userDataStream"
        assert (
            request["params"]
            == "listenKey=JUdsZc8CSmMUxg1wJha23RogrT3EuC8eV5UTbAOVTkF3XWofMzWoXtWmDAhy"
        )

    @pytest.mark.asyncio()
    async def test_delete_listen_key_spot(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.spot_api.delete_listen_key(
            listen_key="JUdsZc8CSmMUxg1wJha23RogrT3EuC8eV5UTbAOVTkF3XWofMzWoXtWmDAhy",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "DELETE"
        assert request["url"] == "https://api.binance.com/api/v3/userDataStream"
        assert (
            request["params"]
            == "listenKey=JUdsZc8CSmMUxg1wJha23RogrT3EuC8eV5UTbAOVTkF3XWofMzWoXtWmDAhy"
        )

    @pytest.mark.asyncio()
    async def test_create_listen_key_isolated_margin(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.isolated_margin_api.create_listen_key(symbol=self.test_symbol)

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "POST"
        assert request["url"] == "https://api.binance.com/sapi/v1/userDataStream/isolated"
        assert request["params"] == "symbol=ETHUSDT"

    @pytest.mark.asyncio()
    async def test_keepalive_listen_key_isolated_margin(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.isolated_margin_api.keepalive_listen_key(
            symbol=self.test_symbol,
            listen_key="JUdsZc8CSmMUxg1wJha23RogrT3EuC8eV5UTbAOVTkF3XWofMzWoXtWmDAhy",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "PUT"
        assert request["url"] == "https://api.binance.com/sapi/v1/userDataStream/isolated"
        assert (
            request["params"]
            == "listenKey=JUdsZc8CSmMUxg1wJha23RogrT3EuC8eV5UTbAOVTkF3XWofMzWoXtWmDAhy&symbol=ETHUSDT"
        )

    @pytest.mark.asyncio()
    async def test_delete_listen_key_isolated_margin(self, mocker):
        # Arrange
        mock_send_request = mocker.patch(target="aiohttp.client.ClientSession.request")

        # Act
        await self.isolated_margin_api.delete_listen_key(
            symbol=self.test_symbol,
            listen_key="JUdsZc8CSmMUxg1wJha23RogrT3EuC8eV5UTbAOVTkF3XWofMzWoXtWmDAhy",
        )

        # Assert
        request = mock_send_request.call_args.kwargs
        assert request["method"] == "DELETE"
        assert request["url"] == "https://api.binance.com/sapi/v1/userDataStream/isolated"
        assert (
            request["params"]
            == "listenKey=JUdsZc8CSmMUxg1wJha23RogrT3EuC8eV5UTbAOVTkF3XWofMzWoXtWmDAhy&symbol=ETHUSDT"
        )
