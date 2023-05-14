# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from collections import Counter
from unittest.mock import patch

import msgspec
import pytest
from betfair_parser.core import parse
from betfair_parser.spec.streaming import STREAM_DECODER
from betfair_parser.spec.streaming.mcm import MCM
from betfair_parser.spec.streaming.mcm import BestAvailableToBack
from betfair_parser.spec.streaming.mcm import MarketChange

# fmt: off
from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.common import BETFAIR_TICK_SCHEME
from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDeltas
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.adapters.betfair.orderbook import create_betfair_order_book
from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
from nautilus_trader.adapters.betfair.parsing.requests import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing.requests import determine_order_status
from nautilus_trader.adapters.betfair.parsing.requests import make_order
from nautilus_trader.adapters.betfair.parsing.requests import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing.requests import order_submit_to_betfair
from nautilus_trader.adapters.betfair.parsing.requests import order_update_to_betfair
from nautilus_trader.adapters.betfair.parsing.streaming import market_change_to_updates
from nautilus_trader.adapters.betfair.parsing.streaming import market_definition_to_betfair_starting_prices
from nautilus_trader.adapters.betfair.parsing.streaming import market_definition_to_instrument_closes
from nautilus_trader.adapters.betfair.parsing.streaming import market_definition_to_instrument_status_updates
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.data.book import OrderBookDeltas
from nautilus_trader.model.data.book import OrderBookSnapshot
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.data.venue import InstrumentClose
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import order_side_from_str
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook import L2OrderBook
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


# fmt: on


