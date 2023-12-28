from collections import Counter
from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.interactive_brokers.client.common import IBPosition
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs


def test_accounts(ib_client):
    # Arrange
    ids = {"DU1234567", "DU7654321"}
    ib_client._account_ids = ids

    # Act
    result = ib_client.accounts()

    # Assert
    assert isinstance(result, set)
    assert result == ids


def test_subscribe_account_summary(ib_client):
    # Arrange
    ib_client._eclient.reqAccountSummary = MagicMock()

    # Act
    ib_client.subscribe_account_summary()

    # Assert
    assert ib_client._subscriptions.get(name="accountSummary") is not None
    ib_client._eclient.reqAccountSummary.assert_called_once()


def test_unsubscribe_account_summary(ib_client):
    # Arrange
    ib_client._eclient.cancelAccountSummary = MagicMock()
    ib_client._subscriptions.add(
        req_id=1,
        name="accountSummary",
        handle=MagicMock(),
        cancel=ib_client._eclient.cancelAccountSummary,
    )

    # Act
    ib_client.unsubscribe_account_summary("DU1234567")

    # Assert
    assert ib_client._subscriptions.get(req_id=1) is None
    ib_client._eclient.cancelAccountSummary.assert_called_once()


@pytest.mark.asyncio
async def test_get_positions_simulates_two_positions(ib_client):
    # Arrange
    ib_client._eclient.reqPositions = MagicMock()
    position_1 = IBPosition(
        "DU1234567",
        IBTestDataStubs.contract(secType="STK", symbol="AAPL", exchange="NASDAQ"),
        Decimal(5),
        10.0,
    )
    position_2 = IBPosition(
        "DU7654321",
        IBTestDataStubs.contract(secType="STK", symbol="SPY", exchange="ARCA"),
        Decimal(10),
        20.0,
    )
    positions_open = [position_1, position_2]
    ib_client.await_request = AsyncMock()
    ib_client.await_request.return_value = positions_open

    # Mock _await_request method
    ib_client._await_request = AsyncMock()
    ib_client._await_request.return_value = positions_open

    # Act
    result_1 = await ib_client.get_positions("DU1234567")
    result_2 = await ib_client.get_positions("DU7654321")

    # Assert
    assert Counter(result_1) == Counter([position_1])
    assert Counter(result_2) == Counter([position_2])
    ib_client._eclient.reqPositions.assert_called()
