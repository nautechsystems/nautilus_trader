# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import bz2
import contextlib
import pathlib
from asyncio import Future
from functools import partial
from typing import Optional
from unittest.mock import MagicMock
from unittest.mock import patch

import msgspec
import numpy as np
import pandas as pd
from aiohttp import ClientResponse

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import make_instruments
from nautilus_trader.adapters.betfair.spec.streaming import OCM
from nautilus_trader.adapters.betfair.spec.streaming import OrderAccountChange
from nautilus_trader.adapters.betfair.spec.streaming import OrderChanges
from nautilus_trader.adapters.betfair.spec.streaming import UnmatchedOrder
from nautilus_trader.adapters.betfair.spec.streaming import stream_decode
from nautilus_trader.adapters.betfair.util import flatten_tree
from nautilus_trader.adapters.betfair.util import historical_instrument_provider_loader
from nautilus_trader.backtest.data.providers import TestDataProvider
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.config import StreamingConfig
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.market import MarketOrder
from nautilus_trader.persistence.external.core import make_raw_files
from nautilus_trader.persistence.external.readers import TextReader
from tests import TESTS_PACKAGE_ROOT
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.stubs.commands import TestCommandStubs
from tests.test_kit.stubs.component import TestComponentStubs
from tests.test_kit.stubs.execution import TestExecStubs
from tests.test_kit.stubs.identifiers import TestIdStubs


TEST_PATH = pathlib.Path(TESTS_PACKAGE_ROOT + "/integration_tests/adapters/betfair/resources/")
DATA_PATH = pathlib.Path(TESTS_PACKAGE_ROOT + "/test_kit/data/betfair")


# monkey patch MagicMock
async def async_magic():
    pass


MagicMock.__await__ = lambda x: async_magic().__await__()


def mock_betfair_request(obj, response, attr="request"):
    mock_resp = MagicMock(spec=ClientResponse)
    mock_resp.data = msgspec.json.encode(response)

    setattr(obj, attr, MagicMock(return_value=Future()))
    getattr(obj, attr).return_value.set_result(mock_resp)


def format_current_orders(
    bet_id="1",
    market_id="1.180575118",
    selection_id=39980,
    customer_order_ref="O-20211118-030800-000",
    customer_strategy_ref="TestStrategy-1",
):
    return [
        {
            "betId": bet_id,
            "marketId": market_id,
            "selectionId": selection_id,
            "handicap": 0.0,
            "priceSize": {"price": 5.0, "size": 10.0},
            "bspLiability": 0.0,
            "side": "BACK",
            "status": "EXECUTABLE",
            "persistenceType": "LAPSE",
            "orderType": "LIMIT",
            "placedDate": "2021-03-24T06:47:02.000Z",
            "averagePriceMatched": 0.0,
            "sizeMatched": 0.0,
            "sizeRemaining": 10.0,
            "sizeLapsed": 0.0,
            "sizeCancelled": 0.0,
            "sizeVoided": 0.0,
            "regulatorCode": "MALTA LOTTERIES AND GAMBLING AUTHORITY",
            "customerOrderRef": customer_order_ref,
            "customerStrategyRef": customer_strategy_ref,
        }
    ]


