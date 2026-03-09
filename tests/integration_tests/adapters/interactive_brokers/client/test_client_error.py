import pytest


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
async def test_ib_is_not_ready_by_error_10182(ib_client):
    # Arrange
    req_id = 6
    ib_client._is_ib_connected.set()
    ib_client._subscriptions.add(req_id, "EUR.USD", ib_client._eclient.reqHistoricalData, {})

    # Act
    await ib_client.process_error(
        req_id=req_id,
        error_time=0,
        error_code=10182,
        error_string="Failed to request live updates (disconnected).",
    )

    # Assert
    assert not ib_client._is_ib_connected.is_set()
