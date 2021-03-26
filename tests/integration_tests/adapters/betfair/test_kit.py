import pathlib
from unittest import mock

import betfairlightweight
import orjson
import pandas as pd

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instrument import BettingInstrument
from nautilus_trader.trading.portfolio import Portfolio
from tests import TESTS_PACKAGE_ROOT
from tests.test_kit.mocks import MockLiveExecutionEngine
from tests.test_kit.stubs import TestStubs


TEST_PATH = pathlib.Path(
    TESTS_PACKAGE_ROOT + "/integration_tests/adapters/betfair/responses/"
)


class BetfairTestStubs(TestStubs):
    @staticmethod
    def integration_endpoint():
        return "stream-api-integration.betfair.com"

    @staticmethod
    def instrument_provider(betfair_client) -> BetfairInstrumentProvider:
        mock.patch(
            "betfairlightweight.endpoints.navigation.Navigation.list_navigation",
            return_value=BetfairTestStubs.navigation(),
        )
        mock.patch(
            "betfairlightweight.endpoints.betting.Betting.resp_market_catalogue",
            return_value=BetfairTestStubs.market_catalogue(),
        )
        return BetfairInstrumentProvider(
            client=betfair_client,
            logger=BetfairTestStubs.live_logger(BetfairTestStubs.clock()),
            market_filter={"event_type_name": "Tennis"},
            load_all=False,
        )

    @staticmethod
    def clock():
        return LiveClock()

    @staticmethod
    def live_logger(clock):
        return LiveLogger(clock)

    @staticmethod
    def portfolio(clock, live_logger):
        return Portfolio(
            clock=clock,
            logger=live_logger,
        )

    @staticmethod
    def position_id():
        return PositionId("1")

    @staticmethod
    def instrument_id():
        return InstrumentId(symbol=Symbol("Test"), venue=BETFAIR_VENUE)

    @staticmethod
    def uuid():
        return UUIDFactory().generate()

    @staticmethod
    def account_id() -> AccountId:
        return AccountId(BETFAIR_VENUE.value, "000")

    @staticmethod
    def data_engine(event_loop, clock, live_logger, portfolio):
        return LiveDataEngine(
            loop=event_loop,
            portfolio=portfolio,
            clock=clock,
            logger=live_logger,
        )

    @staticmethod
    def exec_engine(event_loop, clock, live_logger, portfolio, trader_id):
        database = BypassExecutionDatabase(trader_id=trader_id, logger=live_logger)
        return MockLiveExecutionEngine(
            loop=event_loop,
            database=database,
            portfolio=portfolio,
            clock=clock,
            logger=live_logger,
        )

    @staticmethod
    def betting_instrument():
        return BettingInstrument(
            venue_name=BETFAIR_VENUE.value,
            betting_type="ODDS",
            competition_id="12282733",
            competition_name="NFL",
            event_country_code="GB",
            event_id="29678534",
            event_name="NFL",
            event_open_date=pd.Timestamp("2022-02-07 23:30:00+00:00").to_pydatetime(),
            event_type_id="6423",
            event_type_name="American Football",
            market_id="1.179082386",
            market_name="AFC Conference Winner",
            market_start_time=pd.Timestamp("2022-02-07 23:30:00+00:00").to_pydatetime(),
            market_type="SPECIAL",
            selection_handicap="0.0",
            selection_id="50214",
            selection_name="Kansas City Chiefs",
            currency="GBP",
        )

    @staticmethod
    def betfair_client():
        # Betfair client login
        mock.patch("betfairlightweight.endpoints.login.Login.__call__")
        return betfairlightweight.APIClient(
            username="username",
            password="password",
            app_key="app_key",
            certs="cert_location",
        )

    @staticmethod
    def betfair_order_socket():
        return BetfairOrderStreamClient(
            client=BetfairTestStubs.betfair_client(), message_handler=None
        )

    @staticmethod
    def betfair_market_socket():
        return BetfairMarketStreamClient(
            client=BetfairTestStubs.betfair_client(), message_handler=None
        )

    @staticmethod
    async def execution_client(
        betfair_client, account_id, exec_engine, clock, live_logger
    ) -> BetfairExecutionClient:
        client = BetfairExecutionClient(
            client=betfair_client,
            account_id=account_id,
            engine=exec_engine,
            clock=clock,
            logger=live_logger,
        )
        client.instrument_provider().load_all()
        exec_engine.register_client(client)
        return client

    @staticmethod
    def betfair_data_client(betfair_client, data_engine, clock, live_logger):
        client = BetfairDataClient(
            client=betfair_client,
            engine=data_engine,
            clock=clock,
            logger=live_logger,
        )
        client.instrument_provider().load_all()
        data_engine.register_client(client)
        return client

    # ---- test data

    @staticmethod
    def navigation():
        return orjson.loads((TEST_PATH / "navigation.json").read_bytes())

    @staticmethod
    def market_catalogue():
        return orjson.loads((TEST_PATH / "market_catalogue.json").read_bytes())

    @staticmethod
    def account_detail():
        return orjson.loads((TEST_PATH / "account_detail.json").read_bytes())

    @staticmethod
    def account_funds_no_exposure():
        return orjson.loads((TEST_PATH / "account_funds_no_exposure.json").read_bytes())

    @staticmethod
    def account_funds_with_exposure():
        return orjson.loads(
            (TEST_PATH / "account_funds_with_exposure.json").read_bytes()
        )

    @staticmethod
    def cleared_orders():
        return orjson.loads((TEST_PATH / "cleared_orders.json").read_bytes())

    @staticmethod
    def current_orders():
        return orjson.loads((TEST_PATH / "current_orders.json").read_bytes())

    @staticmethod
    def current_orders_empty():
        return orjson.loads((TEST_PATH / "current_orders_empty.json").read_bytes())

    @staticmethod
    def streaming_ocm_FULL_IMAGE():
        return (TEST_PATH / "streaming_ocm_FULL_IMAGE.json").read_bytes()

    @staticmethod
    def streaming_ocm_EMPTY_IMAGE():
        return (TEST_PATH / "streaming_ocm_EMPTY_IMAGE.json").read_bytes()

    @staticmethod
    def streaming_ocm_NEW_FULL_IMAGE():
        return (TEST_PATH / "streaming_ocm_NEW_FULL_IMAGE.json").read_bytes()

    @staticmethod
    def streaming_ocm_SUB_IMAGE():
        return (TEST_PATH / "streaming_ocm_SUB_IMAGE.json").read_bytes()

    @staticmethod
    def streaming_ocm_UPDATE():
        return (TEST_PATH / "streaming_ocm_UPDATE.json").read_bytes()

    @staticmethod
    def streaming_mcm_HEARTBEAT():
        return (TEST_PATH / "streaming_mcm_HEARTBEAT.json").read_bytes()

    @staticmethod
    def streaming_mcm_SUB_IMAGE():
        return (TEST_PATH / "streaming_mcm_SUB_IMAGE.json").read_bytes()

    @staticmethod
    def streaming_mcm_SUB_IMAGE_no_market_def():
        return (TEST_PATH / "streaming_mcm_SUB_IMAGE_no_market_def.json").read_bytes()

    @staticmethod
    def streaming_mcm_RESUB_DELTA():
        return (TEST_PATH / "streaming_mcm_RESUB_DELTA.json").read_bytes()

    @staticmethod
    def streaming_mcm_UPDATE():
        return (TEST_PATH / "streaming_mcm_UPDATE.json").read_bytes()

    @staticmethod
    def streaming_mcm_UPDATE_md():
        return (TEST_PATH / "streaming_mcm_UPDATE_md.json").read_bytes()

    @staticmethod
    def streaming_mcm_UPDATE_tv():
        return (TEST_PATH / "streaming_mcm_UPDATE_tv.json").read_bytes()
