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

import datetime
from collections import Counter
from collections import defaultdict
from copy import copy

import msgspec
import pytest
from betfair_parser.spec.betting.enums import PersistenceType
from betfair_parser.spec.betting.enums import Side
from betfair_parser.spec.betting.orders import CancelOrders
from betfair_parser.spec.betting.orders import PlaceOrders
from betfair_parser.spec.betting.orders import ReplaceOrders
from betfair_parser.spec.betting.type_definitions import CancelInstruction
from betfair_parser.spec.betting.type_definitions import CurrentOrderSummary
from betfair_parser.spec.betting.type_definitions import LimitOnCloseOrder
from betfair_parser.spec.betting.type_definitions import LimitOrder
from betfair_parser.spec.betting.type_definitions import MarketOnCloseOrder
from betfair_parser.spec.betting.type_definitions import PlaceInstruction
from betfair_parser.spec.betting.type_definitions import PriceSize
from betfair_parser.spec.betting.type_definitions import ReplaceInstruction
from betfair_parser.spec.betting.type_definitions import TimeInForce as BP_TimeInForce
from betfair_parser.spec.common import OrderStatus
from betfair_parser.spec.common import OrderType
from betfair_parser.spec.common import decode
from betfair_parser.spec.common import encode
from betfair_parser.spec.common.messages import _default_id_generator
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import BestAvailableToBack
from betfair_parser.spec.streaming import MarketChange
from betfair_parser.spec.streaming import MarketDefinition
from betfair_parser.spec.streaming import stream_decode

# fmt: off
from nautilus_trader.adapters.betfair.common import BETFAIR_TICK_SCHEME
from nautilus_trader.adapters.betfair.common import OrderSideParser
from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.adapters.betfair.orderbook import create_betfair_order_book
from nautilus_trader.adapters.betfair.parsing.common import instrument_id_betfair_ids
from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
from nautilus_trader.adapters.betfair.parsing.requests import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing.requests import determine_order_status
from nautilus_trader.adapters.betfair.parsing.requests import make_customer_order_ref
from nautilus_trader.adapters.betfair.parsing.requests import nautilus_limit_on_close_to_place_instructions
from nautilus_trader.adapters.betfair.parsing.requests import nautilus_limit_to_place_instructions
from nautilus_trader.adapters.betfair.parsing.requests import nautilus_market_on_close_to_place_instructions
from nautilus_trader.adapters.betfair.parsing.requests import nautilus_market_to_place_instructions
from nautilus_trader.adapters.betfair.parsing.requests import nautilus_order_to_place_instructions
from nautilus_trader.adapters.betfair.parsing.requests import order_cancel_to_cancel_order_params
from nautilus_trader.adapters.betfair.parsing.requests import order_submit_to_place_order_params
from nautilus_trader.adapters.betfair.parsing.requests import order_update_to_replace_order_params
from nautilus_trader.adapters.betfair.parsing.streaming import market_change_to_updates
from nautilus_trader.adapters.betfair.parsing.streaming import market_definition_to_betfair_starting_prices
from nautilus_trader.adapters.betfair.parsing.streaming import market_definition_to_instrument_closes
from nautilus_trader.adapters.betfair.parsing.streaming import market_definition_to_instrument_status
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.data import Data
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus as NautilusOrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import order_side_from_str
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.integration_tests.adapters.betfair.test_kit import betting_instrument
from tests.integration_tests.adapters.betfair.test_kit import mock_betfair_request


# fmt: on


