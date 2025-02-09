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

import bz2
import gzip
import pathlib
from unittest.mock import MagicMock

import msgspec
import pandas as pd
from aiohttp import ClientResponse
from betfair_parser.spec.betting.type_definitions import MarketFilter
from betfair_parser.spec.common import EndpointType
from betfair_parser.spec.common import Handicap
from betfair_parser.spec.common import MarketId
from betfair_parser.spec.common import Request
from betfair_parser.spec.common import SelectionId
from betfair_parser.spec.common import encode
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import MatchedOrder
from betfair_parser.spec.streaming import Order
from betfair_parser.spec.streaming import OrderMarketChange
from betfair_parser.spec.streaming import OrderRunnerChange
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.adapters.betfair.common import BETFAIR_TICK_SCHEME
from nautilus_trader.adapters.betfair.constants import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairParser
from nautilus_trader.adapters.betfair.parsing.core import betting_instruments_from_file
from nautilus_trader.adapters.betfair.parsing.core import parse_betfair_file
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.adapters.betfair.providers import market_definition_to_instruments
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.config import StreamingConfig
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.instruments.betting import null_handicap
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog import ParquetDataCatalog


RESOURCES_PATH = pathlib.Path(__file__).parent.joinpath("resources")


# monkey patch MagicMock
async def async_magic():
    pass


MagicMock.__await__ = lambda x: async_magic().__await__()


def mock_betfair_request(obj, response):
    async def mock_request(method: HttpMethod, request: Request):
        mock_resp = MagicMock(spec=ClientResponse)
        response["id"] = request.id
        mock_resp.body = encode(response)
        return mock_resp

    setattr(obj, "_request", MagicMock(side_effect=mock_request))


