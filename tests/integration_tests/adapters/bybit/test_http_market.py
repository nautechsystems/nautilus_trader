import pkgutil
from typing import Optional

import msgspec
import pytest
from nautilus_trader.core.nautilus_pyo3.network import HttpClient, HttpResponse

from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.adapters.bybit.endpoints.market.server_time import BybitServerTimeEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.nautilus_pyo3.network import HttpResponse


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
        self.api = BybitMarketHttpAPI(
            client=self.client,
            clock=clock,
            account_type=BybitAccountType.LINEAR,
        )

    @pytest.mark.asyncio()
    async def test_server_time(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses",
            "server_time.json",
        )

        async def mock(*args,**kwargs):
            return HttpResponse(status=200,body=response)

        monkeypatch.setattr(HttpClient,"request",mock)
        server_time = await self.api.fetch_server_time()
        assert server_time.timeSecond == '1694549475'
        assert server_time.timeNano == '1694549475089935642'
