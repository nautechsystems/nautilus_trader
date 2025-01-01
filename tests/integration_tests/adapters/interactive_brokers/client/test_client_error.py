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

import functools
from unittest.mock import Mock

import pytest


@pytest.mark.asyncio
async def test_ib_is_ready_by_notification_1101(ib_client):
    # Arrange
    ib_client._is_ib_connected.clear()

    # Act
    await ib_client.process_error(
        req_id=-1,
        error_code=1101,
        error_string="Connectivity between IB and Trader Workstation has been restored",
    )

    # Assert
    assert ib_client._is_ib_connected.is_set()


@pytest.mark.asyncio
async def test_ib_is_ready_by_notification_1102(ib_client):
    # Arrange
    ib_client._is_ib_connected.clear()

    # Act
    await ib_client.process_error(
        req_id=-1,
        error_code=1102,
        error_string="Connectivity between IB and Trader Workstation has been restored",
    )

    # Assert
    assert ib_client._is_ib_connected.is_set()


@pytest.mark.asyncio
async def test_ib_is_not_ready_by_error_10182(ib_client):
    # Arrange
    req_id = 6
    ib_client._is_ib_connected.set()
    ib_client._subscriptions.add(req_id, "EUR.USD", ib_client._eclient.reqHistoricalData, {})

    # Act
    await ib_client.process_error(
        req_id=req_id,
        error_code=10182,
        error_string="Failed to request live updates (disconnected).",
    )

    # Assert
    assert not ib_client._is_ib_connected.is_set()


@pytest.mark.skip("Failing, need to investigate")
@pytest.mark.asyncio
async def test_ib_is_not_ready_by_error_10189(ib_client):
    # Arrange
    req_id = 6
    ib_client._is_ib_connected.set()
    ib_client._subscriptions.add(
        req_id=req_id,
        name="EUR.USD",
        handle=functools.partial(
            ib_client.subscribe_ticks,
            instrument_id="EUR/USD.IDEALPRO",
            contract=Mock(),
            tick_type="BidAsk",
        ),
        cancel=functools.partial(
            ib_client._eclient.cancelAccountSummary,
            reqId=req_id,
        ),
    )

    # Act
    await ib_client.process_error(
        req_id=req_id,
        error_code=10189,
        error_string="Failed to request tick-by-tick data.BidAsk tick-by-tick requests are not supported for EUR.USD.",
    )

    # Assert
    assert not ib_client._is_ib_connected.is_set()
