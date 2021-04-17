from model.orderbook.book import OrderBookDeltas
from nautilus_trader.adapters.betfair.parsing import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing import build_market_update_messages
from nautilus_trader.adapters.betfair.parsing import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_submit_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_update_to_betfair
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.tick import TradeTick
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


def test_order_submit_to_betfair(betting_instrument):
    command = BetfairTestStubs.submit_order_command()
    result = order_submit_to_betfair(command=command, instrument=betting_instrument)
    expected = {
        "customer_ref": command.id.value.replace("-", ""),
        "customer_strategy_ref": command.strategy_id.value[:15],
        "instructions": [
            {
                "customerOrderRef": command.order.client_order_id.value,
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
    result = betfair_account_to_account_state(
        account_detail=detail,
        account_funds=funds,
        event_id=uuid,
        timestamp_ns=clock.timestamp_ns(),
    )
    expected = AccountState(
        AccountId(issuer="betfair", identifier="Testy-McTest"),
        [Money(1000.0, Currency.from_str("AUD"))],
        [Money(1000.0, Currency.from_str("AUD"))],
        [Money(-0.00, Currency.from_str("AUD"))],
        {"funds": funds, "detail": detail},
        uuid,
        result.timestamp_ns,
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
