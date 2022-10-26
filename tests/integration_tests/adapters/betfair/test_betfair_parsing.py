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

import asyncio
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.parsing import _order_quantity_to_stake
from nautilus_trader.adapters.betfair.parsing import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing import build_market_update_messages
from nautilus_trader.adapters.betfair.parsing import determine_order_status
from nautilus_trader.adapters.betfair.parsing import make_order
from nautilus_trader.adapters.betfair.parsing import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_submit_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_update_to_betfair
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderSideParser
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.stubs.commands import TestCommandStubs
from tests.test_kit.stubs.execution import TestExecStubs
from tests.test_kit.stubs.identifiers import TestIdStubs


class TestBetfairParsing:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.logger = LiveLogger(loop=self.loop, clock=self.clock)
        self.instrument = BetfairTestStubs.betting_instrument()
        self.client = BetfairTestStubs.betfair_client(loop=self.loop, logger=self.logger)
        self.provider = BetfairTestStubs.instrument_provider(self.client)
        self.uuid = UUID4()

    @pytest.mark.parametrize(
        "quantity, betfair_quantity",
        [
            ("100", "100.0"),
            ("375", "375.0"),
            ("6.25", "6.25"),
            ("200", "200.0"),
        ],
    )
    def test_order_quantity_to_stake(self, quantity, betfair_quantity):
        result = _order_quantity_to_stake(
            quantity=Quantity.from_str(quantity),
        )
        assert result == betfair_quantity

    def test_order_submit_to_betfair(self):
        command = TestCommandStubs.submit_order_command(
            order=TestExecStubs.limit_order(
                price=Price.from_str("0.4"),
                quantity=Quantity.from_str("10"),
            )
        )
        result = order_submit_to_betfair(command=command, instrument=self.instrument)
        expected = {
            "customer_ref": command.id.value.replace("-", ""),
            "customer_strategy_ref": "S-001",
            "instructions": [
                {
                    "customerOrderRef": "O-20210410-022422-001",
                    "handicap": "0.0",
                    "limitOrder": {
                        "persistenceType": "PERSIST",
                        "price": "2.5",
                        "size": "10.0",
                    },
                    "orderType": "LIMIT",
                    "selectionId": "50214",
                    "side": "BACK",
                }
            ],
            "market_id": "1.179082386",
        }
        assert result == expected

    def test_order_update_to_betfair(self):
        modify = TestCommandStubs.modify_order_command(
            price=Price(0.74347, precision=5), quantity=Quantity.from_int(10)
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
                venue_order_id=VenueOrderId("228302937743")
            ),
            instrument=self.instrument,
        )
        expected = {
            "market_id": "1.179082386",
            "customer_ref": result["customer_ref"],
            "instructions": [
                {
                    "betId": "228302937743",
                }
            ],
        }
        assert result == expected

    @pytest.mark.asyncio
    async def test_account_statement(self):
        with patch.object(
            BetfairClient, "request", return_value=BetfairResponses.account_details()
        ):
            detail = await self.client.get_account_details()
        with patch.object(
            BetfairClient, "request", return_value=BetfairResponses.account_funds_no_exposure()
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
                )
            ],
            margins=[],
            info={"funds": funds, "detail": detail},
            event_id=self.uuid,
            ts_event=result.ts_event,
            ts_init=result.ts_init,
        )
        assert result == expected

    @pytest.mark.asyncio
    async def test_merge_order_book_deltas(self):
        await self.provider.load_all_async(market_filter={"market_id": "1.180759290"})
        raw = {
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
                }
            ],
        }
        updates = build_market_update_messages(self.provider, raw)
        assert len(updates) == 3
        trade, ticker, deltas = updates
        assert isinstance(trade, TradeTick)
        assert isinstance(ticker, Ticker)
        assert isinstance(deltas, OrderBookDeltas)
        assert len(deltas.deltas) == 2

    def test_make_order_limit(self):
        order = TestExecStubs.limit_order(
            price=Price.from_str("0.33"), quantity=Quantity.from_str("10")
        )
        result = make_order(order)
        expected = {
            "limitOrder": {"persistenceType": "PERSIST", "price": "3.05", "size": "10.0"},
            "orderType": "LIMIT",
        }
        assert result == expected

    def test_make_order_limit_on_close(self):
        order = TestExecStubs.limit_order(
            price=Price(0.33, precision=5),
            quantity=Quantity.from_int(10),
            instrument_id=TestIdStubs.betting_instrument_id(),
            time_in_force=TimeInForce.AT_THE_CLOSE,
        )
        result = make_order(order)
        expected = {
            "limitOnCloseOrder": {"price": "3.05", "liability": "10.0"},
            "orderType": "LIMIT_ON_CLOSE",
        }
        assert result == expected

    def test_make_order_market_buy(self):
        order = BetfairTestStubs.market_order(side=OrderSide.BUY)
        result = make_order(order)
        expected = {
            "limitOrder": {
                "persistenceType": "LAPSE",
                "price": "1.01",
                "size": "10.0",
                "timeInForce": "FILL_OR_KILL",
            },
            "orderType": "LIMIT",
        }
        assert result == expected

    def test_make_order_market_sell(self):
        order = BetfairTestStubs.market_order(side=OrderSide.SELL)
        result = make_order(order)
        expected = {
            "limitOrder": {
                "persistenceType": "LAPSE",
                "price": "1000.0",
                "size": "10.0",
                "timeInForce": "FILL_OR_KILL",
            },
            "orderType": "LIMIT",
        }
        assert result == expected

    @pytest.mark.parametrize(
        "side,liability",
        [("BUY", "10.0"), ("SELL", "10.0")],
    )
    def test_make_order_market_on_close(self, side, liability):
        order = BetfairTestStubs.market_order(
            time_in_force=TimeInForce.AT_THE_CLOSE, side=OrderSideParser.from_str_py(side)
        )
        result = make_order(order)
        expected = {
            "marketOnCloseOrder": {"liability": liability},
            "orderType": "MARKET_ON_CLOSE",
        }
        assert result == expected

    @pytest.mark.parametrize(
        "status,size,matched,cancelled,expected",
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