class BetfairTestStubs:
    @staticmethod
    def integration_endpoint():
        return "stream-api-integration.betfair.com"

    @staticmethod
    def instrument_provider(betfair_client) -> BetfairInstrumentProvider:
        return BetfairInstrumentProvider(
            client=betfair_client,
            logger=TestComponentStubs.logger(),
        )

    @staticmethod
    def betting_instrument(
        market_id: str = "1.179082386", selection_id: str = "50214", handicap: str = "0.0"
    ):
        return BettingInstrument(
            venue_name=BETFAIR_VENUE.value,
            betting_type="ODDS",
            competition_id="12282733",
            competition_name="NFL",
            event_country_code="GB",
            event_id="29678534",
            event_name="NFL",
            event_open_date=pd.Timestamp("2022-02-07 23:30:00+00:00"),
            event_type_id="6423",
            event_type_name="American Football",
            market_id=market_id,
            market_name="AFC Conference Winner",
            market_start_time=pd.Timestamp("2022-02-07 23:30:00+00:00"),
            market_type="SPECIAL",
            selection_handicap=handicap,
            selection_id=selection_id,
            selection_name="Kansas City Chiefs",
            currency="GBP",
            ts_event=TestComponentStubs.clock().timestamp_ns(),
            ts_init=TestComponentStubs.clock().timestamp_ns(),
        )

    @staticmethod
    def betting_instrument_handicap():
        return BettingInstrument.from_dict(
            {
                "venue_name": "BETFAIR",
                "event_type_id": "61420",
                "event_type_name": "Australian Rules",
                "competition_id": "11897406",
                "competition_name": "AFL",
                "event_id": "30777079",
                "event_name": "GWS v Richmond",
                "event_country_code": "AU",
                "event_open_date": "2021-08-13T09:50:00+00:00",
                "betting_type": "ASIAN_HANDICAP_DOUBLE_LINE",
                "market_id": "1.186249896",
                "market_name": "Handicap",
                "market_start_time": "2021-08-13T09:50:00+00:00",
                "market_type": "HANDICAP",
                "selection_id": "5304641",
                "selection_name": "GWS",
                "selection_handicap": "-5.5",
                "currency": "AUD",
                "ts_event": 1628753086658060000,
                "ts_init": 1628753086658060000,
            }
        )

    @staticmethod
    def betfair_client(loop, logger) -> BetfairClient:
        client = BetfairClient(
            username="", password="", app_key="", cert_dir="", ssl=False, loop=loop, logger=logger
        )

        async def request(method, url, **kwargs):
            rpc_method = kwargs.get("json", {}).get("method") or url
            responses = {
                "https://api.betfair.com/exchange/betting/rest/v1/en/navigation/menu.json": BetfairResponses.navigation_list_navigation,
                "AccountAPING/v1.0/getAccountDetails": BetfairResponses.account_details,
                "AccountAPING/v1.0/getAccountFunds": BetfairResponses.account_funds_no_exposure,
                "SportsAPING/v1.0/listMarketCatalogue": BetfairResponses.betting_list_market_catalogue,
                "SportsAPING/v1.0/list": BetfairResponses.betting_list_market_catalogue,
                "SportsAPING/v1.0/placeOrders": BetfairResponses.betting_place_order_success(),
                "SportsAPING/v1.0/replaceOrders": BetfairResponses.betting_replace_orders_success(),
                "SportsAPING/v1.0/cancelOrders": BetfairResponses.betting_cancel_orders_success,
                "SportsAPING/v1.0/listCurrentOrders": BetfairResponses.list_current_orders,
                "SportsAPING/v1.0/listClearedOrders": BetfairResponses.list_cleared_orders,
            }
            kw = {}
            if rpc_method == "SportsAPING/v1.0/listMarketCatalogue":
                kw = {"filters": kwargs["json"]["params"]["filter"]}
            if rpc_method in responses:
                resp = MagicMock(spec=ClientResponse)
                resp.data = msgspec.json.encode(responses[rpc_method](**kw))
                return resp
            raise KeyError(rpc_method)

        client.request = MagicMock()  # type: ignore
        client.request.side_effect = request
        client.session_token = "xxxsessionToken="

        return client

    @staticmethod
    def betfair_data_client(betfair_client, data_engine, cache, clock, live_logger):
        client = BetfairDataClient(
            client=betfair_client,
            engine=data_engine,
            cache=cache,
            clock=clock,
            logger=live_logger,
        )
        client.instrument_provider().load_all()
        data_engine.register_client(client)
        return client

    @staticmethod
    def market_order(side=None, time_in_force=None) -> MarketOrder:
        return TestExecStubs.market_order(
            instrument_id=TestIdStubs.betting_instrument_id(),
            client_order_id=ClientOrderId(
                f"O-20210410-022422-001-001-{TestIdStubs.strategy_id().value}"
            ),
            order_side=side or OrderSide.BUY,
            quantity=Quantity.from_int(10),
            time_in_force=time_in_force or TimeInForce.GTC,
        )

    @staticmethod
    def limit_order(
        quantity: Optional[Quantity] = None,
        price: Optional[Price] = None,
        time_in_force: Optional[TimeInForce] = None,
        **kwargs,
    ):
        return TestExecStubs.limit_order(
            instrument_id=TestIdStubs.betting_instrument_id(),
            quantity=quantity or Quantity.from_int(10),
            price=price or Price(0.33, precision=5),
            time_in_force=time_in_force,
            **kwargs,
        )

    @staticmethod
    def submit_order_command(time_in_force=TimeInForce.GTC, order=None):
        order = order or BetfairTestStubs.limit_order()
        return TestCommandStubs.submit_order_command(
            order=order or TestExecStubs.limit_order(time_in_force=time_in_force),
        )

    @staticmethod
    def modify_order_command(
        instrument_id: Optional[InstrumentId] = None,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ):
        return TestCommandStubs.modify_order_command(
            instrument_id=instrument_id or TestIdStubs.betting_instrument_id(),
            client_order_id=client_order_id or ClientOrderId("O-20210410-022422-001-001-1"),
            venue_order_id=venue_order_id or VenueOrderId("001"),
            quantity=Quantity.from_int(50),
            price=Price(0.74347, precision=5),
        )

    @staticmethod
    def cancel_order_command(instrument_id=None, client_order_id=None, venue_order_id=None):
        return TestCommandStubs.cancel_order_command(
            instrument_id=instrument_id or TestIdStubs.betting_instrument_id(),
            client_order_id=client_order_id or ClientOrderId("O-20210410-022422-001-001-1"),
            venue_order_id=venue_order_id or VenueOrderId("228302937743"),
        )

    @staticmethod
    def make_submitted_order(order: Optional[Order] = None, **kwargs):
        order = order or BetfairTestStubs.limit_order(**kwargs)
        return TestExecStubs.make_submitted_order(order=order)

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

    @staticmethod
    def parse_betfair(line, instrument_provider):
        yield from on_market_update(
            instrument_provider=instrument_provider, update=msgspec.json.decode(line)
        )

    @staticmethod
    def betfair_reader(instrument_provider=None, **kwargs):
        instrument_provider = instrument_provider or BetfairInstrumentProvider.from_instruments([])
        reader = TextReader(
            line_parser=partial(
                BetfairTestStubs.parse_betfair, instrument_provider=instrument_provider
            ),
            instrument_provider=instrument_provider,
            instrument_provider_update=historical_instrument_provider_loader,
            **kwargs,
        )
        return reader

    @staticmethod
    def betfair_venue_config() -> BacktestVenueConfig:
        return BacktestVenueConfig(  # type: ignore
            name="BETFAIR",
            oms_type="NETTING",
            account_type="BETTING",
            base_currency="GBP",
            starting_balances=["10000 GBP"],
            book_type="L2_MBP",
        )

    @staticmethod
    def streaming_config(catalog_path: str, catalog_fs_protocol: str = "memory") -> StreamingConfig:
        return StreamingConfig(
            catalog_path=catalog_path,
            fs_protocol=catalog_fs_protocol,
            kind="backtest",
            persit_logs=True,
        )

    @staticmethod
    def betfair_backtest_run_config(
        catalog_path: str,
        instrument_id: str,
        catalog_fs_protocol: str = "memory",
        persist=True,
        add_strategy=True,
        bypass_risk=False,
    ) -> BacktestRunConfig:
        engine_config = BacktestEngineConfig(
            log_level="INFO",
            bypass_logging=True,
            risk_engine=RiskEngineConfig(bypass=bypass_risk),
            streaming=BetfairTestStubs.streaming_config(catalog_path=catalog_path)
            if persist
            else None,
            strategies=[
                ImportableStrategyConfig(
                    strategy_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalance",
                    config_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalanceConfig",
                    config=dict(
                        instrument_id=instrument_id,
                        max_trade_size=50,
                    ),
                )
            ]
            if add_strategy
            else None,
        )
        run_config = BacktestRunConfig(  # type: ignore
            engine=engine_config,
            venues=[BetfairTestStubs.betfair_venue_config()],
            data=[
                BacktestDataConfig(  # type: ignore
                    data_cls=TradeTick.fully_qualified_name(),
                    catalog_path=catalog_path,
                    catalog_fs_protocol=catalog_fs_protocol,
                    instrument_id=instrument_id,
                ),
                BacktestDataConfig(  # type: ignore
                    data_cls=OrderBookData.fully_qualified_name(),
                    catalog_path=catalog_path,
                    catalog_fs_protocol=catalog_fs_protocol,
                    instrument_id=instrument_id,
                ),
            ],
        )
        return run_config


