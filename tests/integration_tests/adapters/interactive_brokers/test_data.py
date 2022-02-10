import asyncio

import pytest

from nautilus_trader.adapters.interactive_brokers.factories import (
    InteractiveBrokersLiveDataClientFactory,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.msgbus.bus import MessageBus
from tests.test_kit.mocks import MockCacheDatabase
from tests.test_kit.stubs import TestStubs


class TestInteractiveBrokersData:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.logger = LiveLogger(
            loop=self.loop,
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
        )

        self.trader_id = TestStubs.trader_id()
        self.strategy_id = TestStubs.strategy_id()
        self.account_id = TestStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache_db = MockCacheDatabase(
            logger=self.logger,
        )

        self.cache = Cache(
            database=self.cache_db,
            logger=self.logger,
        )

        # Arrange, Act
        self.data_client = InteractiveBrokersLiveDataClientFactory.create(
            loop=self.loop,
            name="IB",
            config={},
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    def _async_setup(self, loop):
        # Fixture Setup
        self.loop = loop
        self.clock = LiveClock()
        self.logger = LiveLogger(
            loop=self.loop,
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
        )

        self.trader_id = TestStubs.trader_id()
        self.strategy_id = TestStubs.strategy_id()
        self.account_id = TestStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache_db = MockCacheDatabase(
            logger=self.logger,
        )

        self.cache = Cache(
            database=self.cache_db,
            logger=self.logger,
        )

        # Arrange, Act
        self.data_client = InteractiveBrokersLiveDataClientFactory.create(
            loop=self.loop,
            name="IB",
            config={},
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

    @pytest.mark.asyncio
    async def test_factory(self, event_loop):
        # Arrange
        self._async_setup(loop=event_loop)

        # Act
        data_client = self.data_client

        # Assert
        assert data_client is not None

    @pytest.mark.asyncio
    async def test_subscribe_trade_ticks(self, event_loop, instrument_aapl, contract_details_aapl):
        # Arrange
        self._async_setup(loop=event_loop)

        # Act
        results = []

        def collect(ticker: Ticker):
            results.append(ticker)

        self.data_client._on_ticker_update = collect
        self.data_client.instrument_provider.contract_details[
            instrument_aapl.id
        ] = contract_details_aapl

        instrument_id = instrument_aapl.id
        self.data_client.subscribe_trade_ticks(instrument_id=instrument_id)

        # Assert
        await asyncio.sleep(10)
