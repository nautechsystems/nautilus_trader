# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
import bz2
import pathlib
from unittest import mock

import betfairlightweight
import orjson
import pandas as pd

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import make_instruments
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.core.uuid import UUID
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.commands import UpdateOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests import TESTS_PACKAGE_ROOT
from tests.test_kit.mocks import MockLiveExecutionEngine
from tests.test_kit.mocks import MockLiveRiskEngine
from tests.test_kit.stubs import TestStubs


TEST_PATH = pathlib.Path(
    TESTS_PACKAGE_ROOT + "/integration_tests/adapters/betfair/responses/"
)
DATA_PATH = pathlib.Path(TESTS_PACKAGE_ROOT + "/test_kit/data/betfair")


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
        return LiveLogger(loop=asyncio.get_event_loop(), clock=clock)

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
        return BetfairTestStubs.betting_instrument().id

    @staticmethod
    def uuid():
        return UUID(
            value=b"\x03\x89\x90\xc6\x19\xd2\xb5\xc87\xa6\xfe\x91\xf9\xb7\xb9\xed"
        )

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
    def exec_engine(event_loop, clock, live_logger, portfolio):
        return MockLiveExecutionEngine(
            loop=event_loop,
            portfolio=portfolio,
            cache=TestStubs.cache,
            clock=clock,
            logger=live_logger,
        )

    @staticmethod
    def risk_engine(event_loop, clock, live_logger, portfolio, exec_engine):
        return MockLiveRiskEngine(
            loop=event_loop,
            exec_engine=exec_engine,
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
            ts_event_ns=BetfairTestStubs.clock().timestamp_ns(),
            ts_recv_ns=BetfairTestStubs.clock().timestamp_ns(),
        )

    @staticmethod
    def betfair_client():
        mock.patch("betfairlightweight.endpoints.login.Login.__call__")
        return betfairlightweight.APIClient(  # noqa (S106 Possible hardcoded password: 'password')
            username="username",
            password="password",
            app_key="app_key",
            certs="cert_location",
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
    def market_ids():
        """
        A list of market_ids used by the tests. Used in `navigation_short` and `market_catalogue_short`
        """
        return (
            "1.148894697",
            "1.159045690",
            "1.160683973",
            "1.160740937",
            "1.160837650",
            "1.163016936",
            "1.164555327",
            "1.166577732",
            "1.166881256",
            "1.167249009",
            "1.167249195",
            "1.167249197",
            "1.170262001",
            "1.170262002",
            "1.170436895",
            "1.170508139",
            "1.171431228",
            "1.172698506",
            "1.173509358",
            "1.175061137",
            "1.175061138",
            "1.175135109",
            "1.175492291",
            "1.175492292",
            "1.175492293",
            "1.175492294",
            "1.175492295",
            "1.175492296",
            "1.175775529",
            "1.175776462",
            "1.176584117",
            "1.176621195",
            "1.177125720",
            "1.177125722",
            "1.177126187",
            "1.177126652",
            "1.177126864",
            "1.178198625",
            "1.180294966",
            "1.180294971",
            "1.180434883",
            "1.180604981",
            "1.180727728",
            "1.180737193",
            "1.180770798",
        )

    @staticmethod
    def market_catalogue_short():
        catalogue = BetfairTestStubs.market_catalogue()
        market_ids = BetfairTestStubs.market_ids()
        return [
            m
            for m in catalogue
            if m["eventType"]["name"] in ("Horse Racing", "American Football")
            or m["marketId"] in market_ids
        ]

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
    def list_cleared_orders(order_id=None):
        data = orjson.loads((TEST_PATH / "list_cleared_orders.json").read_bytes())
        if order_id:
            data["clearedOrders"] = [
                order for order in data["clearedOrders"] if order["betId"] == order_id
            ]
        return data

    @staticmethod
    def list_current_orders():
        return orjson.loads((TEST_PATH / "list_current_orders.json").read_bytes())

    @staticmethod
    def list_current_orders_empty():
        return orjson.loads((TEST_PATH / "list_current_orders_empty.json").read_bytes())

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
    def streaming_ocm_order_update():
        return (TEST_PATH / "streaming_ocm_order_update.json").read_bytes()

    @staticmethod
    def streaming_ocm_FILLED():
        return (TEST_PATH / "streaming_ocm_FILLED.json").read_bytes()

    @staticmethod
    def streaming_ocm_MIXED():
        return (TEST_PATH / "streaming_ocm_MIXED.json").read_bytes()

    @staticmethod
    def streaming_ocm_DUPLICATE_EXECUTION():
        return (TEST_PATH / "streaming_ocm_DUPLICATE_EXECUTION.json").read_bytes()

    @staticmethod
    def streaming_mcm_HEARTBEAT():
        return (TEST_PATH / "streaming_mcm_HEARTBEAT.json").read_bytes()

    @staticmethod
    def streaming_mcm_live_IMAGE():
        return (TEST_PATH / "streaming_mcm_live_IMAGE.json").read_bytes()

    @staticmethod
    def streaming_mcm_live_UPDATE():
        return (TEST_PATH / "streaming_mcm_live_UPDATE.json").read_bytes()

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

    @staticmethod
    def streaming_market_updates():
        return (
            (TEST_PATH / "streaming_market_updates.log").read_text().strip().split("\n")
        )

    @staticmethod
    def place_orders_success():
        return orjson.loads(
            (TEST_PATH / "betting_place_order_success.json").read_bytes()
        )

    @staticmethod
    def place_orders_error():
        return orjson.loads((TEST_PATH / "betting_place_order_error.json").read_bytes())

    @staticmethod
    def replace_orders_success():
        return orjson.loads(
            (TEST_PATH / "betting_replace_orders_success.json").read_bytes()
        )

    @staticmethod
    def replace_orders_resp_success():
        return orjson.loads(
            (TEST_PATH / "betting_post_replace_order_success.json").read_bytes()
        )

    @staticmethod
    def cancel_orders_success():
        return orjson.loads(
            (TEST_PATH / "betting_cancel_orders_success.json").read_bytes()
        )

    @staticmethod
    def raw_market_updates(market="1.166811431", runner1="60424", runner2="237478"):
        def _fix_ids(r):
            return (
                r.replace(market.encode(), b"1.180737206")
                .replace(runner1.encode(), b"19248890")
                .replace(runner2.encode(), b"38848248")
            )

        lines = bz2.open(DATA_PATH / f"{market}.bz2").readlines()
        return [orjson.loads(_fix_ids(line.strip())) for line in lines]

    @staticmethod
    def raw_market_updates_instruments(
        market="1.166811431", runner1="60424", runner2="237478", currency="GBP"
    ):
        updates = BetfairTestStubs.raw_market_updates(
            market=market, runner1=runner1, runner2=runner2
        )
        market_def = updates[0]["mc"][0]
        instruments = make_instruments(market_def, currency)
        return instruments

    @staticmethod
    def parsed_market_updates(
        instrument_provider, market="1.166811431", runner1="60424", runner2="237478"
    ):
        updates = []
        for raw in BetfairTestStubs.raw_market_updates(
            market=market, runner1=runner1, runner2=runner2
        ):
            for message in on_market_update(
                instrument_provider=instrument_provider, update=raw
            ):
                updates.append(message)
        return updates

    @staticmethod
    def make_order(engine: MockLiveExecutionEngine) -> LimitOrder:
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            BetfairTestStubs.clock(),
            BetfairTestStubs.logger(),
        )

        engine.register_strategy(strategy)

        order = strategy.order_factory.limit(
            BetfairTestStubs.instrument_id(),
            OrderSide.BUY,
            Quantity.from_int(10),
            Price.from_str("0.50"),
        )
        return order

    @staticmethod
    def submit_order_command():
        return SubmitOrder(
            trader_id=BetfairTestStubs.trader_id(),
            strategy_id=BetfairTestStubs.strategy_id(),
            position_id=BetfairTestStubs.position_id(),
            order=LimitOrder(
                client_order_id=ClientOrderId(
                    f"O-20210410-022422-001-001-{BetfairTestStubs.strategy_id().value}"
                ),
                strategy_id=BetfairTestStubs.strategy_id(),
                instrument_id=BetfairTestStubs.instrument_id(),
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(10),
                price=Price(0.33, precision=5),
                time_in_force=TimeInForce.GTC,
                expire_time=None,
                init_id=BetfairTestStubs.uuid(),
                timestamp_ns=BetfairTestStubs.clock().timestamp_ns(),
            ),
            command_id=BetfairTestStubs.uuid(),
            timestamp_ns=BetfairTestStubs.clock().timestamp_ns(),
        )

    @staticmethod
    def update_order_command(instrument_id=None, client_order_id=None):
        if instrument_id is None:
            instrument_id = BetfairTestStubs.instrument_id()
        return UpdateOrder(
            trader_id=BetfairTestStubs.trader_id(),
            strategy_id=BetfairTestStubs.strategy_id(),
            instrument_id=instrument_id,
            client_order_id=client_order_id
            or ClientOrderId("O-20210410-022422-001-001-1"),
            venue_order_id=VenueOrderId("001"),
            quantity=Quantity.from_int(50),
            price=Price(0.74347, precision=5),
            command_id=BetfairTestStubs.uuid(),
            timestamp_ns=BetfairTestStubs.clock().timestamp_ns(),
        )

    @staticmethod
    def cancel_order_command():
        return CancelOrder(
            trader_id=BetfairTestStubs.trader_id(),
            strategy_id=BetfairTestStubs.strategy_id(),
            instrument_id=BetfairTestStubs.instrument_id(),
            client_order_id=ClientOrderId("O-20210410-022422-001-001-1"),
            venue_order_id=VenueOrderId("229597791245"),
            command_id=BetfairTestStubs.uuid(),
            timestamp_ns=BetfairTestStubs.clock().timestamp_ns(),
        )

    @staticmethod
    def make_order_place_response(
        market_id="1.182127885",
        customer_order_ref="O-20210418-015047-001-001-3",
        bet_id="230486317487",
    ):
        return {
            "customerRef": "c8dc484d5cea2ab472c844859bca7010",
            "status": "SUCCESS",
            "marketId": market_id,
            "instructionReports": [
                {
                    "status": "SUCCESS",
                    "instruction": {
                        "selectionId": 237477,
                        "handicap": 0.0,
                        "limitOrder": {
                            "size": 10.0,
                            "price": 1.75,
                            "persistenceType": "PERSIST",
                        },
                        "customerOrderRef": customer_order_ref,
                        "orderType": "LIMIT",
                        "side": "LAY",
                    },
                    "betId": bet_id,
                    "placedDate": "2021-04-18T01:50:49.000Z",
                    "averagePriceMatched": 1.73,
                    "sizeMatched": 1.12,
                    "orderStatus": "EXECUTABLE",
                }
            ],
        }