class BetfairRequests:
    @staticmethod
    def load(filename):
        return msgspec.json.decode((TEST_PATH / "requests" / filename).read_bytes())

    @staticmethod
    def account_details():
        return BetfairRequests.load("account_details.json")

    @staticmethod
    def account_funds():
        return BetfairRequests.load("account_funds.json")

    @staticmethod
    def betting_cancel_order():
        return BetfairRequests.load("betting_cancel_order.json")

    @staticmethod
    def betting_list_market_catalogue():
        return BetfairRequests.load("betting_list_market_catalogue.json")

    @staticmethod
    def betting_place_order():
        return BetfairRequests.load("betting_place_order.json")

    @staticmethod
    def betting_place_order_handicap():
        return BetfairRequests.load("betting_place_order_handicap.json")

    @staticmethod
    def betting_place_order_bsp():
        return BetfairRequests.load("betting_place_order_bsp.json")

    @staticmethod
    def betting_replace_order():
        return BetfairRequests.load("betting_replace_order.json")

    @staticmethod
    def cert_login():
        return BetfairRequests.load("cert_login.json")

    @staticmethod
    def navigation_list_navigation_request():
        return BetfairRequests.load("navigation_list_navigation.json")


class BetfairResponses:
    @staticmethod
    def load(filename):
        return msgspec.json.decode((TEST_PATH / "responses" / filename).read_bytes())

    @staticmethod
    def account_details():
        return BetfairResponses.load("account_details.json")

    @staticmethod
    def account_funds_no_exposure():
        return BetfairResponses.load("account_funds_no_exposure.json")

    @staticmethod
    def account_funds_with_exposure():
        return BetfairResponses.load("account_funds_with_exposure.json")

    @staticmethod
    def account_funds_error():
        return BetfairResponses.load("account_funds_error.json")

    @staticmethod
    def betting_cancel_orders_success():
        return BetfairResponses.load("betting_cancel_orders_success.json")

    @staticmethod
    def betting_cancel_orders_error():
        return BetfairResponses.load("betting_cancel_orders_error.json")

    @staticmethod
    def betting_place_order_error():
        return BetfairResponses.load("betting_place_order_error.json")

    @staticmethod
    def betting_place_order_success():
        return BetfairResponses.load("betting_place_order_success.json")

    @staticmethod
    def betting_place_orders_old():
        return BetfairResponses.load("betting_place_orders_old.json")

    @staticmethod
    def betting_replace_orders_success():
        return BetfairResponses.load("betting_replace_orders_success.json")

    @staticmethod
    def betting_replace_orders_success_multi():
        return BetfairResponses.load("betting_replace_orders_success_multi.json")

    @staticmethod
    def cert_login():
        return BetfairResponses.load("cert_login.json")

    @staticmethod
    def list_cleared_orders():
        return BetfairResponses.load("list_cleared_orders.json")

    @staticmethod
    def list_current_orders():
        return BetfairResponses.load("list_current_orders.json")

    @staticmethod
    def list_current_orders_empty():
        return BetfairResponses.load("list_current_orders_empty.json")

    @staticmethod
    def betting_list_market_catalogue(filters=None):
        result = BetfairResponses.load("betting_list_market_catalogue.json")
        filters = filters or {}
        if "marketIds" in filters:
            result = [r for r in result if r["marketId"] in filters["marketIds"]]
        return {"jsonrpc": "2.0", "result": result, "id": 1}

    @staticmethod
    def navigation_list_navigation_response():
        return BetfairResponses.load("navigation_list_navigation.json")


