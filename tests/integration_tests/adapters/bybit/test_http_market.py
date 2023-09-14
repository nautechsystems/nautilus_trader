import pkgutil
from typing import Optional

import msgspec
import pytest
from nautilus_trader.core.nautilus_pyo3.network import HttpClient, HttpResponse

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.endpoints.market.server_time import BybitServerTimeEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.nautilus_pyo3.network import HttpResponse

from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrumentsLinearResponse, \
    BybitInstrumentsSpotResponse, BybitInstrumentsOptionResponse
from nautilus_trader.adapters.bybit.schemas.market.server_time import BybitServerTimeResponse


def get_mock(response):
    async def mock(*args,**kwargs):
        return HttpResponse(status=200,body=response)
    return mock

class TestBybitMarketHttpAPI:
    def setup(self):
        clock = LiveClock()
        logger = Logger(clock=clock)
        self.client = BybitHttpClient(
            clock=clock,
            logger=logger,
            api_key="SOME_BYBIT_API_KEY",
            api_secret="SOME_BYBIT_API_SECRET",
            base_url="https://api-testnet.bybit.com",
        )
        self.linear_api = BybitMarketHttpAPI(
            client=self.client,
            clock=clock,
            instrument_type=BybitInstrumentType.LINEAR,
        )
        self.spot_api = BybitMarketHttpAPI(
            client=self.client,
            clock=clock,
            instrument_type=BybitInstrumentType.SPOT,
        )
        self.option_api = BybitMarketHttpAPI(
            client=self.client,
            clock=clock,
            instrument_type=BybitInstrumentType.OPTION,
        )

    @pytest.mark.asyncio()
    async def test_server_time(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses",
            "server_time.json",
        )
        response_decoded = msgspec.json.Decoder(BybitServerTimeResponse).decode(response)

        monkeypatch.setattr(HttpClient,"request",get_mock(response))
        server_time = await self.spot_api.fetch_server_time()
        assert server_time.timeSecond == response_decoded.result.timeSecond
        assert server_time.timeNano == response_decoded.result.timeNano


    @pytest.mark.asyncio()
    async def test_spot_instruments(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.spot",
            "instruments.json",
        )
        response_decoded = msgspec.json.Decoder(BybitInstrumentsSpotResponse).decode(response)

        monkeypatch.setattr(HttpClient,"request",get_mock(response))
        instruments = await self.spot_api.fetch_instruments()
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

        monkeypatch.setattr(HttpClient,"request",get_mock(response))
        instruments = await self.linear_api.fetch_instruments()
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

        monkeypatch.setattr(HttpClient,"request",get_mock(response))
        instruments = await self.option_api.fetch_instruments()
        assert len(instruments) == 2
        assert response_decoded.result.list[0] == instruments[0]
        assert response_decoded.result.list[1] == instruments[1]

