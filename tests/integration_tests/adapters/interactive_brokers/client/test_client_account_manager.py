from collections import Counter
from decimal import Decimal

from nautilus_trader.adapters.interactive_brokers.client.common import IBPosition
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs


def test_accounts(ib_client):
    # Arrange
    ids = {"DU1234567", "DU7654321"}
    ib_client.account_manager.account_ids = ids

    # Act
    result = ib_client.account_manager.accounts()

    # Assert
    assert isinstance(result, set)
    assert result == ids


def test_subscribe_account_summary(ib_client):
    # Arrange, Act
    ib_client.account_manager.subscribe_account_summary()

    # Assert
    assert ib_client.subscriptions.get(name="accountSummary") is not None
    ib_client._eclient.reqAccountSummary.assert_called_once()


def test_unsubscribe_account_summary(ib_client):
    # Arrange
    ib_client.account_manager.subscribe_account_summary()

    # Act
    ib_client.account_manager.unsubscribe_account_summary("DU1234567")

    # Assert
    assert ib_client.subscriptions.get(name="accountSummary") is None
    ib_client._eclient.cancelAccountSummary.assert_called_once()


def test_get_positions(ib_client):
    # Arrange
    position_1 = IBPosition(
        "DU1234567",
        IBTestDataStubs.contract(secType="STK", symbol="AAPL", exchange="NASDAQ"),
        Decimal(5),
        10.0,
    )
    position_2 = IBPosition(
        "DU1234567",
        IBTestDataStubs.contract(secType="STK", symbol="SPY", exchange="ARCA"),
        Decimal(10),
        20.0,
    )
    positions_open = [position_1, position_2]
    ib_client.await_request.return_value = positions_open

    # Act
    result = ib_client.account_manager.get_positions("DU1234567")

    # Assert
    assert Counter(result) == Counter(positions_open)
    ib_client._eclient.reqPositions.assert_called_once()
    positions_open_cache = ib_client.cache.positions_open()
    assert Counter(positions_open_cache) == Counter(positions_open)
    assert ib_client.cache.positions_open_count() == 2