class BetfairTestStubs:
    @staticmethod
    def trader_id() -> TraderId:
        return TraderId("001")

    @staticmethod
    def instrument_provider(
        betfair_client,
        config: BetfairInstrumentProviderConfig | None = None,
    ) -> BetfairInstrumentProvider:
        return BetfairInstrumentProvider(
            client=betfair_client,
            config=config or BetfairInstrumentProviderConfig(account_currency="GBP"),
        )

    @staticmethod
    def betfair_client(loop) -> BetfairHttpClient:
        client = BetfairHttpClient(
            username="",
            password="",
            app_key="",
        )

        async def request(method, request: Request, **kwargs):
            assert method  # required to stop mocks from breaking
            rpc_method = request.method
            responses = {
                "login": BetfairResponses.login_success,
                "AccountAPING/v1.0/getAccountDetails": BetfairResponses.account_details,
                "AccountAPING/v1.0/getAccountFunds": BetfairResponses.account_funds_no_exposure,
                "SportsAPING/v1.0/listMarketCatalogue": BetfairResponses.betting_list_market_catalogue,
                "SportsAPING/v1.0/list": BetfairResponses.betting_list_market_catalogue,
                "SportsAPING/v1.0/placeOrders": BetfairResponses.betting_place_order_success,
                "SportsAPING/v1.0/replaceOrders": BetfairResponses.betting_replace_orders_success,
                "SportsAPING/v1.0/cancelOrders": BetfairResponses.betting_cancel_orders_success,
                "SportsAPING/v1.0/listCurrentOrders": BetfairResponses.list_current_orders_executable,
                "SportsAPING/v1.0/listClearedOrders": BetfairResponses.list_cleared_orders,
            }
            kw = {}
            if rpc_method == "SportsAPING/v1.0/listMarketCatalogue":
                kw = {"filter_": request.params.filter}
            if rpc_method in responses:
                response = responses[rpc_method](**kw)  # type: ignore
                if "id" in response:
                    response["id"] = request.id
                resp = MagicMock(spec=ClientResponse)
                resp.body = msgspec.json.encode(response)
                return resp
            elif request.endpoint_type == EndpointType.NAVIGATION:
                resp = MagicMock(spec=ClientResponse)
                resp.body = msgspec.json.encode(BetfairResponses.navigation_list_navigation())
                return resp
            else:
                raise KeyError(rpc_method)

        client._request = MagicMock()  # type: ignore
        client._request.side_effect = request
        client._headers["X-Authentication"] = "xxxsessionToken="

        return client

    @staticmethod
    def make_order_place_response(
        market_id="1-182127885",
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
                },
            ],
        }

    @staticmethod
    def parse_betfair(line):
        parser = BetfairParser(currency="GBP")
        yield from parser.parse(stream_decode(line))

    @staticmethod
    def betfair_venue_config(
        name: str = "BETFAIR",
        book_type: str = "L1_MBP",
    ) -> BacktestVenueConfig:
        return BacktestVenueConfig(
            name=name,
            oms_type="NETTING",
            account_type="BETTING",
            base_currency="GBP",
            starting_balances=["10000 GBP"],
            book_type=book_type,
        )

    @staticmethod
    def streaming_config(
        catalog_path: str,
        catalog_fs_protocol: str = "memory",
        flush_interval_ms: int | None = None,
        include_types: list[type] | None = None,
    ) -> StreamingConfig:
        return StreamingConfig(
            catalog_path=catalog_path,
            fs_protocol=catalog_fs_protocol,
            flush_interval_ms=flush_interval_ms,
            include_types=include_types,
        )

    @staticmethod
    def backtest_run_config(
        catalog_path: str,
        instrument_id: InstrumentId,
        catalog_fs_protocol: str = "memory",
        persist: bool = True,
        add_strategy: bool = True,
        bypass_risk: bool = False,
        flush_interval_ms: int | None = None,
        bypass_logging: bool = True,
        log_level: str = "WARNING",
        venue_name: str = "BETFAIR",
        book_type: str = "L2_MBP",
    ) -> BacktestRunConfig:
        engine_config = BacktestEngineConfig(
            logging=LoggingConfig(
                log_level=log_level,
                bypass_logging=bypass_logging,
            ),
            risk_engine=RiskEngineConfig(bypass=bypass_risk),
            streaming=(
                BetfairTestStubs.streaming_config(
                    catalog_fs_protocol=catalog_fs_protocol,
                    catalog_path=catalog_path,
                    flush_interval_ms=flush_interval_ms,
                )
                if persist
                else None
            ),
            strategies=(
                [
                    ImportableStrategyConfig(
                        strategy_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalance",
                        config_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalanceConfig",
                        config={
                            "instrument_id": instrument_id.value,
                            "max_trade_size": 50,
                        },
                    ),
                ]
                if add_strategy
                else []
            ),
        )
        run_config = BacktestRunConfig(
            engine=engine_config,
            venues=[BetfairTestStubs.betfair_venue_config(name=venue_name, book_type=book_type)],
            data=[
                BacktestDataConfig(
                    data_cls=TradeTick.fully_qualified_name(),
                    catalog_path=catalog_path,
                    catalog_fs_protocol=catalog_fs_protocol,
                    instrument_id=instrument_id,
                ),
                BacktestDataConfig(
                    data_cls=OrderBookDelta.fully_qualified_name(),
                    catalog_path=catalog_path,
                    catalog_fs_protocol=catalog_fs_protocol,
                    instrument_id=instrument_id,
                ),
            ],
            chunk_size=5_000,
        )
        return run_config


class BetfairRequests:
    @staticmethod
    def load(filename, cls=None):
        raw = (RESOURCES_PATH / "requests" / filename).read_bytes()
        return msgspec.json.decode(raw, type=cls) if cls is not None else msgspec.json.decode(raw)

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
    def navigation_list_navigation_request():
        return BetfairRequests.load("navigation_list_navigation.json")