class TestBetfairParsingStreaming:
    def setup(self):
        self.instrument = betting_instrument()
        self.tick_scheme = BETFAIR_TICK_SCHEME
        self.parser = BetfairParser(currency="GBP")

    def test_market_definition_to_instrument_status(self):
        # Arrange
        market_definition_open = decode(
            encode(BetfairResponses.market_definition_open()),
            type=MarketDefinition,
        )

        # Act
        updates = market_definition_to_instrument_status(
            market_definition_open,
            "1.205822330",
            0,
            0,
        )

        # Assert
        result = [
            upd
            for upd in updates
            if isinstance(upd, InstrumentStatus) and upd.action == MarketStatusAction.PRE_OPEN
        ]
        assert len(result) == 17

    def test_market_definition_to_instrument_close_price(self):
        # Arrange
        market_definition_close = decode(
            encode(BetfairResponses.market_definition_closed()),
            type=MarketDefinition,
        )

        # Act
        updates = market_definition_to_instrument_closes(
            market_definition_close,
            "1.205822330",
            0,
            0,
        )

        # Assert
        result = [upd for upd in updates if isinstance(upd, InstrumentClose)]
        assert len(result) == 17

    def test_market_definition_to_betfair_starting_price(self):
        # Arrange
        market_definition_close = decode(
            encode(BetfairResponses.market_definition_closed()),
            type=MarketDefinition,
        )

        updates = market_definition_to_betfair_starting_prices(
            market_definition_close,
            "1.205822330",
            0,
            0,
        )

        # Assert
        result = [
            upd
            for upd in updates
            if isinstance(upd, CustomData) and upd.data_type.type == BetfairStartingPrice
        ]
        assert len(result) == 14

    def test_market_definition_to_instrument_updates(self):
        # Arrange
        raw = BetfairStreaming.mcm_market_definition_racing()
        mcm = msgspec.json.decode(raw, type=MCM)

        # Act
        updates = self.parser.parse(mcm)

        # Assert
        counts = Counter([update.__class__.__name__ for update in updates])
        expected = Counter(
            {
                "InstrumentStatus": 7,
                "OrderBookDeltas": 7,
                "BettingInstrument": 7,
                "CustomData": 1,
            },
        )
        assert counts == expected

    def test_market_change_bsp_updates(self):
        raw = b'{"id":"1.205822330","rc":[{"spb":[[1000,32.21]],"id":45368013},{"spb":[[1000,20.5]],"id":49808343},{"atb":[[1.93,10.09]],"id":49808342},{"spb":[[1000,20.5]],"id":39000334},{"spb":[[1000,84.22]],"id":16206031},{"spb":[[1000,18]],"id":10591436},{"spb":[[1000,88.96]],"id":48672282},{"spb":[[1000,18]],"id":19143530},{"spb":[[1000,20.5]],"id":6159479},{"spb":[[1000,10]],"id":25694777},{"spb":[[1000,10]],"id":49808335},{"spb":[[1000,10]],"id":49808334},{"spb":[[1000,20.5]],"id":35672106}],"con":true,"img":false}'  # noqa
        mc = msgspec.json.decode(raw, type=MarketChange)
        result = Counter([upd.__class__.__name__ for upd in market_change_to_updates(mc, {}, 0, 0)])
        expected = Counter({"CustomData": 12, "OrderBookDeltas": 1})
        assert result == expected

    def test_market_change_ticker(self):
        raw = b'{"id":"1.205822330","rc":[{"atl":[[1.98,0],[1.91,30.38]],"id":49808338},{"atb":[[3.95,2.98]],"id":49808334},{"trd":[[3.95,46.95]],"ltp":3.95,"tv":46.95,"id":49808334}],"con":true,"img":false}'  # noqa
        mc = msgspec.json.decode(raw, type=MarketChange)
        result = market_change_to_updates(mc, {}, 0, 0)
        assert result[0] == TradeTick.from_dict(
            {
                "type": "TradeTick",
                "instrument_id": "1-205822330-49808334-None.BETFAIR",
                "price": "3.95",
                "size": "46.950000",
                "aggressor_side": "NO_AGGRESSOR",
                "trade_id": "358e633f2969dc2f12e77c0cacce8c224a54",
                "ts_event": 0,
                "ts_init": 0,
            },
        )
        assert result[1].data == BetfairTicker.from_dict(
            {
                "type": "BetfairTicker",
                "instrument_id": "1-205822330-49808334-None.BETFAIR",
                "ts_event": 0,
                "ts_init": 0,
                "last_traded_price": 0.2531646,
                "traded_volume": 46.95,
                "starting_price_near": None,
                "starting_price_far": None,
            },
        )
        assert isinstance(result[2], OrderBookDeltas)

    @pytest.mark.parametrize(
        ("filename", "num_msgs"),
        [
            ("1-166564490.bz2", 4114),
            ("1-166811431.bz2", 29209),
            ("1-180305278.bz2", 22850),
            ("1-206064380.bz2", 70904),
        ],
    )
    def test_parsing_streaming_file(self, filename, num_msgs):
        mcms = BetfairDataProvider.market_updates(filename)
        updates = []
        for mcm in mcms:
            upd = self.parser.parse(mcm)
            updates.extend(upd)
        assert len(updates) == num_msgs

    def test_parsing_streaming_file_message_counts(self):
        mcms = BetfairDataProvider.read_mcm("1-206064380.bz2")
        updates = [x for mcm in mcms for x in self.parser.parse(mcm)]
        counts = Counter(
            [
                x.__class__.__name__ if not isinstance(x, CustomData) else x.data.__class__.__name__
                for x in updates
            ],
        )
        expected = Counter(
            {
                "OrderBookDeltas": 40525,
                "BetfairTicker": 4658,
                "TradeTick": 3487,
                "BetfairSequenceCompleted": 18793,
                "BettingInstrument": 260,
                "BSPOrderBookDelta": 2824,
                "InstrumentStatus": 260,
                "BetfairStartingPrice": 72,
                "InstrumentClose": 25,
            },
        )
        assert counts == expected

    @pytest.mark.parametrize(
        ("filename", "book_count"),
        [
            ("1-166564490.bz2", [1077, 1307]),
            ("1-166811431.bz2", [9374, 9348]),
            ("1-180305278.bz2", [1714, 7695]),
            (
                "1-206064380.bz2",
                [6736, 3362, 6785, 5701, 363, 4191, 4442, 3636, 5861, 4318, 10854, 5599, 3520],
            ),
        ],
    )
    def test_order_book_integrity(self, filename, book_count) -> None:
        mcms = BetfairDataProvider.market_updates(filename)

        books: dict[InstrumentId, OrderBook] = {}
        for update in [x for mcm in mcms for x in self.parser.parse(mcm)]:
            if isinstance(update, OrderBookDeltas) and not isinstance(
                update,
                BSPOrderBookDelta,
            ):
                instrument_id = update.instrument_id
                if instrument_id not in books:
                    instrument = betting_instrument(*instrument_id_betfair_ids(instrument_id))
                    books[instrument_id] = create_betfair_order_book(instrument.id)
                books[instrument_id].apply(update)
                books[instrument_id].check_integrity()
        result = [book.update_count for book in books.values()]
        assert result == book_count

    def test_betfair_trade_sizes(self) -> None:  # noqa: C901
        mcms = BetfairDataProvider.read_mcm("1-206064380.bz2")
        trade_ticks: dict[InstrumentId, list[TradeTick]] = defaultdict(list)
        betfair_tv: dict[int, dict[float, float]] = {}
        for mcm in mcms:
            for data in self.parser.parse(mcm):
                if isinstance(data, TradeTick):
                    trade_ticks[data.instrument_id].append(data)

            for rc in [rc for mc in mcm.mc for rc in mc.rc]:
                if rc.id not in betfair_tv:
                    betfair_tv[rc.id] = {}
                if rc.trd is not None:
                    for trd in rc.trd:
                        if trd.volume > betfair_tv[rc.id].get(trd.price, 0):
                            betfair_tv[rc.id][trd.price] = trd.volume

        for selection_id in betfair_tv:
            for price in betfair_tv[selection_id]:
                instrument_id = next(ins for ins in trade_ticks if f"-{selection_id}-" in ins.value)
                betfair_volume = betfair_tv[selection_id][price]
                trade_volume = sum(
                    [
                        tick.size
                        for tick in trade_ticks[instrument_id]
                        if tick.price.as_double() == price
                    ],
                )
                assert betfair_volume == float(trade_volume)


