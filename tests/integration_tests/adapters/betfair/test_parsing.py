from nautilus_trader.adapters.betfair.parsing import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing import order_amend_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_submit_to_betfair
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.objects import Money
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


def test_order_submit_to_betfair(betting_instrument):
    result = order_submit_to_betfair(
        command=BetfairTestStubs.submit_order_command(), instrument=betting_instrument
    )
    expected = {
        "customer_ref": "1",
        "customer_strategy_ref": "1",
        "instructions": [
            {
                "customerOrderRef": "1",
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


def test_order_amend_to_betfair(betting_instrument):
    result = order_amend_to_betfair(
        command=BetfairTestStubs.amend_order_command(),
        side=OrderSide.BUY,
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
                "betId": "1",
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
