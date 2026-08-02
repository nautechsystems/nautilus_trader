# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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


from unittest.mock import AsyncMock

import pytest

from nautilus_trader.test_kit.functions import eventually


@pytest.mark.asyncio
async def test_ib_is_ready_by_notification_1101(ib_client):
    # Arrange
    ib_client._is_ib_connected.clear()

    # Act
    await ib_client.process_error(
        req_id=-1,
        error_time=0,
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
        error_time=0,
        error_code=1102,
        error_string="Connectivity between IB and Trader Workstation has been restored",
    )

    # Assert
    assert ib_client._is_ib_connected.is_set()


@pytest.mark.asyncio
async def test_ib_is_not_ready_by_error_326(ib_client):
    """
    Test that a single error 326 (client id already in use) clears the connection flag
    and increments the collision counter, but does NOT fetch all open orders or change
    the client id (a back-off then retry with the same configured id preserves order
    isolation).
    """
    # Arrange
    ib_client._is_ib_connected.set()
    ib_client._client_id_collision_count = 0
    ib_client._fetch_all_open_orders = False

    # Act
    await ib_client.process_error(
        req_id=-1,
        error_time=0,
        error_code=326,
        error_string="Unable to connect as the client id is already in use",
    )

    # Assert
    assert not ib_client._is_ib_connected.is_set()
    assert ib_client._client_id_collision_count == 1
    assert ib_client._fetch_all_open_orders is False


@pytest.mark.asyncio
async def test_error_326_increments_collision_count_when_not_connected(ib_client):
    # Arrange
    ib_client._is_ib_connected.clear()
    ib_client._client_id_collision_count = 0
    ib_client._fetch_all_open_orders = False

    # Act
    await ib_client.process_error(
        req_id=-1,
        error_time=0,
        error_code=326,
        error_string="Unable to connect as the client id is already in use",
    )

    # Assert
    assert not ib_client._is_ib_connected.is_set()
    assert ib_client._client_id_collision_count == 1
    assert ib_client._fetch_all_open_orders is False


@pytest.mark.asyncio
async def test_subscription_disconnect_10182_keeps_connection_and_marks_farm_degraded(ib_client):
    """
    Test that a single subscription disconnect (10182) does NOT tear down the socket; it
    keeps the connection (and the order/execution channel) alive and marks the data
    feeds degraded so they resubscribe once the farm recovers.
    """
    # Arrange
    req_id = 6
    ib_client._is_ib_connected.set()
    ib_client._data_farm_degraded_since_ns = None
    ib_client._subscriptions.add(req_id, "EUR.USD", ib_client._eclient.reqHistoricalData, {})

    # Act
    await ib_client.process_error(
        req_id=req_id,
        error_time=0,
        error_code=10182,
        error_string="Failed to request live updates (disconnected).",
    )

    # Assert
    assert ib_client._is_ib_connected.is_set()
    assert ib_client._data_farm_degraded_since_ns is not None


@pytest.mark.asyncio
async def test_market_data_farm_broken_2103_keeps_connection(ib_client):
    """
    Test that a transient market-data farm "broken" (2103) does NOT tear down the
    socket; it keeps the connection alive and only marks the data feeds degraded.
    """
    # Arrange
    ib_client._is_ib_connected.set()
    ib_client._data_farm_degraded_since_ns = None

    # Act
    await ib_client.process_error(
        req_id=-1,
        error_time=0,
        error_code=2103,
        error_string="Market data farm connection is broken:usfarm",
    )

    # Assert
    assert ib_client._is_ib_connected.is_set()
    assert ib_client._data_farm_degraded_since_ns is not None


@pytest.mark.asyncio
async def test_hmds_farm_broken_2105_keeps_connection(ib_client):
    """
    Test that a transient HMDS (historical) data farm "broken" (2105) does NOT tear down
    the socket; it keeps the connection alive and only marks the data feeds degraded.
    """
    # Arrange
    ib_client._is_ib_connected.set()
    ib_client._data_farm_degraded_since_ns = None

    # Act
    await ib_client.process_error(
        req_id=-1,
        error_time=0,
        error_code=2105,
        error_string="HMDS data farm connection is broken:ushmds",
    )

    # Assert
    assert ib_client._is_ib_connected.is_set()
    assert ib_client._data_farm_degraded_since_ns is not None


