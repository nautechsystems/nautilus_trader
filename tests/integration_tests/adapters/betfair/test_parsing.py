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
from nautilus_trader.adapters.betfair.parsing import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_submit_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_update_to_betfair
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.tick import TradeTick
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


def test_order_submit_to_betfair(betting_instrument):
    command = BetfairTestStubs.submit_order_command()
    result = order_submit_to_betfair(command=command, instrument=betting_instrument)
    expected = {
        "customer_ref": command.id.value.replace("-", ""),
        "customer_strategy_ref": "Test-1",
        "instructions": [
            {
                "customerOrderRef": "O-20210410-022422-001-001-Test",
                "handicap": "0",
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
    timestamp_ns = clock.timestamp_ns()
    result = betfair_account_to_account_state(
        account_detail=detail,
        account_funds=funds,
        event_id=uuid,
        ts_updated_ns=timestamp_ns,
        timestamp_ns=timestamp_ns,
    )
    expected = AccountState(
        account_id=AccountId(issuer="BETFAIR", number="Testy-McTest"),
        account_type=AccountType.CASH,
        base_currency=AUD,
        reported=True,  # reported
        balances=[
            AccountBalance(
                AUD, Money(1000.0, AUD), Money(0.00, AUD), Money(1000.0, AUD)
            )
        ],
        info={"funds": funds, "detail": detail},
        event_id=uuid,
        ts_updated_ns=result.timestamp_ns,
        timestamp_ns=result.timestamp_ns,
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
