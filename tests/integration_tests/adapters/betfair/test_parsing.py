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

from nautilus_trader.adapters.betfair.parsing import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing import build_market_update_messages
from nautilus_trader.adapters.betfair.parsing import make_order
from nautilus_trader.adapters.betfair.parsing import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_submit_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_update_to_betfair
from nautilus_trader.core.uuid import UUID
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orders.limit import LimitOrder
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


def test_order_submit_to_betfair(betting_instrument):
    command = BetfairTestStubs.submit_order_command()
    result = order_submit_to_betfair(command=command, instrument=betting_instrument)
    expected = {
        "customer_ref": command.id.value.replace("-", ""),
        "customer_strategy_ref": "S-001",
        "instructions": [
            {
                "customerOrderRef": "O-20210410-022422-001-001-S",
                "handicap": "",
                "limitOrder": {
                    "minFillSize": 0,
                    "persistenceType": "PERSIST",
                    "price": 3.05,
                    "size": 10.0,
                },
                "orderType": "LIMIT",
                "selectionId": "50214",
                "side": "BACK",
            }
        ],
        "market_id": "1.179082386",
    }
    assert result == expected


def test_order_update_to_betfair(betting_instrument):
    result = order_update_to_betfair(
        command=BetfairTestStubs.update_order_command(),
        side=OrderSide.BUY,
        venue_order_id=VenueOrderId("1"),
        instrument=betting_instrument,
    )
    expected = {
        "market_id": "1.179082386",
        "customer_ref": result["customer_ref"],
        "instructions": [{"betId": "1", "newPrice": 1.35}],
    }

    assert result == expected


def test_order_cancel_to_betfair(betting_instrument):
    result = order_cancel_to_betfair(
        command=BetfairTestStubs.cancel_order_command(), instrument=betting_instrument
    )
    expected = {
        "market_id": "1.179082386",
        "customer_ref": result["customer_ref"],
        "instructions": [
            {
                "betId": "229597791245",
            }
        ],
    }
    assert result == expected


def test_account_statement(betfair_client, uuid, clock):
    detail = betfair_client.account.get_account_details()
    funds = betfair_client.account.get_account_funds()
    result = betfair_account_to_account_state(
        account_detail=detail,
        account_funds=funds,
        event_id=uuid,
        ts_event=clock.timestamp_ns(),
        ts_init=clock.timestamp_ns(),
    )
    expected = AccountState(
        account_id=AccountId(issuer="BETFAIR", number="Testy-McTest"),
        account_type=AccountType.CASH,
        base_currency=GBP,
        reported=True,  # reported
        balances=[AccountBalance(GBP, Money(1000.0, GBP), Money(0.00, GBP), Money(1000.0, GBP))],
        info={"funds": funds, "detail": detail},
        event_id=uuid,
        ts_event=result.ts_event,
        ts_init=result.ts_init,
    )
    assert result == expected


def test__merge_order_book_deltas(provider):
    provider.load_all()
    raw = {
        "op": "mcm",
        "clk": "792361654",
        "pt": 1577575379148,
        "mc": [
            {
                "id": "1.179082386",
                "rc": [
                    {"atl": [[3.15, 3.68]], "id": 50214},
                    {"trd": [[3.15, 364.45]], "ltp": 3.15, "tv": 364.45, "id": 50214},
                    {"atb": [[3.15, 0]], "id": 50214},
                ],
                "con": True,
                "img": False,
            }
        ],
    }
    updates = build_market_update_messages(provider, raw)
    assert len(updates) == 2
    assert isinstance(updates[0], TradeTick)
    assert isinstance(updates[1], OrderBookDeltas)
    assert len(updates[1].deltas) == 2


def test_make_order():
    order = LimitOrder(
        trader_id=TraderId("TESTER-001"),
        strategy_id=StrategyId("Quoter-Warwick(AUS)2ndAug | R2800m3yo | 7.DubaiClassic"),
        instrument_id=InstrumentId(
            Symbol("HorseRacing,,30747950,20210802-034000,ODDS,WIN,1.185880713,40372921,0.0"),
            Venue("BETFAIR"),
        ),
        client_order_id=ClientOrderId(
            "O-20210802-033429-001-Warwick(AUS)2ndAug | R2800m3yo | 7.DubaiClassic-25"
        ),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("1.0"),
        price=Price.from_str("0.125"),
        time_in_force=TimeInForce.GTC,
        expire_time=None,
        init_id=UUID.from_str("91de5795-8c64-334f-4f9c-16e01572249d"),
        ts_init=1627875270563974000,
        # account_id='BETFAIR-001'
    )
    result = make_order(order)
    expected = {
        "limit_order": {"minFillSize": 0, "persistenceType": "PERSIST", "price": 8.0, "size": 1.0},
        "order_type": "LIMIT",
    }
    assert result == expected
