import pytest

from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import BetfairLiveExecClientFactory
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestBetfairFactory:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()

        self.trader_id = TestIdStubs.trader_id()
        self.venue = BETFAIR_VENUE

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )
        self.cache = TestComponentStubs.cache()

        return

    @pytest.mark.asyncio
    def test_create(self):
        data_config = BetfairDataClientConfig(
            account_currency="GBP",
            username="SOME_BETFAIR_USERNAME",
            password="SOME_BETFAIR_PASSWORD",
            app_key="SOME_BETFAIR_APP_KEY",
            certs_dir="SOME_BETFAIR_CERTS_DIR",
        )
        exec_config = BetfairExecClientConfig(
            account_currency="GBP",
            username="SOME_BETFAIR_USERNAME",
            password="SOME_BETFAIR_PASSWORD",
            app_key="SOME_BETFAIR_APP_KEY",
            certs_dir="SOME_BETFAIR_CERTS_DIR",
        )

        data_client = BetfairLiveDataClientFactory.create(
            loop=self.loop,
            name=BETFAIR_VENUE.value,
            config=data_config,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        exec_client = BetfairLiveExecClientFactory.create(
            loop=self.loop,
            name=BETFAIR_VENUE.value,
            config=exec_config,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Assert
        assert BetfairDataClient is type(data_client)
        assert BetfairExecutionClient is type(exec_client)
