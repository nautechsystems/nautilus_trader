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

from nautilus_trader.adapters.bybit.common.enums import BybitKlineInterval
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentsLinearResponse
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentsOptionResponse
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentsSpotResponse
from nautilus_trader.adapters.bybit.schemas.market.kline import BybitKlinesResponse
from nautilus_trader.adapters.bybit.schemas.market.server_time import BybitServerTimeResponse
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersLinearResponse
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersOptionResponse
from nautilus_trader.adapters.bybit.schemas.market.ticker import BybitTickersSpotResponse
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from tests.integration_tests.adapters.bybit.utils.get_mock import get_mock


class TestBybitMarketHttpAPI:
    def setup(self):
        clock = LiveClock()
        self.client = BybitHttpClient(
            clock=clock,
            api_key="SOME_BYBIT_API_KEY",
            api_secret="SOME_BYBIT_API_SECRET",
            base_url="https://api-testnet.bybit.com",
        )
        self.http_api = BybitMarketHttpAPI(
            client=self.client,
            clock=clock,
        )

    ################################################################################
    # Server time
    ################################################################################

    @pytest.mark.asyncio()
    async def test_server_time(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses",
            "server_time.json",
        )
        response_decoded = msgspec.json.Decoder(BybitServerTimeResponse).decode(response)

        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        server_time = await self.http_api.fetch_server_time()
        assert server_time.timeSecond == response_decoded.result.timeSecond
        assert server_time.timeNano == response_decoded.result.timeNano

    ################################################################################
    # Fetch instruments
    ################################################################################

    @pytest.mark.asyncio()
    async def test_spot_instruments(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.spot",
            "instruments.json",
        )
        response_decoded = msgspec.json.Decoder(BybitInstrumentsSpotResponse).decode(response)

        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        instruments = await self.http_api.fetch_instruments(BybitProductType.SPOT)
        assert len(instruments) == 2
        assert response_decoded.result.list[0] == instruments[0]
        assert response_decoded.result.list[1] == instruments[1]

    @pytest.mark.asyncio()
    async def test_linear_instruments(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.linear",
            "instruments.json",
        )
        response_decoded = msgspec.json.Decoder(BybitInstrumentsLinearResponse).decode(response)

        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        instruments = await self.http_api.fetch_instruments(BybitProductType.LINEAR)
        assert len(instruments) == 2
        assert response_decoded.result.list[0] == instruments[0]
        assert response_decoded.result.list[1] == instruments[1]

    @pytest.mark.asyncio()
    async def test_option_instruments(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.option",
            "instruments.json",
        )
        response_decoded = msgspec.json.Decoder(BybitInstrumentsOptionResponse).decode(response)

        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        instruments = await self.http_api.fetch_instruments(BybitProductType.OPTION)
        assert len(instruments) == 2
        assert response_decoded.result.list[0] == instruments[0]
        assert response_decoded.result.list[1] == instruments[1]

    ################################################################################
    # Fetch klines
    ################################################################################

    @pytest.mark.asyncio()
    async def test_klines_spot(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.spot",
            "klines_btc.json",
        )
        response_decoded = msgspec.json.Decoder(BybitKlinesResponse).decode(response)
        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        klines = await self.http_api.fetch_klines(
            BybitProductType.SPOT,
            "BTCUSDT",
            BybitKlineInterval.DAY_1,
            3,
        )
        assert len(klines) == 3
        assert response_decoded.result.list[0] == klines[0]
        assert response_decoded.result.list[1] == klines[1]
        assert response_decoded.result.list[2] == klines[2]

    @pytest.mark.asyncio()
    async def test_klines_linear(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.linear",
            "klines_btc.json",
        )
        response_decoded = msgspec.json.Decoder(BybitKlinesResponse).decode(response)
        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        klines = await self.http_api.fetch_klines(
            BybitProductType.LINEAR,
            "BTCUSDT",
            BybitKlineInterval.DAY_1,
            3,
        )
        assert len(klines) == 3
        assert response_decoded.result.list[0] == klines[0]
        assert response_decoded.result.list[1] == klines[1]
        assert response_decoded.result.list[2] == klines[2]

    ################################################################################
    # Fetch tickers
    ################################################################################

    @pytest.mark.asyncio()
    async def test_fetch_tickers_linear(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.linear",
            "tickers.json",
        )
        response_decoded = msgspec.json.Decoder(BybitTickersLinearResponse).decode(response)
        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        tickers = await self.http_api.fetch_tickers(BybitProductType.LINEAR)
        assert response_decoded.result.list == tickers
        assert len(tickers) == 1
        assert tickers[0].symbol == "BTCUSDT"
        assert tickers[0].lastPrice == "16597.00"

    @pytest.mark.asyncio()
    async def test_fetch_tickers_option(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.option",
            "tickers.json",
        )
        response_decoded = msgspec.json.Decoder(BybitTickersOptionResponse).decode(response)
        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        tickers = await self.http_api.fetch_tickers(BybitProductType.OPTION)
        assert response_decoded.result.list == tickers
        assert len(tickers) == 1
        assert tickers[0].symbol == "BTC-30DEC22-18000-C"
        assert tickers[0].lastPrice == "435"

    @pytest.mark.asyncio()
    async def test_fetch_tickers_spot(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.spot",
            "tickers.json",
        )
        response_decoded = msgspec.json.Decoder(BybitTickersSpotResponse).decode(response)
        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        tickers = await self.http_api.fetch_tickers(BybitProductType.SPOT)
        assert response_decoded.result.list == tickers
        assert len(tickers) == 1
        assert tickers[0].symbol == "BTCUSDT"
        assert tickers[0].lastPrice == "20533.13"