class TestBetfairParsingStreaming:
    def setup(self):
        self.instrument = TestInstrumentProvider.betting_instrument()
        self.tick_scheme = BETFAIR_TICK_SCHEME

    def test_market_definition_to_instrument_status_updates(self, market_definition_open):
        # Arrange, Act
        updates = market_definition_to_instrument_status_updates(
            market_definition_open,
            "1.205822330",
            0,
            0,
        )

        # Assert
        result = [
            upd
            for upd in updates
            if isinstance(upd, InstrumentStatusUpdate) and upd.status == MarketStatus.PRE_OPEN
        ]
        assert len(result) == 17

    def test_market_definition_to_instrument_close_price(self, market_definition_close):
        # Arrange, Act
        updates = market_definition_to_instrument_closes(
            market_definition_close,
            "1.205822330",
            0,
            0,
        )

        # Assert
        result = [upd for upd in updates if isinstance(upd, InstrumentClose)]
        assert len(result) == 17

    def test_market_definition_to_betfair_starting_price(self, market_definition_close):
        # Arrange, Act
        updates = market_definition_to_betfair_starting_prices(
            market_definition_close,
            "1.205822330",
            0,
            0,
        )

        # Assert
        result = [upd for upd in updates if isinstance(upd, BetfairStartingPrice)]
        assert len(result) == 14

    def test_market_change_bsp_updates(self):
        raw = b'{"id":"1.205822330","rc":[{"spb":[[1000,32.21]],"id":45368013},{"spb":[[1000,20.5]],"id":49808343},{"atb":[[1.93,10.09]],"id":49808342},{"spb":[[1000,20.5]],"id":39000334},{"spb":[[1000,84.22]],"id":16206031},{"spb":[[1000,18]],"id":10591436},{"spb":[[1000,88.96]],"id":48672282},{"spb":[[1000,18]],"id":19143530},{"spb":[[1000,20.5]],"id":6159479},{"spb":[[1000,10]],"id":25694777},{"spb":[[1000,10]],"id":49808335},{"spb":[[1000,10]],"id":49808334},{"spb":[[1000,20.5]],"id":35672106}],"con":true,"img":false}'  # noqa
        mc = msgspec.json.decode(raw, type=MarketChange)
        result = Counter([upd.__class__.__name__ for upd in market_change_to_updates(mc, 0, 0)])
        expected = Counter({"BSPOrderBookDeltas": 12, "OrderBookDeltas": 1})
        assert result == expected

    def test_market_change_ticker(self):
        raw = b'{"id":"1.205822330","rc":[{"atl":[[1.98,0],[1.91,30.38]],"id":49808338},{"atb":[[3.95,2.98]],"id":49808334},{"trd":[[3.95,46.95]],"ltp":3.95,"tv":46.95,"id":49808334}],"con":true,"img":false}'  # noqa
        mc = msgspec.json.decode(raw, type=MarketChange)
        result = market_change_to_updates(mc, 0, 0)
        assert result[0] == TradeTick.from_dict(
            {
                "type": "TradeTick",
                "instrument_id": "1.205822330|49808334|0.0.BETFAIR",
                "price": "3.95",
                "size": "46.950000",
                "aggressor_side": "NO_AGGRESSOR",
                "trade_id": "3.9546.95",
                "ts_event": 0,
                "ts_init": 0,
            },
        )
        assert result[1] == BetfairTicker.from_dict(
            {
                "type": "BetfairTicker",
                "instrument_id": "1.205822330|49808334|0.0.BETFAIR",
                "ts_event": 0,
                "ts_init": 0,
                "last_traded_price": 0.2531646,
                "traded_volume": 46.95,
                "starting_price_near": None,
                "starting_price_far": None,
            },
        )
        assert isinstance(result[2], OrderBookDeltas)
        assert isinstance(result[3], OrderBookDeltas)

    @pytest.mark.parametrize(
        ("filename", "num_msgs"),
        [
            ("1.166564490.bz2", 2533),
            ("1.166811431.bz2", 17846),
            ("1.180305278.bz2", 15734),
            ("1.206064380.bz2", 50149),
        ],
    )
    def test_parsing_streaming_file(self, filename, num_msgs):
        mcms = BetfairDataProvider.market_updates(filename)
        parser = BetfairParser()
        updates = [x for mcm in mcms for x in parser.parse(mcm)]
        assert len(updates) == num_msgs

    def test_parsing_streaming_file_message_counts(self):
        mcms = BetfairDataProvider.read_mcm("1.206064380.bz2")
        parser = BetfairParser()
        updates = Counter([x.__class__.__name__ for mcm in mcms for x in parser.parse(mcm)])
        expected = Counter(
            {
                "OrderBookDeltas": 40253,
                "BetfairTicker": 4658,
                "TradeTick": 3590,
                "BSPOrderBookDeltas": 1139,
                "OrderBookSnapshot": 152,
                "InstrumentStatusUpdate": 260,
                "BetfairStartingPrice": 72,
                "InstrumentClose": 25,
            },
        )
        assert updates == expected

    @pytest.mark.parametrize(
        ("filename", "book_count"),
        [
            ("1.166564490.bz2", [1854, 2161]),
            ("1.166811431.bz2", [12318, 12181]),
            ("1.180305278.bz2", [2414, 11960]),
            (
                "1.206064380.bz2",
                [10909, 5364, 11256, 9407, 406, 6771, 7056, 5700, 9463, 6707, 18776, 8767, 5392],
            ),
        ],
    )
    def test_order_book_integrity(self, filename, book_count) -> None:
        mcms = BetfairDataProvider.market_updates(filename)
        parser = BetfairParser()

        books: dict[InstrumentId, L2OrderBook] = {}
        for update in [x for mcm in mcms for x in parser.parse(mcm)]:
            if isinstance(update, (OrderBookDeltas, OrderBookSnapshot)) and not isinstance(
                update,
                BSPOrderBookDeltas,
            ):
                instrument_id = update.instrument_id
                if instrument_id not in books:
                    instrument = TestInstrumentProvider.betting_instrument(
                        *instrument_id.value.split("|")
                    )
                    books[instrument_id] = create_betfair_order_book(instrument.id)
                books[instrument_id].apply(update)
                books[instrument_id].check_integrity()
        result = [book.count for book in books.values()]
        assert result == book_count