@pytest.mark.asyncio
async def test_market_data_farm_ok_2104_resubscribes_when_degraded(ib_client):
    """
    Test that a market-data farm "OK" (2104) resubscribes the degraded feeds without a
    socket teardown, then clears the degraded marker.
    """
    # Arrange
    ib_client._is_ib_connected.set()
    ib_client._data_farm_degraded_since_ns = 1
    ib_client._resubscribe_all = AsyncMock()

    # Act
    await ib_client.process_error(
        req_id=-1,
        error_time=0,
        error_code=2104,
        error_string="Market data farm connection is OK:usfarm",
    )
    await eventually(lambda: ib_client._data_farm_degraded_since_ns is None)

    # Assert
    assert ib_client._is_ib_connected.is_set()
    ib_client._resubscribe_all.assert_awaited_once()


@pytest.mark.asyncio
async def test_hmds_farm_ok_2106_resubscribes_when_degraded(ib_client):
    """
    Test that an HMDS data farm "OK" (2106) resubscribes the degraded feeds without a
    socket teardown, then clears the degraded marker.
    """
    # Arrange
    ib_client._is_ib_connected.set()
    ib_client._data_farm_degraded_since_ns = 1
    ib_client._resubscribe_all = AsyncMock()

    # Act
    await ib_client.process_error(
        req_id=-1,
        error_time=0,
        error_code=2106,
        error_string="HMDS data farm connection is OK:ushmds",
    )
    await eventually(lambda: ib_client._data_farm_degraded_since_ns is None)

    # Assert
    assert ib_client._is_ib_connected.is_set()
    ib_client._resubscribe_all.assert_awaited_once()


@pytest.mark.asyncio
async def test_data_farm_ok_without_degradation_does_not_resubscribe(ib_client):
    """
    Test that a farm "OK" with no outstanding degradation is a no-op (no spurious
    resubscribe).
    """
    # Arrange
    ib_client._is_ib_connected.set()
    ib_client._data_farm_degraded_since_ns = None
    ib_client._resubscribe_all = AsyncMock()

    # Act
    await ib_client.process_error(
        req_id=-1,
        error_time=0,
        error_code=2104,
        error_string="Market data farm connection is OK:usfarm",
    )

    # Assert
    assert ib_client._is_ib_connected.is_set()
    ib_client._resubscribe_all.assert_not_awaited()


@pytest.mark.asyncio
async def test_data_farm_flap_sequence_keeps_socket_and_client_id(ib_client):
    """
    Model a nightly-maintenance data-farm flap (2105/2103 broken then 2104/2106 OK) and
    assert the socket (order/execution channel) is never torn down and the client id is
    never rotated; only the data feeds are resubscribed on recovery.
    """
    # Arrange
    ib_client._is_ib_connected.set()
    ib_client._data_farm_degraded_since_ns = None
    ib_client._client_id = ib_client._configured_client_id
    ib_client._client_id_collision_count = 0
    ib_client._resubscribe_all = AsyncMock()

    # Act - farm goes broken (market-data and HMDS)
    for error_code, error_string in [
        (2105, "HMDS data farm connection is broken:ushmds"),
        (2103, "Market data farm connection is broken:usfarm"),
    ]:
        await ib_client.process_error(
            req_id=-1,
            error_time=0,
            error_code=error_code,
            error_string=error_string,
        )

    # Assert - socket stays up through the outage, no client-id churn
    assert ib_client._is_ib_connected.is_set()
    assert ib_client._client_id == ib_client._configured_client_id
    assert ib_client._client_id_collision_count == 0
    assert ib_client._data_farm_degraded_since_ns is not None

    # Act - farm recovers
    for error_code, error_string in [
        (2104, "Market data farm connection is OK:usfarm"),
        (2106, "HMDS data farm connection is OK:ushmds"),
    ]:
        await ib_client.process_error(
            req_id=-1,
            error_time=0,
            error_code=error_code,
            error_string=error_string,
        )
    await eventually(lambda: ib_client._data_farm_degraded_since_ns is None)

    # Assert - feeds resubscribed, still connected, client id unchanged
    assert ib_client._is_ib_connected.is_set()
    assert ib_client._client_id == ib_client._configured_client_id
    ib_client._resubscribe_all.assert_awaited()
