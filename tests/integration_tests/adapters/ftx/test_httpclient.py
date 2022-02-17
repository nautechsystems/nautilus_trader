
import asyncio
import pkgutil
from typing import Dict

import orjson
import pytest

from nautilus_trader.adapters.ftx.common import FTX_VENUE
from nautilus_trader.adapters.ftx.http.client import FTXHttpClient
from nautilus_trader.adapters.ftx.providers import FTXInstrumentProvider

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.msgbus.bus import MessageBus
from tests.test_kit.stubs import TestStubs

class TestFTXHTTPClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(clock=self.clock)

        self.trader_id = TestStubs.trader_id()
        self.venue = FTX_VENUE
        self.account_id = AccountId(self.venue.value, "001")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()

        self.http_client = FTXHttpClient(  # noqa: S106 (no hardcoded password)
            loop=asyncio.get_event_loop(),
            clock=self.clock,
            logger=self.logger,
            key="SOME_FTX_API_KEY",
            secret="SOME_FTX_API_SECRET",
        )

    @pytest.mark.asyncio
    async def test_ftx_http_client(self):
        await self.http_client.connect() 
        
        account_info = await self.http_client.get_account_info()
        assert (len(account_info) > 0)
        markets_list = await self.http_client.list_markets()
        assert (len(markets_list) > 0)
        provider = FTXInstrumentProvider(
         client=self.http_client,
         logger=self.logger,
        )
        await provider.load_all_async()
        assert(len(provider.get_all().values()) > 0)
        await self.http_client.disconnect()
        
        