class BetfairStreaming:
    @staticmethod
    def decode(raw: bytes, iterate: bool = False):
        if iterate:
            return [stream_decode(msgspec.json.encode(r)) for r in msgspec.json.decode(raw)]
        return stream_decode(raw)

    @staticmethod
    def load(filename, iterate: bool = False):
        raw = (TEST_PATH / "streaming" / filename).read_bytes()
        return BetfairStreaming.decode(raw=raw, iterate=iterate)

    @staticmethod
    def load_many(filename):
        lines = msgspec.json.decode((TEST_PATH / "streaming" / filename).read_bytes())
        return [BetfairStreaming.decode(raw=msgspec.json.encode(line)) for line in lines]

    @staticmethod
    def market_definition():
        return BetfairStreaming.load("streaming_market_definition.json")

    @staticmethod
    def market_definition_runner_removed():
        return BetfairStreaming.load(
            "streaming_market_definition_runner_removed.json", iterate=True
        )

    @staticmethod
    def ocm_FULL_IMAGE():
        return BetfairStreaming.load("streaming_ocm_FULL_IMAGE.json")

    @staticmethod
    def ocm_FULL_IMAGE_STRATEGY():
        return BetfairStreaming.load("streaming_ocm_FULL_IMAGE_STRATEGY.json")

    @staticmethod
    def ocm_EMPTY_IMAGE():
        return BetfairStreaming.load("streaming_ocm_EMPTY_IMAGE.json")

    @staticmethod
    def ocm_NEW_FULL_IMAGE():
        return BetfairStreaming.load("streaming_ocm_NEW_FULL_IMAGE.json")

    @staticmethod
    def ocm_SUB_IMAGE():
        return BetfairStreaming.load("streaming_ocm_SUB_IMAGE.json")

    @staticmethod
    def ocm_UPDATE():
        return BetfairStreaming.load("streaming_ocm_UPDATE.json")

    @staticmethod
    def ocm_CANCEL():
        return BetfairStreaming.load("streaming_ocm_CANCEL.json")

    @staticmethod
    def ocm_order_update():
        return BetfairStreaming.load("streaming_ocm_order_update.json")

    @staticmethod
    def ocm_FILLED():
        return BetfairStreaming.load("streaming_ocm_FILLED.json")

    @staticmethod
    def ocm_filled_different_price():
        return BetfairStreaming.load("streaming_ocm_filled_different_price.json")

    @staticmethod
    def ocm_MIXED():
        return BetfairStreaming.load("streaming_ocm_MIXED.json")

    @staticmethod
    def ocm_multiple_fills():
        return BetfairStreaming.load_many("streaming_ocm_multiple_fills.json")

    @staticmethod
    def ocm_DUPLICATE_EXECUTION():
        return BetfairStreaming.load_many("streaming_ocm_DUPLICATE_EXECUTION.json")

    @staticmethod
    def ocm_error_fill():
        return BetfairStreaming.load("streaming_ocm_error_fill.json")

    @staticmethod
    def mcm_BSP():
        return BetfairStreaming.load("streaming_mcm_BSP.json", iterate=True)

    @staticmethod
    def mcm_HEARTBEAT():
        return BetfairStreaming.load("streaming_mcm_HEARTBEAT.json")

    @staticmethod
    def mcm_latency():
        return BetfairStreaming.load("streaming_mcm_latency.json")

    @staticmethod
    def mcm_live_IMAGE():
        return BetfairStreaming.load("streaming_mcm_live_IMAGE.json")

    @staticmethod
    def mcm_live_UPDATE():
        return BetfairStreaming.load("streaming_mcm_live_UPDATE.json")

    @staticmethod
    def mcm_SUB_IMAGE():
        return BetfairStreaming.load("streaming_mcm_SUB_IMAGE.json")

    @staticmethod
    def mcm_SUB_IMAGE_no_market_def():
        return BetfairStreaming.load("streaming_mcm_SUB_IMAGE_no_market_def.json")

    @staticmethod
    def mcm_RESUB_DELTA():
        return BetfairStreaming.load("streaming_mcm_RESUB_DELTA.json")

    @staticmethod
    def mcm_UPDATE():
        return BetfairStreaming.load("streaming_mcm_UPDATE.json")

    @staticmethod
    def mcm_UPDATE_md():
        return BetfairStreaming.load("streaming_mcm_UPDATE_md.json")

    @staticmethod
    def mcm_UPDATE_tv():
        return BetfairStreaming.load("streaming_mcm_UPDATE_tv.json")

    @staticmethod
    def market_updates():
        return BetfairStreaming.load("streaming_market_updates.json", iterate=True)

    @staticmethod
    def generate_order_change_message(
        price=1.3,
        size=20,
        side="B",
        status="EC",
        sm=0,
        sr=0,
        sc=0,
        avp=0,
        order_id: str = "248485109136",
    ) -> OCM:

        assert side in ("B", "L"), "`side` should be 'B' or 'L'"
        return OCM(
            id=1,
            clk="1",
            pt=0,
            oc=[
                OrderAccountChange(
                    id="1",
                    orc=[
                        OrderChanges(
                            id=1,
                            uo=[
                                UnmatchedOrder(
                                    id=order_id,
                                    p=price,
                                    s=size,
                                    side=side,
                                    status=status,
                                    pt="P",
                                    ot="L",
                                    pd=1635217893000,
                                    md=int(pd.Timestamp.utcnow().timestamp()),
                                    sm=sm,
                                    sr=sr,
                                    sl=0,
                                    sc=sc,
                                    sv=0,
                                    rac="",
                                    rc="REG_LGA",
                                    rfo="O-20211026-031132-000",
                                    rfs="TestStrategy-1.",
                                    avp=avp,
                                ),
                            ],
                        )
                    ],
                )
            ],
        )
        return {
            "oc": [
                {
                    "id": "1",
                    "orc": [
                        {
                            "id": 1,
                            "uo": [
                                {
                                    "id": order_id,
                                    "p": price,
                                    "s": size,
                                    "side": side,
                                    "status": status,
                                    "pt": "P",
                                    "ot": "L",
                                    "pd": 1635217893000,
                                    "md": int(pd.Timestamp.utcnow().timestamp()),
                                    "sm": sm,
                                    "sr": sr,
                                    "sl": 0,
                                    "sc": sc,
                                    "sv": 0,
                                    "rac": "",
                                    "rc": "REG_LGA",
                                    "rfo": "O-20211026-031132-000",
                                    "rfs": "TestStrategy-1.",
                                    **({"avp": avp} if avp else {}),
                                }
                            ],
                        }
                    ],
                }
            ]
        }


