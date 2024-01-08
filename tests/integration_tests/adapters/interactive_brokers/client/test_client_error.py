import functools
from unittest.mock import Mock


def test_ib_is_ready_by_notification_1101(ib_client):
    # Arrange
    ib_client._is_ib_ready.clear()

    # Act
    ib_client.error(
        -1,
        1101,
        "Connectivity between IB and Trader Workstation has been restored",
    )

    # Assert
    assert ib_client._is_ib_ready.is_set()


def test_ib_is_ready_by_notification_1102(ib_client):
    # Arrange
    ib_client._is_ib_ready.clear()

    # Act
    ib_client.error(
        -1,
        1102,
        "Connectivity between IB and Trader Workstation has been restored",
    )

    # Assert
    assert ib_client._is_ib_ready.is_set()


def test_ib_is_not_ready_by_error_10182(ib_client):
    # Arrange
    req_id = 6
    ib_client._is_ib_ready.set()
    ib_client._subscriptions.add(req_id, "EUR.USD", ib_client._eclient.reqHistoricalData, {})

    # Act
    ib_client.error(req_id, 10182, "Failed to request live updates (disconnected).")

    # Assert
    assert not ib_client._is_ib_ready.is_set()


def test_ib_is_not_ready_by_error_10189(ib_client):
    # Arrange
    req_id = 6
    ib_client._is_ib_ready.set()
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
    ib_client.error(
        req_id,
        10189,
        "Failed to request tick-by-tick data.BidAsk tick-by-tick requests are not supported for EUR.USD.",
    )

    # Assert
    assert not ib_client._is_ib_ready.is_set()