class TestBetfairParsing:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.new_event_loop()
        self.clock = LiveClock()
        self.logger = Logger(clock=self.clock, bypass=True)
        self.instrument = TestInstrumentProvider.betting_instrument()
        self.client = BetfairTestStubs.betfair_client(loop=self.loop, logger=self.logger)
        self.provider = BetfairTestStubs.instrument_provider(self.client)
        self.uuid = UUID4()

    def test_order_submit_to_betfair(self):
        command = TestCommandStubs.submit_order_command(
            order=TestExecStubs.limit_order(
                price=betfair_float_to_price(2.5),
                quantity=betfair_float_to_quantity(10),
            ),
        )
        result = order_submit_to_betfair(command=command, instrument=self.instrument)
        expected = {
            "customer_ref": command.id.value.replace("-", ""),
            "customer_strategy_ref": "S-001",
            "instructions": [
                {
                    "customerOrderRef": "O-20210410-022422-001",
                    "handicap": None,
                    "limitOrder": {
                        "persistenceType": "PERSIST",
                        "price": 2.5,
                        "size": 10.0,
                    },
                    "orderType": "LIMIT",
                    "selectionId": "50214",
                    "side": "BACK",
                },
            ],
            "market_id": "1.179082386",
        }
        assert result == expected

    def test_order_update_to_betfair(self):
        modify = TestCommandStubs.modify_order_command(
            instrument_id=self.instrument.id,
            client_order_id=ClientOrderId("C-1"),
            quantity=betfair_float_to_quantity(10),
            price=betfair_float_to_price(1.35),
        )

        result = order_update_to_betfair(
            command=modify,
            side=OrderSide.BUY,
            venue_order_id=VenueOrderId("1"),
            instrument=self.instrument,
        )
        expected = {
            "market_id": "1.179082386",
            "customer_ref": result["customer_ref"],
            "instructions": [{"betId": "1", "newPrice": 1.35}],
        }

        assert result == expected

    def test_order_cancel_to_betfair(self):
        result = order_cancel_to_betfair(
            command=TestCommandStubs.cancel_order_command(
                venue_order_id=VenueOrderId("228302937743"),
            ),
            instrument=self.instrument,
        )
        expected = {
            "market_id": "1.179082386",
            "customer_ref": result["customer_ref"],
            "instructions": [
                {
                    "betId": "228302937743",
                },
            ],
        }
        assert result == expected

    @pytest.mark.asyncio()
    async def test_account_statement(self):
        with patch.object(
            BetfairClient,
            "request",
            return_value=BetfairResponses.account_details(),
        ):
            detail = await self.client.get_account_details()
        with patch.object(
            BetfairClient,
            "request",
            return_value=BetfairResponses.account_funds_no_exposure(),
        ):
            funds = await self.client.get_account_funds()
        result = betfair_account_to_account_state(
            account_detail=detail,
            account_funds=funds,
            event_id=self.uuid,
            ts_event=0,
            ts_init=0,
        )
        expected = AccountState(
            account_id=AccountId("BETFAIR-Testy-McTest"),
            account_type=AccountType.CASH,
            base_currency=GBP,
            reported=True,  # reported
            balances=[
                AccountBalance(
                    Money(1000.0, GBP),
                    Money(0.00, GBP),
                    Money(1000.0, GBP),
                ),
            ],
            margins=[],
            info={"funds": funds, "detail": detail},
            event_id=self.uuid,
            ts_event=result.ts_event,
            ts_init=result.ts_init,
        )
        assert result == expected

    @pytest.mark.asyncio()
    async def test_merge_order_book_deltas(self):
        raw = msgspec.json.encode(
            {
                "op": "mcm",
                "clk": "792361654",
                "pt": 1577575379148,
                "mc": [
                    {
                        "id": "1.180759290",
                        "rc": [
                            {"atl": [[3.15, 3.68]], "id": 7659748},
                            {"trd": [[3.15, 364.45]], "ltp": 3.15, "tv": 364.45, "id": 7659748},
                            {"atb": [[3.15, 0]], "id": 7659748},
                        ],
                        "con": True,
                        "img": False,
                    },
                ],
                "id": 1,
            },
        )
        mcm = msgspec.json.decode(raw, type=MCM)
        parser = BetfairParser()
        updates = parser.parse(mcm)
        assert len(updates) == 3
        trade, ticker, deltas = updates
        assert isinstance(trade, TradeTick)
        assert isinstance(ticker, Ticker)
        assert isinstance(deltas, OrderBookDeltas)
        assert len(deltas.deltas) == 2

    def test_make_order_limit(self):
        order = TestExecStubs.limit_order(
            price=betfair_float_to_price(3.05),
            quantity=betfair_float_to_quantity(10),
        )
        result = make_order(order)
        expected = {
            "limitOrder": {"persistenceType": "PERSIST", "price": 3.05, "size": 10.0},
            "orderType": "LIMIT",
        }
        assert result == expected

    def test_make_order_limit_on_close(self):
        order = TestExecStubs.limit_order(
            price=betfair_float_to_price(3.05),
            quantity=betfair_float_to_quantity(10),
            instrument_id=TestIdStubs.betting_instrument_id(),
            time_in_force=TimeInForce.AT_THE_OPEN,
        )
        result = make_order(order)
        expected = {
            "limitOnCloseOrder": {"price": 3.05, "liability": 10.0},
            "orderType": "LIMIT_ON_CLOSE",
        }
        assert result == expected

    def test_make_order_market_buy(self):
        order = TestExecStubs.market_order(order_side=OrderSide.BUY)
        result = make_order(order)
        expected = {
            "limitOrder": {
                "persistenceType": "LAPSE",
                "price": 1.01,
                "size": 100.0,
                "timeInForce": "FILL_OR_KILL",
            },
            "orderType": "LIMIT",
        }
        assert result == expected

    def test_make_order_market_sell(self):
        order = TestExecStubs.market_order(order_side=OrderSide.SELL)
        result = make_order(order)
        expected = {
            "limitOrder": {
                "persistenceType": "LAPSE",
                "price": 1000.0,
                "size": 100.0,
                "timeInForce": "FILL_OR_KILL",
            },
            "orderType": "LIMIT",
        }
        assert result == expected

    @pytest.mark.parametrize(
        ("side", "liability"),
        [("BUY", "100.0"), ("SELL", "100.0")],
    )
    def test_make_order_market_on_close(self, side, liability):
        order = TestExecStubs.market_order(
            time_in_force=TimeInForce.AT_THE_OPEN,
            order_side=order_side_from_str(side),
        )
        result = make_order(order)
        expected = {
            "marketOnCloseOrder": {"liability": liability},
            "orderType": "MARKET_ON_CLOSE",
        }
        assert result == expected

    @pytest.mark.parametrize(
        ("status", "size", "matched", "cancelled", "expected"),
        [
            ("EXECUTION_COMPLETE", 10.0, 10.0, 0.0, OrderStatus.FILLED),
            ("EXECUTION_COMPLETE", 10.0, 5.0, 5.0, OrderStatus.CANCELED),
            ("EXECUTABLE", 10.0, 0.0, 0.0, OrderStatus.ACCEPTED),
            ("EXECUTABLE", 10.0, 5.0, 0.0, OrderStatus.PARTIALLY_FILLED),
        ],
    )
    def test_determine_order_status(self, status, size, matched, cancelled, expected):
        order = {
            "betId": "257272569678",
            "priceSize": {"price": 3.4, "size": size},
            "status": status,
            "averagePriceMatched": 3.4211,
            "sizeMatched": matched,
            "sizeRemaining": size - matched - cancelled,
            "sizeLapsed": 0.0,
            "sizeCancelled": cancelled,
            "sizeVoided": 0.0,
        }
        status = determine_order_status(order=order)
        assert status == expected

    def test_parse_line(self):
        lines = [
            b'{"op":"connection","connectionId":"105-280621060315-3705817"}',
            b'{"op":"status","id":1,"statusCode":"SUCCESS","connectionClosed":false,"connectionsAvailable":5}',
            b'{"op":"status","id":1,"statusCode":"SUCCESS","connectionClosed":false}',
            b'{"op":"mcm","id":1,"initialClk":"nhy58bfvDawc+Jbf/A2jHKee5vUN","clk":"AAAAAAAA","conflateMs":0,"heartbeatMs":5000,"pt":1624860195431,"ct":"SUB_IMAGE","mc":[{"id":"1.184839563","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30633417","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-06-29T01:10:00.000Z","suspendTime":"2021-06-29T01:10:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"OPEN","runners":[{"status":"ACTIVE","sortPriority":1,"id":6023845},{"status":"ACTIVE","sortPriority":2,"id":237487}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-06-29T01:10:00.000Z","version":3888693695,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[1.46,59.86],[1.48,1419.67],[1.47,2.92],[1.01,971.95],[1.02,119.11],[1.21,103],[1.42,27.32]],"atl":[[2,68.67],[1000,1.72],[200,1.72]],"trd":[[1.53,27.93],[1.46,407.17],[1.41,5.15],[1.48,29.85],[1.52,53.15],[1.47,10.38],[1.49,10],[1.5,22.58],[1.4,5.76]],"batb":[[2,1.46,59.86],[0,1.48,1419.67],[1,1.47,2.92],[6,1.01,971.95],[5,1.02,119.11],[4,1.21,103],[3,1.42,27.32]],"batl":[[0,2,68.67],[2,1000,1.72],[1,200,1.72]],"tv":571.97,"id":237487},{"atb":[[2.8,1.54],[1.01,971.95],[1.02,119.11],[2,68.67],[2.82,1440.67],[2.88,14.22],[1.43,2.73]],"atl":[[9.8,25.75],[1000,1.72],[200,1.72],[3.6,2.54]],"trd":[[2.9,13.06],[2.92,2.95],[3.1,138.82],[2.88,32.33],[3.2,77.73],[2.94,27.48],[3,34.24],[3.15,2.94]],"batb":[[6,1.01,971.95],[5,1.02,119.11],[4,1.43,2.73],[3,2,68.67],[2,2.8,1.54],[1,2.82,1440.67],[0,2.88,14.22]],"batl":[[3,1000,1.72],[2,200,1.72],[1,9.8,25.75],[0,3.6,2.54]],"tv":329.55,"id":6023845}],"img":true,"tv":901.52},{"id":"1.183516561","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30533301","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-05-19T01:16:00.000Z","suspendTime":"2021-05-19T01:16:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"SUSPENDED","runners":[{"status":"ACTIVE","sortPriority":1,"id":237485},{"status":"ACTIVE","sortPriority":2,"id":60427}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-05-19T01:16:00.000Z","version":3824150209,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[2.2,238.14],[2.22,451.53],[2.1,20.7],[2.24,462.2],[2.18,8.89],[1.4,2],[1.65,86.15],[2.16,11.6],[1.01,746.03],[2.08,56.26],[1.05,91.09],[1.1,86.15],[1.3,3.84],[1.02,86.15],[1.03,86.15],[1.86,17.5]],"atl":[[2.32,11.53],[2.3,140.58],[2.28,201.16],[2.36,21.14],[1000,1.72],[200,1.72]],"trd":[[2.26,908.83],[2.24,2262.18],[2.28,1206.46],[2.22,5340.65],[2.16,2461.4],[2.2,2042.06],[2.18,1704.71],[2.08,74.11],[2.14,1098.39],[2.1,1413.03],[2.12,62.51],[2.04,7.37],[2.32,41.98],[2.3,554.84],[2,54.31],[2.36,20.68],[2.06,2045.77],[1.98,0.63]],"batb":[[2,2.2,238.14],[1,2.22,451.53],[5,2.1,20.7],[0,2.24,462.2],[3,2.18,8.89],[9,1.4,2],[8,1.65,86.15],[7,1.86,17.5],[6,2.08,56.26],[4,2.16,11.6]],"batl":[[2,2.32,11.53],[5,1000,1.72],[4,200,1.72],[3,2.36,21.14],[1,2.3,140.58],[0,2.28,201.16]],"tv":21299.91,"id":237485},{"atb":[[1.78,210.83],[1.75,14.41],[1.76,28.4],[1.79,450.18],[1.77,14.42],[1.01,746.03],[1.65,86.15],[1.05,91.09],[1.1,86.15],[1.3,3.84],[1.02,86.15],[1.03,86.15]],"atl":[[1.81,430.16],[1.82,488.33],[1.85,11.31],[1.83,14.32],[1.84,14.28],[3.1,27.45],[1000,1.72],[200,1.72],[2.08,1.72]],"trd":[[1.8,6609.88],[1.79,2742.92],[1.81,2879.6],[1.82,1567.46],[1.77,964.99],[1.86,272.44],[1.91,96.58],[1.99,16.47],[1.92,220.37],[1.76,11.91],[1.87,362.25],[1.78,437.4],[1.85,415.17],[1.84,580.74],[1.83,1394.8],[1.73,4],[1.88,22.37],[1.95,9.49],[1.96,1.96],[1.89,45.75],[1.9,2.3],[2.02,0.61],[1.93,4.71]],"batb":[[1,1.78,210.83],[4,1.75,14.41],[3,1.76,28.4],[0,1.79,450.18],[9,1.03,86.15],[8,1.05,91.09],[7,1.1,86.15],[6,1.3,3.84],[5,1.65,86.15],[2,1.77,14.42]],"batl":[[0,1.81,430.16],[8,1000,1.72],[7,200,1.72],[6,3.1,27.45],[5,2.08,1.72],[4,1.85,11.31],[3,1.84,14.28],[2,1.83,14.32],[1,1.82,488.33]],"tv":18664.17,"id":60427}],"img":true,"tv":39964.08},{"id":"1.184866117","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30635089","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-06-30T00:40:00.000Z","suspendTime":"2021-06-30T00:40:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"OPEN","runners":[{"status":"ACTIVE","sortPriority":1,"id":237477},{"status":"ACTIVE","sortPriority":2,"id":237490}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-06-30T00:40:00.000Z","version":3890540057,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[1.03,1.93],[1.02,76.24],[1.01,108.66],[1.39,68.58]],"atl":[[1.49,1.93]],"trd":[[1.39,52.64]],"batb":[[3,1.01,108.66],[2,1.02,76.24],[1,1.03,1.93],[0,1.39,68.58]],"batl":[[0,1.49,1.93]],"tv":52.64,"id":237477},{"atb":[[3.05,1.93],[1.02,76.24],[1.01,108.66],[3,13.37]],"atl":[[3.55,1.93]],"batb":[[3,1.01,108.66],[2,1.02,76.24],[1,3,13.37],[0,3.05,1.93]],"batl":[[0,3.55,1.93]],"id":237490}],"img":true,"tv":52.64}]}',  # noqa
            b'{"op":"mcm","id":1,"clk":"AKgBAIgBANgB","pt":1624860200431,"ct":"HEARTBEAT"}',
        ]
        for line in lines:
            data = STREAM_DECODER.decode(line)
            assert data

    def test_mcm(self) -> None:
        line = b'{"op":"mcm","id":1,"initialClk":"nhy58bfvDawc+Jbf/A2jHKee5vUN","clk":"AAAAAAAA","conflateMs":0,"heartbeatMs":5000,"pt":1624860195431,"ct":"SUB_IMAGE","mc":[{"id":"1.184839563","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30633417","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-06-29T01:10:00.000Z","suspendTime":"2021-06-29T01:10:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"OPEN","runners":[{"status":"ACTIVE","sortPriority":1,"id":6023845},{"status":"ACTIVE","sortPriority":2,"id":237487}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-06-29T01:10:00.000Z","version":3888693695,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[1.46,59.86],[1.48,1419.67],[1.47,2.92],[1.01,971.95],[1.02,119.11],[1.21,103],[1.42,27.32]],"atl":[[2,68.67],[1000,1.72],[200,1.72]],"trd":[[1.53,27.93],[1.46,407.17],[1.41,5.15],[1.48,29.85],[1.52,53.15],[1.47,10.38],[1.49,10],[1.5,22.58],[1.4,5.76]],"batb":[[2,1.46,59.86],[0,1.48,1419.67],[1,1.47,2.92],[6,1.01,971.95],[5,1.02,119.11],[4,1.21,103],[3,1.42,27.32]],"batl":[[0,2,68.67],[2,1000,1.72],[1,200,1.72]],"tv":571.97,"id":237487},{"atb":[[2.8,1.54],[1.01,971.95],[1.02,119.11],[2,68.67],[2.82,1440.67],[2.88,14.22],[1.43,2.73]],"atl":[[9.8,25.75],[1000,1.72],[200,1.72],[3.6,2.54]],"trd":[[2.9,13.06],[2.92,2.95],[3.1,138.82],[2.88,32.33],[3.2,77.73],[2.94,27.48],[3,34.24],[3.15,2.94]],"batb":[[6,1.01,971.95],[5,1.02,119.11],[4,1.43,2.73],[3,2,68.67],[2,2.8,1.54],[1,2.82,1440.67],[0,2.88,14.22]],"batl":[[3,1000,1.72],[2,200,1.72],[1,9.8,25.75],[0,3.6,2.54]],"tv":329.55,"id":6023845}],"img":true,"tv":901.52},{"id":"1.183516561","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30533301","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-05-19T01:16:00.000Z","suspendTime":"2021-05-19T01:16:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"SUSPENDED","runners":[{"status":"ACTIVE","sortPriority":1,"id":237485},{"status":"ACTIVE","sortPriority":2,"id":60427}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-05-19T01:16:00.000Z","version":3824150209,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[2.2,238.14],[2.22,451.53],[2.1,20.7],[2.24,462.2],[2.18,8.89],[1.4,2],[1.65,86.15],[2.16,11.6],[1.01,746.03],[2.08,56.26],[1.05,91.09],[1.1,86.15],[1.3,3.84],[1.02,86.15],[1.03,86.15],[1.86,17.5]],"atl":[[2.32,11.53],[2.3,140.58],[2.28,201.16],[2.36,21.14],[1000,1.72],[200,1.72]],"trd":[[2.26,908.83],[2.24,2262.18],[2.28,1206.46],[2.22,5340.65],[2.16,2461.4],[2.2,2042.06],[2.18,1704.71],[2.08,74.11],[2.14,1098.39],[2.1,1413.03],[2.12,62.51],[2.04,7.37],[2.32,41.98],[2.3,554.84],[2,54.31],[2.36,20.68],[2.06,2045.77],[1.98,0.63]],"batb":[[2,2.2,238.14],[1,2.22,451.53],[5,2.1,20.7],[0,2.24,462.2],[3,2.18,8.89],[9,1.4,2],[8,1.65,86.15],[7,1.86,17.5],[6,2.08,56.26],[4,2.16,11.6]],"batl":[[2,2.32,11.53],[5,1000,1.72],[4,200,1.72],[3,2.36,21.14],[1,2.3,140.58],[0,2.28,201.16]],"tv":21299.91,"id":237485},{"atb":[[1.78,210.83],[1.75,14.41],[1.76,28.4],[1.79,450.18],[1.77,14.42],[1.01,746.03],[1.65,86.15],[1.05,91.09],[1.1,86.15],[1.3,3.84],[1.02,86.15],[1.03,86.15]],"atl":[[1.81,430.16],[1.82,488.33],[1.85,11.31],[1.83,14.32],[1.84,14.28],[3.1,27.45],[1000,1.72],[200,1.72],[2.08,1.72]],"trd":[[1.8,6609.88],[1.79,2742.92],[1.81,2879.6],[1.82,1567.46],[1.77,964.99],[1.86,272.44],[1.91,96.58],[1.99,16.47],[1.92,220.37],[1.76,11.91],[1.87,362.25],[1.78,437.4],[1.85,415.17],[1.84,580.74],[1.83,1394.8],[1.73,4],[1.88,22.37],[1.95,9.49],[1.96,1.96],[1.89,45.75],[1.9,2.3],[2.02,0.61],[1.93,4.71]],"batb":[[1,1.78,210.83],[4,1.75,14.41],[3,1.76,28.4],[0,1.79,450.18],[9,1.03,86.15],[8,1.05,91.09],[7,1.1,86.15],[6,1.3,3.84],[5,1.65,86.15],[2,1.77,14.42]],"batl":[[0,1.81,430.16],[8,1000,1.72],[7,200,1.72],[6,3.1,27.45],[5,2.08,1.72],[4,1.85,11.31],[3,1.84,14.28],[2,1.83,14.32],[1,1.82,488.33]],"tv":18664.17,"id":60427}],"img":true,"tv":39964.08},{"id":"1.184866117","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30635089","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-06-30T00:40:00.000Z","suspendTime":"2021-06-30T00:40:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"OPEN","runners":[{"status":"ACTIVE","sortPriority":1,"id":237477},{"status":"ACTIVE","sortPriority":2,"id":237490}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-06-30T00:40:00.000Z","version":3890540057,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[1.03,1.93],[1.02,76.24],[1.01,108.66],[1.39,68.58]],"atl":[[1.49,1.93]],"trd":[[1.39,52.64]],"batb":[[3,1.01,108.66],[2,1.02,76.24],[1,1.03,1.93],[0,1.39,68.58]],"batl":[[0,1.49,1.93]],"tv":52.64,"id":237477},{"atb":[[3.05,1.93],[1.02,76.24],[1.01,108.66],[3,13.37]],"atl":[[3.55,1.93]],"batb":[[3,1.01,108.66],[2,1.02,76.24],[1,3,13.37],[0,3.05,1.93]],"batl":[[0,3.55,1.93]],"id":237490}],"img":true,"tv":52.64}]}'  # noqa
        mcm: MCM = STREAM_DECODER.decode(line)
        expected = [
            BestAvailableToBack(level=2, price=1.46, volume=59.86),
            BestAvailableToBack(level=0, price=1.48, volume=1419.67),
            BestAvailableToBack(level=1, price=1.47, volume=2.92),
            BestAvailableToBack(level=6, price=1.01, volume=971.95),
            BestAvailableToBack(level=5, price=1.02, volume=119.11),
            BestAvailableToBack(level=4, price=1.21, volume=103),
            BestAvailableToBack(level=3, price=1.42, volume=27.32),
        ]
        assert mcm.mc[0].rc[0].batb == expected

    def test_mcm_bsp_example1(self):
        parser = BetfairParser()
        r = b'{"op":"mcm","id":1,"clk":"ANjxBACiiQQAlpQD","pt":1672131753550,"mc":[{"id":"1.208011084","marketDefinition":{"bspMarket":true,"turnInPlayEnabled":false,"persistenceEnabled":false,"marketBaseRate":7,"eventId":"31987078","eventTypeId":"4339","numberOfWinners":1,"bettingType":"ODDS","marketType":"WIN","marketTime":"2022-12-27T09:00:00.000Z","suspendTime":"2022-12-27T09:00:00.000Z","bspReconciled":true,"complete":true,"inPlay":false,"crossMatching":false,"runnersVoidable":false,"numberOfActiveRunners":0,"betDelay":0,"status":"CLOSED","settledTime":"2022-12-27T09:02:21.000Z","runners":[{"status":"WINNER","sortPriority":1,"bsp":2.0008034621107256,"id":45967562},{"status":"LOSER","sortPriority":2,"bsp":5.5,"id":45565847},{"status":"LOSER","sortPriority":3,"bsp":9.2,"id":47727833},{"status":"LOSER","sortPriority":4,"bsp":166.61668896346615,"id":47179469},{"status":"LOSER","sortPriority":5,"bsp":44,"id":51247493},{"status":"LOSER","sortPriority":6,"bsp":32,"id":42324350},{"status":"LOSER","sortPriority":7,"bsp":7.4,"id":51247494},{"status":"LOSER","sortPriority":8,"bsp":32.28604557164013,"id":48516342}],"regulators":["MR_INT"],"venue":"Warragul","countryCode":"AU","discountAllowed":true,"timezone":"Australia/Sydney","openDate":"2022-12-27T07:46:00.000Z","version":4968605121,"priceLadderDefinition":{"type":"CLASSIC"}}}]}'  # noqa
        mcm = parse(r)
        updates = parser.parse(mcm)
        starting_prices = [upd for upd in updates if isinstance(upd, BetfairStartingPrice)]
        assert len(starting_prices) == 8
        assert starting_prices[0].instrument_id == InstrumentId.from_str(
            "1.208011084|45967562|0.0-BSP.BETFAIR",
        )
        assert starting_prices[0].bsp == 2.0008034621107256

    def test_mcm_bsp_example2(self):
        raw = b'{"op":"mcm","clk":"7066946780","pt":1667288437853,"mc":[{"id":"1.205880280","rc":[{"spl":[[1.01,2]],"id":49892033},{"atl":[[2.8,0],[2.78,0]],"id":49892032},{"atb":[[2.8,378.82]],"id":49892032},{"trd":[[2.8,1.16],[2.78,1.18]],"ltp":2.8,"tv":2.34,"id":49892032},{"spl":[[1.01,4.79]],"id":49892030},{"spl":[[1.01,2]],"id":49892029},{"spl":[[1.01,3.79]],"id":49892028},{"spl":[[1.01,2]],"id":49892027},{"spl":[[1.01,2]],"id":49892034}],"con":true,"img":false}]}'  # noqa
        parser = BetfairParser()
        mcm = parse(raw)
        updates = parser.parse(mcm)
        single_instrument_bsp_updates = [
            upd
            for upd in updates
            if isinstance(upd, BSPOrderBookDeltas)
            and upd.instrument_id == InstrumentId.from_str("1.205880280|49892033|0.0-BSP.BETFAIR")
        ]
        assert len(single_instrument_bsp_updates) == 1