class BetfairDataProvider:
    @staticmethod
    def market_ids():
        """
        A list of market_ids used by the tests. Used in `navigation_short` and `market_catalogue_short`.
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
            "1.180737206",
            "1.165003060",
        )

    @staticmethod
    def market_sample():
        np.random.seed(0)
        navigation = BetfairResponses.navigation_list_navigation()
        markets = list(flatten_tree(navigation))
        return np.random.choice(markets, size=int(len(markets) * 0.05))

    @staticmethod
    def market_catalogue_short():
        catalogue = BetfairResponses.betting_list_market_catalogue()["result"]
        market_ids = BetfairDataProvider.market_ids()
        return [
            m
            for m in catalogue
            if m["eventType"]["name"] in ("Horse Racing", "American Football")
            or m["marketId"] in market_ids
        ]

    @staticmethod
    def raw_market_updates(market="1.166811431", runner1="60424", runner2="237478") -> list:
        def _fix_ids(r):
            return (
                r.replace(market.encode(), b"1.180737206")
                .replace(runner1.encode(), b"19248890")
                .replace(runner2.encode(), b"38848248")
            )

        lines = bz2.open(DATA_PATH / f"{market}.bz2").readlines()
        return [stream_decode(_fix_ids(line.strip())) for line in lines]

    @staticmethod
    def raw_market_updates_instruments(
        market="1.166811431", runner1="60424", runner2="237478", currency="GBP"
    ):
        updates = BetfairDataProvider.raw_market_updates(
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
        for raw in BetfairDataProvider.raw_market_updates(
            market=market, runner1=runner1, runner2=runner2
        ):
            for message in on_market_update(instrument_provider=instrument_provider, update=raw):
                updates.append(message)
        return updates

    @staticmethod
    def betfair_feed_parsed(market_id="1.166564490", folder="data"):
        instrument_provider = BetfairInstrumentProvider.from_instruments([])
        reader = BetfairTestStubs.betfair_reader(instrument_provider=instrument_provider)
        files = make_raw_files(glob_path=f"{PACKAGE_ROOT}/{folder}/{market_id}*")

        data = []
        for rf in files:
            for block in rf.iter():
                data.extend(reader.parse(block=block))

        return data

    @staticmethod
    def betfair_trade_ticks():
        return [msg["trade"] for msg in TestDataProvider.l2_feed() if msg.get("op") == "trade"]

    @staticmethod
    def badly_formatted_log():
        return open(DATA_PATH / "badly_formatted.txt", "rb").read()


@contextlib.contextmanager
def mock_client_request(response):
    """
    Patch BetfairClient.request with a correctly formatted `response`.
    """
    mock_response = MagicMock(ClientResponse)
    mock_response.data = msgspec.json.encode(response)
    with patch.object(BetfairClient, "request", return_value=mock_response) as mock_request:
        yield mock_request