class TestBetfairParsing:
    @pytest.fixture(autouse=True)
    def setup(self, session_event_loop) -> None:
        # Fixture Setup
        self.loop = session_event_loop
        self.clock = LiveClock()
        self.instrument = betting_instrument()
        self.client = BetfairTestStubs.betfair_client(loop=self.loop)
        self.provider = BetfairTestStubs.instrument_provider(self.client)
        self.uuid = UUID4()
        self.parser = BetfairParser(currency="GBP")

    def test_order_side_parser_to_betfair(self):
        assert OrderSideParser.to_betfair(OrderSide.BUY) == Side.LAY
        assert OrderSideParser.to_betfair(OrderSide.SELL) == Side.BACK

    def test_order_side_parser_round_trip(self):
        assert (
            OrderSideParser.to_nautilus(OrderSideParser.to_betfair(OrderSide.BUY)) == OrderSide.BUY
        )
        assert (
            OrderSideParser.to_nautilus(OrderSideParser.to_betfair(OrderSide.SELL))
            == OrderSide.SELL
        )

    def test_order_submit_to_betfair(self):
        command = TestCommandStubs.submit_order_command(
            order=TestExecStubs.limit_order(
                price=betfair_float_to_price(2.5),
                quantity=betfair_float_to_quantity(10),
                order_side=OrderSide.SELL,
            ),
        )
        result = order_submit_to_place_order_params(command=command, instrument=self.instrument)
        expected = PlaceOrders.with_params(
            request_id=result.id,
            market_id="1-179082386",
            instructions=[
                PlaceInstruction(
                    order_type=OrderType.LIMIT,
                    selection_id=50214,
                    handicap=None,
                    side=Side.BACK,
                    limit_order=LimitOrder(
                        price=2.5,
                        size=10.0,
                        persistence_type=PersistenceType.PERSIST,
                    ),
                    limit_on_close_order=None,
                    market_on_close_order=None,
                    customer_order_ref="O-20210410-022422-001-001-1",
                ),
            ],
            customer_ref="2d89666b1a1e4a75b1934eb3b454c757",
            market_version=None,
            customer_strategy_ref="4827311aa8c4c74",
            async_=False,
        )
        assert result == expected
        assert msgspec.json.decode(msgspec.json.encode(result), type=PlaceOrders) == expected

    def test_order_update_to_betfair(self):
        modify = TestCommandStubs.modify_order_command(
            instrument_id=self.instrument.id,
            client_order_id=ClientOrderId("C-1"),
            quantity=betfair_float_to_quantity(10),
            price=betfair_float_to_price(1.35),
        )

        result = order_update_to_replace_order_params(
            command=modify,
            venue_order_id=VenueOrderId("1"),
            instrument=self.instrument,
        )
        expected = ReplaceOrders.with_params(
            request_id=result.id,
            market_id="1-179082386",
            instructions=[ReplaceInstruction(bet_id=1, new_price=1.35)],
            customer_ref="2d89666b1a1e4a75b1934eb3b454c757",
            market_version=None,
            async_=False,
        )

        assert result == expected
        assert msgspec.json.decode(msgspec.json.encode(result), type=ReplaceOrders) == expected

    def test_order_cancel_to_betfair(self):
        result = order_cancel_to_cancel_order_params(
            command=TestCommandStubs.cancel_order_command(
                venue_order_id=VenueOrderId("228302937743"),
            ),
            instrument=self.instrument,
        )
        expected = CancelOrders.with_params(
            request_id=result.id,
            market_id="1-179082386",
            instructions=[CancelInstruction(bet_id=228302937743, size_reduction=None)],
            customer_ref="2d89666b1a1e4a75b1934eb3b454c757",
        )
        assert result == expected
        assert msgspec.json.decode(msgspec.json.encode(result), type=CancelOrders) == expected

    @pytest.mark.asyncio()
    async def test_account_statement(self, betfair_client):
        mock_betfair_request(betfair_client, BetfairResponses.account_details())
        detail = await self.client.get_account_details()
        mock_betfair_request(betfair_client, BetfairResponses.account_funds_no_exposure())
        funds = await self.client.get_account_funds()
        result = betfair_account_to_account_state(
            account_detail=detail,
            account_funds=funds,
            event_id=self.uuid,
            reported=True,
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
        updates = self.parser.parse(mcm)
        assert len(updates) == 4
        trade, ticker, deltas, completed = updates
        assert isinstance(trade, TradeTick)
        assert isinstance(ticker, Data)
        assert isinstance(deltas, OrderBookDeltas)
        assert isinstance(completed, CustomData)
        assert len(deltas.deltas) == 2

    def test_make_order_limit(self):
        # Arrange
        order = TestExecStubs.limit_order(
            price=betfair_float_to_price(3.05),
            quantity=betfair_float_to_quantity(10),
            order_side=OrderSide.SELL,
        )
        command = TestCommandStubs.submit_order_command(order)

        # Act
        result = nautilus_limit_to_place_instructions(command, instrument=self.instrument)

        # Assert
        expected = PlaceInstruction(
            order_type=OrderType.LIMIT,
            selection_id=50214,
            handicap=None,
            side=Side.BACK,
            limit_order=LimitOrder(
                size=10.0,
                price=3.05,
                persistence_type=PersistenceType.PERSIST,
                time_in_force=None,
                min_fill_size=None,
                bet_target_type=None,
                bet_target_size=None,
            ),
            limit_on_close_order=None,
            market_on_close_order=None,
            customer_order_ref="O-20210410-022422-001-001-1",
        )
        assert result == expected
        assert msgspec.json.decode(msgspec.json.encode(result), type=PlaceInstruction) == expected

    def test_make_order_limit_on_close(self):
        order = TestExecStubs.limit_order(
            price=betfair_float_to_price(3.05),
            quantity=betfair_float_to_quantity(10),
            instrument=TestInstrumentProvider.betting_instrument(),
            time_in_force=TimeInForce.AT_THE_OPEN,
            order_side=OrderSide.SELL,
        )
        command = TestCommandStubs.submit_order_command(order)
        result = nautilus_limit_on_close_to_place_instructions(command, instrument=self.instrument)
        expected = PlaceInstruction(
            order_type=OrderType.LIMIT_ON_CLOSE,
            selection_id=50214,
            handicap=None,
            side=Side.BACK,
            limit_order=None,
            limit_on_close_order=LimitOnCloseOrder(liability=10.0, price=3.05),
            market_on_close_order=None,
            customer_order_ref="O-20210410-022422-001-001-1",
        )
        assert result == expected
        assert msgspec.json.decode(msgspec.json.encode(result), type=PlaceInstruction) == expected

    def test_make_order_market_buy(self):
        order = TestExecStubs.market_order(order_side=OrderSide.BUY)
        command = TestCommandStubs.submit_order_command(order)
        result = nautilus_market_to_place_instructions(command, instrument=self.instrument)
        expected = PlaceInstruction(
            order_type=OrderType.LIMIT,
            selection_id=50214,
            handicap=None,
            side=Side.LAY,
            limit_order=LimitOrder(
                size=100.0,
                price=1.01,
                persistence_type=PersistenceType.PERSIST,
                time_in_force=None,
                min_fill_size=None,
                bet_target_type=None,
                bet_target_size=None,
            ),
            limit_on_close_order=None,
            market_on_close_order=None,
            customer_order_ref="O-20210410-022422-001-001-1",
        )
        assert result == expected
        assert msgspec.json.decode(msgspec.json.encode(result), type=PlaceInstruction) == expected

    def test_make_order_market_sell(self):
        order = TestExecStubs.market_order(order_side=OrderSide.SELL)
        command = TestCommandStubs.submit_order_command(order)
        result = nautilus_market_to_place_instructions(command, instrument=self.instrument)
        expected = PlaceInstruction(
            order_type=OrderType.LIMIT,
            selection_id=50214,
            handicap=None,
            side=Side.BACK,
            limit_order=LimitOrder(
                size=100.0,
                price=1000,
                persistence_type=PersistenceType.PERSIST,
                time_in_force=None,
                min_fill_size=None,
                bet_target_type=None,
                bet_target_size=None,
            ),
            limit_on_close_order=None,
            market_on_close_order=None,
            customer_order_ref="O-20210410-022422-001-001-1",
        )
        assert result == expected
        assert msgspec.json.decode(msgspec.json.encode(result), type=PlaceInstruction) == expected

    @pytest.mark.parametrize(
        ("side", "liability"),
        [("BUY", 100.0), ("SELL", 100.0)],
    )
    def test_make_order_market_on_close(self, side, liability):
        order = TestExecStubs.market_order(
            time_in_force=TimeInForce.AT_THE_OPEN,
            order_side=order_side_from_str(side),
        )
        command = TestCommandStubs.submit_order_command(order)
        place_instructions = nautilus_market_on_close_to_place_instructions(
            command,
            instrument=self.instrument,
        )
        result = place_instructions.market_on_close_order
        expected = MarketOnCloseOrder(liability=liability)
        assert result == expected
        assert msgspec.json.decode(msgspec.json.encode(result), type=MarketOnCloseOrder) == expected

    @pytest.mark.parametrize(
        ("status", "size", "matched", "cancelled", "expected"),
        [
            (OrderStatus.EXECUTION_COMPLETE, 10.0, 10.0, 0.0, NautilusOrderStatus.FILLED),
            (OrderStatus.EXECUTION_COMPLETE, 10.0, 5.0, 5.0, NautilusOrderStatus.CANCELED),
            (OrderStatus.EXECUTABLE, 10.0, 0.0, 0.0, NautilusOrderStatus.ACCEPTED),
            (OrderStatus.EXECUTABLE, 10.0, 5.0, 0.0, NautilusOrderStatus.PARTIALLY_FILLED),
        ],
    )
    def test_determine_order_status(self, status, size, matched, cancelled, expected):
        order = CurrentOrderSummary(
            bet_id="257272569678",
            market_id="",
            selection_id=0,
            handicap=None,
            price_size=PriceSize(price=3.4, size=size),
            side=Side.BACK,
            persistence_type=PersistenceType.LAPSE,
            order_type=OrderType.LIMIT,
            placed_date=datetime.datetime.now(),
            bsp_liability=0.0,
            status=status,
            average_price_matched=3.4211,
            size_matched=matched,
            size_remaining=size - matched - cancelled,
            size_lapsed=0.0,
            size_cancelled=cancelled,
            size_voided=0.0,
        )
        status = determine_order_status(order=order)
        assert status == expected

    def test_parse_line(self):
        lines = [
            b'{"op":"connection","connectionId":"105-280621060315-3705817"}',
            b'{"op":"status","id":1,"statusCode":"SUCCESS","connectionClosed":false,"connectionsAvailable":5}',
            b'{"op":"status","id":1,"statusCode":"SUCCESS","connectionClosed":false}',
            b'{"op":"mcm","id":1,"initialClk":"nhy58bfvDawc+Jbf/A2jHKee5vUN","clk":"AAAAAAAA","conflateMs":0,"heartbeatMs":5000,"pt":1624860195431,"ct":"SUB_IMAGE","mc":[{"id":"1.184839563","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30633417","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-06-29T01:10:00.000Z","suspendTime":"2021-06-29T01:10:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"OPEN","runners":[{"status":"ACTIVE","sortPriority":1,"id":6023845},{"status":"ACTIVE","sortPriority":2,"id":237487}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-06-29T01:10:00.000Z","version":3888693695,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[1.46,59.86],[1.48,1419.67],[1.47,2.92],[1.01,971.95],[1.02,119.11],[1.21,103],[1.42,27.32]],"atl":[[2,68.67],[1000,1.72],[200,1.72]],"trd":[[1.53,27.93],[1.46,407.17],[1.41,5.15],[1.48,29.85],[1.52,53.15],[1.47,10.38],[1.49,10],[1.5,22.58],[1.4,5.76]],"batb":[[2,1.46,59.86],[0,1.48,1419.67],[1,1.47,2.92],[6,1.01,971.95],[5,1.02,119.11],[4,1.21,103],[3,1.42,27.32]],"batl":[[0,2,68.67],[2,1000,1.72],[1,200,1.72]],"tv":571.97,"id":237487},{"atb":[[2.8,1.54],[1.01,971.95],[1.02,119.11],[2,68.67],[2.82,1440.67],[2.88,14.22],[1.43,2.73]],"atl":[[9.8,25.75],[1000,1.72],[200,1.72],[3.6,2.54]],"trd":[[2.9,13.06],[2.92,2.95],[3.1,138.82],[2.88,32.33],[3.2,77.73],[2.94,27.48],[3,34.24],[3.15,2.94]],"batb":[[6,1.01,971.95],[5,1.02,119.11],[4,1.43,2.73],[3,2,68.67],[2,2.8,1.54],[1,2.82,1440.67],[0,2.88,14.22]],"batl":[[3,1000,1.72],[2,200,1.72],[1,9.8,25.75],[0,3.6,2.54]],"tv":329.55,"id":6023845}],"img":true,"tv":901.52},{"id":"1.183516561","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30533301","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-05-19T01:16:00.000Z","suspendTime":"2021-05-19T01:16:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"SUSPENDED","runners":[{"status":"ACTIVE","sortPriority":1,"id":237485},{"status":"ACTIVE","sortPriority":2,"id":60427}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-05-19T01:16:00.000Z","version":3824150209,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[2.2,238.14],[2.22,451.53],[2.1,20.7],[2.24,462.2],[2.18,8.89],[1.4,2],[1.65,86.15],[2.16,11.6],[1.01,746.03],[2.08,56.26],[1.05,91.09],[1.1,86.15],[1.3,3.84],[1.02,86.15],[1.03,86.15],[1.86,17.5]],"atl":[[2.32,11.53],[2.3,140.58],[2.28,201.16],[2.36,21.14],[1000,1.72],[200,1.72]],"trd":[[2.26,908.83],[2.24,2262.18],[2.28,1206.46],[2.22,5340.65],[2.16,2461.4],[2.2,2042.06],[2.18,1704.71],[2.08,74.11],[2.14,1098.39],[2.1,1413.03],[2.12,62.51],[2.04,7.37],[2.32,41.98],[2.3,554.84],[2,54.31],[2.36,20.68],[2.06,2045.77],[1.98,0.63]],"batb":[[2,2.2,238.14],[1,2.22,451.53],[5,2.1,20.7],[0,2.24,462.2],[3,2.18,8.89],[9,1.4,2],[8,1.65,86.15],[7,1.86,17.5],[6,2.08,56.26],[4,2.16,11.6]],"batl":[[2,2.32,11.53],[5,1000,1.72],[4,200,1.72],[3,2.36,21.14],[1,2.3,140.58],[0,2.28,201.16]],"tv":21299.91,"id":237485},{"atb":[[1.78,210.83],[1.75,14.41],[1.76,28.4],[1.79,450.18],[1.77,14.42],[1.01,746.03],[1.65,86.15],[1.05,91.09],[1.1,86.15],[1.3,3.84],[1.02,86.15],[1.03,86.15]],"atl":[[1.81,430.16],[1.82,488.33],[1.85,11.31],[1.83,14.32],[1.84,14.28],[3.1,27.45],[1000,1.72],[200,1.72],[2.08,1.72]],"trd":[[1.8,6609.88],[1.79,2742.92],[1.81,2879.6],[1.82,1567.46],[1.77,964.99],[1.86,272.44],[1.91,96.58],[1.99,16.47],[1.92,220.37],[1.76,11.91],[1.87,362.25],[1.78,437.4],[1.85,415.17],[1.84,580.74],[1.83,1394.8],[1.73,4],[1.88,22.37],[1.95,9.49],[1.96,1.96],[1.89,45.75],[1.9,2.3],[2.02,0.61],[1.93,4.71]],"batb":[[1,1.78,210.83],[4,1.75,14.41],[3,1.76,28.4],[0,1.79,450.18],[9,1.03,86.15],[8,1.05,91.09],[7,1.1,86.15],[6,1.3,3.84],[5,1.65,86.15],[2,1.77,14.42]],"batl":[[0,1.81,430.16],[8,1000,1.72],[7,200,1.72],[6,3.1,27.45],[5,2.08,1.72],[4,1.85,11.31],[3,1.84,14.28],[2,1.83,14.32],[1,1.82,488.33]],"tv":18664.17,"id":60427}],"img":true,"tv":39964.08},{"id":"1.184866117","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30635089","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-06-30T00:40:00.000Z","suspendTime":"2021-06-30T00:40:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"OPEN","runners":[{"status":"ACTIVE","sortPriority":1,"id":237477},{"status":"ACTIVE","sortPriority":2,"id":237490}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-06-30T00:40:00.000Z","version":3890540057,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[1.03,1.93],[1.02,76.24],[1.01,108.66],[1.39,68.58]],"atl":[[1.49,1.93]],"trd":[[1.39,52.64]],"batb":[[3,1.01,108.66],[2,1.02,76.24],[1,1.03,1.93],[0,1.39,68.58]],"batl":[[0,1.49,1.93]],"tv":52.64,"id":237477},{"atb":[[3.05,1.93],[1.02,76.24],[1.01,108.66],[3,13.37]],"atl":[[3.55,1.93]],"batb":[[3,1.01,108.66],[2,1.02,76.24],[1,3,13.37],[0,3.05,1.93]],"batl":[[0,3.55,1.93]],"id":237490}],"img":true,"tv":52.64}]}',
            b'{"op":"mcm","id":1,"clk":"AKgBAIgBANgB","pt":1624860200431,"ct":"HEARTBEAT"}',
        ]
        for line in lines:
            data = stream_decode(line)
            assert data

    def test_mcm(self) -> None:
        line = b'{"op":"mcm","id":1,"initialClk":"nhy58bfvDawc+Jbf/A2jHKee5vUN","clk":"AAAAAAAA","conflateMs":0,"heartbeatMs":5000,"pt":1624860195431,"ct":"SUB_IMAGE","mc":[{"id":"1.184839563","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30633417","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-06-29T01:10:00.000Z","suspendTime":"2021-06-29T01:10:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"OPEN","runners":[{"status":"ACTIVE","sortPriority":1,"id":6023845},{"status":"ACTIVE","sortPriority":2,"id":237487}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-06-29T01:10:00.000Z","version":3888693695,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[1.46,59.86],[1.48,1419.67],[1.47,2.92],[1.01,971.95],[1.02,119.11],[1.21,103],[1.42,27.32]],"atl":[[2,68.67],[1000,1.72],[200,1.72]],"trd":[[1.53,27.93],[1.46,407.17],[1.41,5.15],[1.48,29.85],[1.52,53.15],[1.47,10.38],[1.49,10],[1.5,22.58],[1.4,5.76]],"batb":[[2,1.46,59.86],[0,1.48,1419.67],[1,1.47,2.92],[6,1.01,971.95],[5,1.02,119.11],[4,1.21,103],[3,1.42,27.32]],"batl":[[0,2,68.67],[2,1000,1.72],[1,200,1.72]],"tv":571.97,"id":237487},{"atb":[[2.8,1.54],[1.01,971.95],[1.02,119.11],[2,68.67],[2.82,1440.67],[2.88,14.22],[1.43,2.73]],"atl":[[9.8,25.75],[1000,1.72],[200,1.72],[3.6,2.54]],"trd":[[2.9,13.06],[2.92,2.95],[3.1,138.82],[2.88,32.33],[3.2,77.73],[2.94,27.48],[3,34.24],[3.15,2.94]],"batb":[[6,1.01,971.95],[5,1.02,119.11],[4,1.43,2.73],[3,2,68.67],[2,2.8,1.54],[1,2.82,1440.67],[0,2.88,14.22]],"batl":[[3,1000,1.72],[2,200,1.72],[1,9.8,25.75],[0,3.6,2.54]],"tv":329.55,"id":6023845}],"img":true,"tv":901.52},{"id":"1.183516561","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30533301","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-05-19T01:16:00.000Z","suspendTime":"2021-05-19T01:16:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"SUSPENDED","runners":[{"status":"ACTIVE","sortPriority":1,"id":237485},{"status":"ACTIVE","sortPriority":2,"id":60427}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-05-19T01:16:00.000Z","version":3824150209,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[2.2,238.14],[2.22,451.53],[2.1,20.7],[2.24,462.2],[2.18,8.89],[1.4,2],[1.65,86.15],[2.16,11.6],[1.01,746.03],[2.08,56.26],[1.05,91.09],[1.1,86.15],[1.3,3.84],[1.02,86.15],[1.03,86.15],[1.86,17.5]],"atl":[[2.32,11.53],[2.3,140.58],[2.28,201.16],[2.36,21.14],[1000,1.72],[200,1.72]],"trd":[[2.26,908.83],[2.24,2262.18],[2.28,1206.46],[2.22,5340.65],[2.16,2461.4],[2.2,2042.06],[2.18,1704.71],[2.08,74.11],[2.14,1098.39],[2.1,1413.03],[2.12,62.51],[2.04,7.37],[2.32,41.98],[2.3,554.84],[2,54.31],[2.36,20.68],[2.06,2045.77],[1.98,0.63]],"batb":[[2,2.2,238.14],[1,2.22,451.53],[5,2.1,20.7],[0,2.24,462.2],[3,2.18,8.89],[9,1.4,2],[8,1.65,86.15],[7,1.86,17.5],[6,2.08,56.26],[4,2.16,11.6]],"batl":[[2,2.32,11.53],[5,1000,1.72],[4,200,1.72],[3,2.36,21.14],[1,2.3,140.58],[0,2.28,201.16]],"tv":21299.91,"id":237485},{"atb":[[1.78,210.83],[1.75,14.41],[1.76,28.4],[1.79,450.18],[1.77,14.42],[1.01,746.03],[1.65,86.15],[1.05,91.09],[1.1,86.15],[1.3,3.84],[1.02,86.15],[1.03,86.15]],"atl":[[1.81,430.16],[1.82,488.33],[1.85,11.31],[1.83,14.32],[1.84,14.28],[3.1,27.45],[1000,1.72],[200,1.72],[2.08,1.72]],"trd":[[1.8,6609.88],[1.79,2742.92],[1.81,2879.6],[1.82,1567.46],[1.77,964.99],[1.86,272.44],[1.91,96.58],[1.99,16.47],[1.92,220.37],[1.76,11.91],[1.87,362.25],[1.78,437.4],[1.85,415.17],[1.84,580.74],[1.83,1394.8],[1.73,4],[1.88,22.37],[1.95,9.49],[1.96,1.96],[1.89,45.75],[1.9,2.3],[2.02,0.61],[1.93,4.71]],"batb":[[1,1.78,210.83],[4,1.75,14.41],[3,1.76,28.4],[0,1.79,450.18],[9,1.03,86.15],[8,1.05,91.09],[7,1.1,86.15],[6,1.3,3.84],[5,1.65,86.15],[2,1.77,14.42]],"batl":[[0,1.81,430.16],[8,1000,1.72],[7,200,1.72],[6,3.1,27.45],[5,2.08,1.72],[4,1.85,11.31],[3,1.84,14.28],[2,1.83,14.32],[1,1.82,488.33]],"tv":18664.17,"id":60427}],"img":true,"tv":39964.08},{"id":"1.184866117","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":true,"persistenceEnabled":true,"marketBaseRate":5,"eventId":"30635089","eventTypeId":"7522","numberOfWinners":1,"bettingType":"ODDS","marketType":"MATCH_ODDS","marketTime":"2021-06-30T00:40:00.000Z","suspendTime":"2021-06-30T00:40:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":true,"runnersVoidable":false,"numberOfActiveRunners":2,"betDelay":0,"status":"OPEN","runners":[{"status":"ACTIVE","sortPriority":1,"id":237477},{"status":"ACTIVE","sortPriority":2,"id":237490}],"regulators":["MR_INT"],"countryCode":"GB","discountAllowed":true,"timezone":"GMT","openDate":"2021-06-30T00:40:00.000Z","version":3890540057,"priceLadderDefinition":{"type":"CLASSIC"}},"rc":[{"atb":[[1.03,1.93],[1.02,76.24],[1.01,108.66],[1.39,68.58]],"atl":[[1.49,1.93]],"trd":[[1.39,52.64]],"batb":[[3,1.01,108.66],[2,1.02,76.24],[1,1.03,1.93],[0,1.39,68.58]],"batl":[[0,1.49,1.93]],"tv":52.64,"id":237477},{"atb":[[3.05,1.93],[1.02,76.24],[1.01,108.66],[3,13.37]],"atl":[[3.55,1.93]],"batb":[[3,1.01,108.66],[2,1.02,76.24],[1,3,13.37],[0,3.05,1.93]],"batl":[[0,3.55,1.93]],"id":237490}],"img":true,"tv":52.64}]}'  # noqa
        mcm: MCM = stream_decode(line)
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
        r = b'{"op":"mcm","id":1,"clk":"ANjxBACiiQQAlpQD","pt":1672131753550,"mc":[{"id":"1.208011084","marketDefinition":{"bspMarket":true,"turnInPlayEnabled":false,"persistenceEnabled":false,"marketBaseRate":7,"eventId":"31987078","eventTypeId":"4339","numberOfWinners":1,"bettingType":"ODDS","marketType":"WIN","marketTime":"2022-12-27T09:00:00.000Z","suspendTime":"2022-12-27T09:00:00.000Z","bspReconciled":true,"complete":true,"inPlay":false,"crossMatching":false,"runnersVoidable":false,"numberOfActiveRunners":0,"betDelay":0,"status":"CLOSED","settledTime":"2022-12-27T09:02:21.000Z","runners":[{"status":"WINNER","sortPriority":1,"bsp":2.0008034621107256,"id":45967562},{"status":"LOSER","sortPriority":2,"bsp":5.5,"id":45565847},{"status":"LOSER","sortPriority":3,"bsp":9.2,"id":47727833},{"status":"LOSER","sortPriority":4,"bsp":166.61668896346615,"id":47179469},{"status":"LOSER","sortPriority":5,"bsp":44,"id":51247493},{"status":"LOSER","sortPriority":6,"bsp":32,"id":42324350},{"status":"LOSER","sortPriority":7,"bsp":7.4,"id":51247494},{"status":"LOSER","sortPriority":8,"bsp":32.28604557164013,"id":48516342}],"regulators":["MR_INT"],"venue":"Warragul","countryCode":"AU","discountAllowed":true,"timezone":"Australia/Sydney","openDate":"2022-12-27T07:46:00.000Z","version":4968605121,"priceLadderDefinition":{"type":"CLASSIC"}}}]}'  # noqa
        mcm = stream_decode(r)
        updates = self.parser.parse(mcm)
        starting_prices = [
            upd.data
            for upd in updates
            if isinstance(upd, CustomData) and isinstance(upd.data, BetfairStartingPrice)
        ]
        assert len(starting_prices) == 8
        assert starting_prices[0].instrument_id == InstrumentId.from_str(
            "1-208011084-45967562-None.BETFAIR",
        )
        assert starting_prices[0].bsp == 2.0008034621107256

    def test_mcm_bsp_example2(self):
        raw = b'{"op":"mcm","clk":"7066946780","pt":1667288437853,"mc":[{"id":"1.205880280","rc":[{"spl":[[1.01,2]],"id":49892033},{"atl":[[2.8,0],[2.78,0]],"id":49892032},{"atb":[[2.8,378.82]],"id":49892032},{"trd":[[2.8,1.16],[2.78,1.18]],"ltp":2.8,"tv":2.34,"id":49892032},{"spl":[[1.01,4.79]],"id":49892030},{"spl":[[1.01,2]],"id":49892029},{"spl":[[1.01,3.79]],"id":49892028},{"spl":[[1.01,2]],"id":49892027},{"spl":[[1.01,2]],"id":49892034}],"con":true,"img":false}]}'  # noqa
        mcm = stream_decode(raw)
        updates = self.parser.parse(mcm)
        single_instrument_bsp_updates = [
            upd
            for upd in updates
            if isinstance(upd, CustomData)
            and isinstance(upd.data, BSPOrderBookDelta)
            and upd.data.instrument_id == InstrumentId.from_str("1-205880280-49892033-None.BETFAIR")
        ]
        assert len(single_instrument_bsp_updates) == 1

    @pytest.mark.parametrize(
        ("time_in_force", "expected_time_in_force", "expected_persistence_type"),
        [
            (TimeInForce.GTC, None, PersistenceType.PERSIST),
            (TimeInForce.DAY, None, PersistenceType.LAPSE),
            (TimeInForce.FOK, BP_TimeInForce.FILL_OR_KILL, PersistenceType.LAPSE),
        ],
    )
    def test_persistence_types(
        self,
        time_in_force,
        expected_time_in_force,
        expected_persistence_type,
    ):
        # Arrange
        order = TestExecStubs.limit_order(time_in_force=time_in_force)
        command = TestCommandStubs.submit_order_command(order)

        # Act
        place_instruction = nautilus_order_to_place_instructions(command, self.instrument)
        place_time_in_force = place_instruction.limit_order.time_in_force
        place_persistence_type = place_instruction.limit_order.persistence_type

        # Assert
        assert place_time_in_force == expected_time_in_force
        assert place_persistence_type == expected_persistence_type

    def test_persistence_encoding(self):
        # Arrange
        order = TestExecStubs.limit_order(time_in_force=TimeInForce.DAY)
        command = TestCommandStubs.submit_order_command(order)

        # Act
        place_instruction = nautilus_order_to_place_instructions(command, self.instrument)
        result = msgspec.json.decode(msgspec.json.encode(place_instruction))["limitOrder"]

        expected = {
            "size": 100.0,
            "price": 55.0,
            "persistenceType": "LAPSE",
        }
        assert result == expected

    def test_customer_order_ref(self):
        # Arrange
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
        )
        client_order_id = order.client_order_id

        # Act
        customer_order_ref = make_customer_order_ref(client_order_id)

        # Assert
        assert customer_order_ref == "O-20210410-022422-001-001-1"
        assert len(customer_order_ref) <= 32

    def test_encode_place_orders(self):
        place_orders = PlaceInstruction(
            order_type=OrderType.LIMIT,
            selection_id="237486",
            handicap="0",
            side=Side.LAY,
            limit_order=LimitOrder(
                size="2",
                price="3",
                persistence_type=PersistenceType.PERSIST,
            ),
        )
        result = msgspec.json.decode(msgspec.json.encode(place_orders))
        result = {k: v for k, v in result.items() if v}
        expected = {
            "selectionId": "237486",
            "handicap": "0",
            "side": "LAY",
            "orderType": "LIMIT",
            "limitOrder": {
                "size": "2",
                "price": "3",
                "persistenceType": "PERSIST",
            },
        }
        assert result == expected

    def teardown(self) -> None:
        # pytest-asyncio manages loop lifecycle, no cleanup needed
        pass


def request_id() -> int:
    """
    `betfair_parser uses an auto=incrementing request_id which can cause issues with the
    test suite depending on how it is run.

    Return the current request value for testing purposes

    """
    return next(copy(_default_id_generator))
