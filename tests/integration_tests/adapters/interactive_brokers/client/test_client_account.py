import asyncio
from collections import Counter
from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import Mock
from unittest.mock import patch

import pytest
from ibapi import decoder

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


@pytest.mark.asyncio
async def test_process_account_id(ib_client):
    # Arrange
    ib_client._account_ids = set()
    ib_client._eclient.conn = MagicMock()
    ib_client._eclient.conn.isConnected.return_value = True
    ib_client._eclient.serverVersion = Mock(return_value=179)
    ib_client._eclient.decoder = decoder.Decoder(
        wrapper=ib_client._eclient.wrapper,
        serverVersion=ib_client._eclient.serverVersion(),
    )

    test_messages = [
        b"15\x001\x00DU1234567\x00",
        b"9\x001\x00574\x00",
        b"15\x001\x00DU1234567\x00",
        b"9\x001\x001\x00",
        b"4\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm\x00\x00",
        b"4\x002\x00-1\x002106\x00HMDS data farm connection is OK:ushmds\x00\x00",
        b"4\x002\x00-1\x002104\x00Market data farm connection is OK:usfarm\x00\x00",
    ]
    with patch("ibapi.comm.read_msg", side_effect=[(None, msg, b"") for msg in test_messages]):
        # Act
        ib_client._start_client_tasks_and_tws_api()
        await asyncio.sleep(0.1)

    # Assert
    assert "DU1234567" in ib_client.accounts()


def test_subscribe_account_summary(ib_client):
    # Arrange
    ib_client._eclient.reqAccountSummary = Mock()

    # Act
    ib_client.subscribe_account_summary()

    # Assert
    assert ib_client._subscriptions.get(name="accountSummary") is not None
    ib_client._eclient.reqAccountSummary.assert_called_once()


def test_unsubscribe_account_summary(ib_client):
    # Arrange
    ib_client._eclient.cancelAccountSummary = Mock()
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
    position_3 = IBPosition(
        "DU7654321",
        IBTestDataStubs.contract(secType="STK", symbol="TSLA", exchange="ARCA"),
        Decimal(10),
        20.0,
    )
    ib_client._await_request = AsyncMock(return_value=[position_1, position_2, position_3])

    # Act
    results_1 = await ib_client.get_positions("DU1234567")
    results_2 = await ib_client.get_positions("DU7654321")

    # Assert
    assert Counter(results_1) == Counter([position_1])
    assert Counter(results_2) == Counter([position_2, position_3])
    ib_client._eclient.reqPositions.assert_called()