class BetfairResponses:
    @staticmethod
    def load(filename: str) -> None:
        raw = (RESOURCES_PATH / "responses" / filename).read_bytes()
        data = msgspec.json.decode(raw)
        return data

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
    def login_success():
        return BetfairResponses.load("login_success.json")

    @staticmethod
    def login_failure():
        return BetfairResponses.load("login_failure.json")

    @staticmethod
    def list_cleared_orders():
        return BetfairResponses.load("list_cleared_orders.json")

    @staticmethod
    def list_current_orders_executable():
        return BetfairResponses.load("list_current_orders_executable.json")

    @staticmethod
    def list_current_orders_on_close_execution_complete():
        return BetfairResponses.load("list_current_orders_on_close_execution_complete.json")

    @staticmethod
    def list_current_orders_execution_complete():
        return BetfairResponses.load("list_current_orders_execution_complete.json")

    @staticmethod
    def list_current_orders_empty():
        return BetfairResponses.load("list_current_orders_empty.json")

    @staticmethod
    def list_current_orders_custom(
        market_id: str,
        selection_id: int,
        customer_order_ref: str = "",
        customer_strategy_ref: str = "",
    ) -> None:
        raw = BetfairResponses.load("list_current_orders_single.json")
        raw["result"]["currentOrders"][0].update(  # type: ignore
            {
                "marketId": market_id,
                "selectionId": selection_id,
                "customerOrderRef": customer_order_ref,
                "customerStrategyRef": customer_strategy_ref,
            },
        )
        return raw

    @staticmethod
    def list_market_catalogue():
        return BetfairResponses.load("list_market_catalogue.json")

    @staticmethod
    def betting_list_market_catalogue(filter_: MarketFilter | None = None) -> dict:
        result = BetfairResponses.load("betting_list_market_catalogue.json")
        if filter_:
            result = [r for r in result if r["marketId"] in filter_.market_ids]  # type: ignore
        return {"jsonrpc": "2.0", "result": result, "id": 1}

    @staticmethod
    def navigation_list_navigation():
        return BetfairResponses.load("navigation_list_navigation.json")

    @staticmethod
    def market_definition_open():
        return BetfairResponses.load("market_definition_open.json")

    @staticmethod
    def market_definition_closed():
        return BetfairResponses.load("market_definition_closed.json")


class BetfairStreaming:
    @staticmethod
    def decode(raw: bytes, iterate: bool = False):
        if iterate:
            return [stream_decode(msgspec.json.encode(r)) for r in msgspec.json.decode(raw)]
        return stream_decode(raw)

    @staticmethod
    def load(filename, iterate: bool = False) -> bytes | list[bytes]:
        raw = (RESOURCES_PATH / "streaming" / filename).read_bytes()
        message = BetfairStreaming.decode(raw=raw, iterate=iterate)
        if iterate:
            return [msgspec.json.encode(r) for r in message]
        else:
            return msgspec.json.encode(message)

    @staticmethod
    def load_many(filename) -> list[bytes]:
        lines = msgspec.json.decode((RESOURCES_PATH / "streaming" / filename).read_bytes())
        return [msgspec.json.encode(line) for line in lines]

    @staticmethod
    def market_definition():
        return BetfairStreaming.load("streaming_market_definition.json")

    @staticmethod
    def market_definition_runner_removed():
        return BetfairStreaming.load(
            "streaming_market_definition_runner_removed.json",
            iterate=False,
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
    def mcm_BSP() -> list[bytes]:
        return BetfairStreaming.load("streaming_mcm_BSP.json", iterate=True)  # type: ignore

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
    def mcm_market_definition_racing():
        return BetfairStreaming.load("streaming_market_definition_racing.json")

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
        order_id: int = 248485109136,
        client_order_id: str = "",
        mb: list[MatchedOrder] | None = None,
        ml: list[MatchedOrder] | None = None,
    ) -> OCM:
        assert side in ("B", "L"), "`side` should be 'B' or 'L'"
        assert isinstance(order_id, int)
        return OCM(
            id=1,
            clk="1",
            pt=0,
            oc=[
                OrderMarketChange(
                    id="1",
                    orc=[
                        OrderRunnerChange(
                            id=1,
                            uo=[
                                Order(
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
                                    rfo=client_order_id,
                                    rfs="TestStrategy-1.",
                                    avp=avp,
                                ),
                            ],
                            mb=mb or [],
                            ml=ml or [],
                        ),
                    ],
                ),
            ],
        )


