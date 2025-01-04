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

from collections import Counter
from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import Mock
from unittest.mock import patch

import pytest
from ibapi import decoder

from nautilus_trader.adapters.interactive_brokers.client.common import IBPosition
from nautilus_trader.test_kit.functions import eventually
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs


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
        ib_client._start_tws_incoming_msg_reader()
        ib_client._start_internal_msg_queue_processor()

        # Assert
        await eventually(lambda: "DU1234567" in ib_client.accounts())


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
    aapl = IBTestContractStubs.aapl_equity_ib_contract()
    spy = IBTestContractStubs.create_contract(secType="STK", symbol="SPY", exchange="ARCA")
    spy = IBTestContractStubs.convert_contract_to_ib_contract(spy)
    tsla = IBTestContractStubs.create_contract(secType="STK", symbol="TSLA", exchange="ARCA")
    tsla = IBTestContractStubs.convert_contract_to_ib_contract(tsla)

    position_1 = IBPosition(
        "DU1234567",
        aapl,
        Decimal(5),
        10.0,
    )
    position_2 = IBPosition(
        "DU7654321",
        spy,
        Decimal(10),
        20.0,
    )
    position_3 = IBPosition(
        "DU7654321",
        tsla,
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
