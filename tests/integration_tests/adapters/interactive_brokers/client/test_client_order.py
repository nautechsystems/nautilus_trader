from collections import Counter
from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import Mock

import pytest

from nautilus_trader.adapters.interactive_brokers.client.common import AccountOrderRef
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestExecStubs


def test_place_order(ib_client):
    # Arrange
    ib_order = IBTestExecStubs.aapl_buy_ib_order(order_id=1)
    ib_order.contract = IBTestDataStubs.aapl_contract()
    ib_client._eclient.placeOrder = MagicMock()

    # Act
    ib_client.place_order(ib_order)

    # Assert
    ib_client._eclient.placeOrder.assert_called_with(
        ib_order.orderId,
        ib_order.contract,
        ib_order,
    )


def test_cancel_order(ib_client):
    # Arrange
    order_id = 1
    ib_client._eclient.cancelOrder = MagicMock()

    # Act
    ib_client.cancel_order(order_id)

    # Assert
    ib_client._eclient.cancelOrder.assert_called_with(
        order_id,
        "",
    )


def test_cancel_all_orders(ib_client):
    # Arrange
    ib_client._eclient.reqGlobalCancel = MagicMock()

    # Act
    ib_client.cancel_all_orders()

    # Assert
    ib_client._eclient.reqGlobalCancel.assert_called_once()


@pytest.mark.asyncio
async def test_get_open_orders(ib_client):
    # Arrange
    account_id_1 = "DU123456"
    account_id_2 = "DU999999"
    order_1 = IBTestExecStubs.aapl_buy_ib_order(order_id=1, account_id=account_id_1)
    order_2 = IBTestExecStubs.aapl_buy_ib_order(order_id=2, account_id=account_id_1)
    order_3 = IBTestExecStubs.aapl_buy_ib_order(order_id=3, account_id=account_id_2)
    all_orders = [order_1, order_2, order_3]
    ib_client._await_request = AsyncMock(return_value=all_orders)

    ib_client._eclient.reqOpenOrders = MagicMock()

    # Act
    orders = await ib_client.get_open_orders(account_id_1)

    # Assert
    assert Counter(orders) == Counter([order_1, order_2])
    ib_client._eclient.reqOpenOrders.assert_called_once()


def test_next_order_id(ib_client):
    # Arrange
    ib_client._eclient.reqIds = Mock()
    first_id = ib_client.next_order_id()

    # Act
    second_id = ib_client.next_order_id()

    # Assert
    assert first_id + 1 == second_id
    ib_client._eclient.reqIds.assert_called_with(-1)


def test_openOrder(ib_client):
    # Arrange
    mock_request = Mock()
    mock_request.result = []
    ib_client._requests = Mock()
    ib_client._requests.get = Mock(return_value=mock_request)
    handler_mock = Mock()
    ib_client._event_subscriptions = Mock()
    ib_client._event_subscriptions.get = Mock(return_value=handler_mock)

    order_id = 1
    contract = IBTestDataStubs.aapl_contract()
    order = IBTestExecStubs.aapl_buy_ib_order(order_id=order_id)
    order_state = IBTestExecStubs.ib_order_state_presubmitted()

    # Act
    ib_client.openOrder(
        order_id,
        contract,
        order,
        order_state,
    )

    # Assert
    assert ib_client._order_id_to_order_ref[order.orderId]
    assert mock_request.result == [order]
    handler_mock.assert_not_called()


def test_orderStatus(ib_client):
    # Arrange
    ib_client._order_id_to_order_ref = {
        1: AccountOrderRef(order_id=1, account_id="DU123456"),
    }
    handler_func = Mock()
    ib_client._event_subscriptions = Mock()
    ib_client._event_subscriptions.get = MagicMock(return_value=handler_func)

    # Act
    ib_client.orderStatus(
        1,
        "Filled",
        Decimal("100"),
        Decimal("0"),
        100.0,
        1916994655,
        0,
        100.0,
        1,
        "",
        0.0,
    )

    # Assert
    ib_client._event_subscriptions.get.assert_called_with("orderStatus-DU123456", None)
    handler_func.assert_called_with(
        order_ref=1,
        order_status="Filled",
    )


def test_execDetails(ib_client):
    # Arrange
    req_id = 1
    contract = Mock()
    execution = IBTestExecStubs.execution(
        order_id=1,
        account_id="DU123456",
    )

    handler_func = Mock()
    ib_client._event_subscriptions = Mock()
    ib_client._event_subscriptions.get = MagicMock(return_value=handler_func)

    # Act
    ib_client.execDetails(
        req_id,
        contract,
        execution,
    )

    # Assert


def test_commissionReport(ib_client):
    # Arrange

    # Act

    # Assert
    pass