class BetfairDataProvider:
    @staticmethod
    def betting_instrument(
        market_id: str = "1-179082386",
        selection_id: str = "50214",
        handicap: str | None = None,
    ) -> BettingInstrument:
        return BettingInstrument(
            venue_name=BETFAIR_VENUE.value,
            betting_type="ODDS",
            competition_id=12282733,
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
            min_notional=Money(1, GBP),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def market_ids():
        """
        Return a list of market_ids used by the tests.

        Used in `navigation_short` and `market_catalogue_short`.

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
    def read_lines(filename: str = "1-166811431.bz2") -> list[bytes]:
        path = TEST_DATA_DIR / "betfair" / filename

        if path.suffix == ".bz2":
            return bz2.open(path).readlines()
        elif path.suffix == ".gz":
            return gzip.open(path).readlines()
        elif path.suffix == ".log":
            return open(path, "rb").readlines()
        else:
            raise ValueError(filename)

    @staticmethod
    def read_mcm(filename: str) -> list[MCM]:
        return [stream_decode(line) for line in BetfairDataProvider.read_lines(filename)]

    @staticmethod
    def market_updates(filename="1-166811431.bz2", runner1="60424", runner2="237478") -> list:
        market_id = pathlib.Path(filename).name
        assert market_id.startswith("1-")

        def _fix_ids(r):
            return (
                r.replace(market_id.encode(), b"1-180737206")
                .replace(runner1.encode(), b"19248890")
                .replace(runner2.encode(), b"38848248")
            )

        return [
            stream_decode(_fix_ids(line.strip()))
            for line in BetfairDataProvider.read_lines(filename)
        ]

    @staticmethod
    def mcm_to_instruments(mcm: MCM, currency="GBP") -> list[BettingInstrument]:
        instruments: list[BettingInstrument] = []
        for mc in mcm.mc:
            if mc.market_definition:
                market_def = msgspec.structs.replace(mc.market_definition, market_id=mc.id)
                instruments.extend(
                    market_definition_to_instruments(
                        market_def,
                        currency,
                        0,
                        0,
                        None,
                    ),
                )
        return instruments

    @staticmethod
    def betfair_feed_parsed(market_id: str = "1-166564490"):
        parser = BetfairParser(currency="GBP")

        instruments: list[BettingInstrument] = []
        data = []
        for mcm in BetfairDataProvider.read_mcm(f"{market_id}.bz2"):
            if not instruments:
                instruments = BetfairDataProvider.mcm_to_instruments(mcm)
                data.extend(instruments)
            data.extend(parser.parse(mcm))

        return data

    @staticmethod
    def badly_formatted_log():
        return open(RESOURCES_PATH / "badly_formatted.txt", "rb").read()


def betting_instrument(
    market_id: MarketId = "1-179082386",
    selection_id: SelectionId = 50214,
    selection_handicap: Handicap | None = None,
) -> BettingInstrument:
    return BettingInstrument(
        venue_name=BETFAIR_VENUE.value,
        betting_type="ODDS",
        competition_id=12282733,
        competition_name="NFL",
        event_country_code="GB",
        event_id=29678534,
        event_name="NFL",
        event_open_date=pd.Timestamp("2022-02-07 23:30:00+00:00"),
        event_type_id=6423,
        event_type_name="American Football",
        market_id=market_id,
        market_name="AFC Conference Winner",
        market_start_time=pd.Timestamp("2022-02-07 23:30:00+00:00"),
        market_type="SPECIAL",
        selection_handicap=selection_handicap or null_handicap(),
        selection_id=selection_id,
        selection_name="Kansas City Chiefs",
        currency="GBP",
        price_precision=BETFAIR_PRICE_PRECISION,
        size_precision=BETFAIR_QUANTITY_PRECISION,
        min_notional=Money(1, GBP),
        tick_scheme_name=BETFAIR_TICK_SCHEME.name,
        ts_event=0,
        ts_init=0,
    )


def betting_instrument_handicap() -> BettingInstrument:
    return BettingInstrument.from_dict(
        {
            "venue_name": "BETFAIR",
            "event_type_id": 61420,
            "event_type_name": "Australian Rules",
            "competition_id": 11897406,
            "competition_name": "AFL",
            "event_id": 30777079,
            "event_name": "GWS v Richmond",
            "event_country_code": "AU",
            "event_open_date": "2021-08-13T09:50:00+00:00",
            "betting_type": "ASIAN_HANDICAP_DOUBLE_LINE",
            "market_id": "1-186249896",
            "market_name": "Handicap",
            "market_start_time": "2021-08-13T09:50:00+00:00",
            "market_type": "HANDICAP",
            "selection_id": 5304641,
            "selection_name": "GWS",
            "selection_handicap": -5.5,
            "currency": "AUD",
            "price_precision": 2,
            "size_precision": 2,
            "ts_event": 0,
            "ts_init": 0,
        },
    )


def load_betfair_data(catalog: ParquetDataCatalog) -> ParquetDataCatalog:
    filename = TEST_DATA_DIR / "betfair" / "1-166564490.bz2"

    # Write betting instruments
    instruments = betting_instruments_from_file(
        filename,
        currency="GBP",
        ts_event=0,
        ts_init=0,
        min_notional=Money(1, GBP),
    )
    catalog.write_data(instruments)

    # Write data
    data = list(parse_betfair_file(filename, currency="GBP"))
    catalog.write_data(data)

    return catalog
