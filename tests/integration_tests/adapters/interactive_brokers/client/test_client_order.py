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

import pytest
from ibapi.order_cancel import OrderCancel as IBOrderCancel

from nautilus_trader.adapters.interactive_brokers.client.common import AccountOrderRef
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestExecStubs


def test_place_order(ib_client):
    """
    Test case for placing an order with the Interactive Brokers client.

    This test verifies that the placeOrder method is called with the correct parameters.

    """
    # Arrange: Set up the order and mock the placeOrder method
    ib_order = IBTestExecStubs.aapl_buy_ib_order(order_id=1)
    ib_order.contract = IBTestContractStubs.aapl_equity_ib_contract()
    ib_client._eclient.placeOrder = MagicMock()

    # Act: Place the order using the ib_client
    ib_client.place_order(ib_order)

    # Assert: Verify that the placeOrder method was called with the correct parameters
    ib_client._eclient.placeOrder.assert_called_once_with(
        ib_order.orderId,
        ib_order.contract,
        ib_order,
    )

    # Additional assertions to verify the attributes of the order
    assert ib_order.orderId == 1
    assert ib_order.action == "BUY"
    assert ib_order.totalQuantity == Decimal("100")
    assert ib_order.orderType == "MKT"
    assert ib_order.account == "DU123456"
    assert ib_order.tif == "IOC"


def test_cancel_order(ib_client):
    # Arrange
    order_id = 1
    ib_client._eclient.cancelOrder = MagicMock()

    # Act
    ib_client.cancel_order(order_id, IBOrderCancel)

    # Assert
    ib_client._eclient.cancelOrder.assert_called_with(
        order_id,
        IBOrderCancel,
    )


def test_cancel_all_orders(ib_client):
    # Arrange
    ib_client._eclient.reqGlobalCancel = MagicMock()

    # Act
    ib_client.cancel_all_orders()

    # Assert
    ib_client._eclient.reqGlobalCancel.assert_called_once()


@pytest.mark.asyncio
async def test_get_open_orders_with_fetch_all_false(ib_client):
    # Arrange - default behavior (fetch_all_open_orders=False)
    account_id_1 = "DU123456"
    account_id_2 = "DU999999"
    order_1 = IBTestExecStubs.aapl_buy_ib_order(order_id=1, account_id=account_id_1)
    order_2 = IBTestExecStubs.aapl_buy_ib_order(order_id=2, account_id=account_id_1)
    order_3 = IBTestExecStubs.aapl_buy_ib_order(order_id=3, account_id=account_id_2)
    all_orders = [order_1, order_2, order_3]
    ib_client._await_request = AsyncMock(return_value=all_orders)
    ib_client._fetch_all_open_orders = False  # Explicitly set to False

    ib_client._eclient.reqOpenOrders = MagicMock()
    ib_client._eclient.reqAllOpenOrders = MagicMock()

    # Act
    orders = await ib_client.get_open_orders(account_id_1)

    # Assert
    assert Counter(orders) == Counter([order_1, order_2])
    ib_client._eclient.reqOpenOrders.assert_called_once()
    ib_client._eclient.reqAllOpenOrders.assert_not_called()


@pytest.mark.asyncio
async def test_get_open_orders_with_fetch_all_true(ib_client):
    # Arrange - with fetch_all_open_orders=True
    account_id_1 = "DU123456"
    account_id_2 = "DU999999"
    order_1 = IBTestExecStubs.aapl_buy_ib_order(order_id=1, account_id=account_id_1)
    order_2 = IBTestExecStubs.aapl_buy_ib_order(order_id=2, account_id=account_id_1)
    order_3 = IBTestExecStubs.aapl_buy_ib_order(order_id=3, account_id=account_id_2)
    all_orders = [order_1, order_2, order_3]
    ib_client._await_request = AsyncMock(return_value=all_orders)
    ib_client._fetch_all_open_orders = True  # Set to True

    ib_client._eclient.reqOpenOrders = MagicMock()
    ib_client._eclient.reqAllOpenOrders = MagicMock()

    # Act
    orders = await ib_client.get_open_orders(account_id_1)

    # Assert
    assert Counter(orders) == Counter([order_1, order_2])
    ib_client._eclient.reqAllOpenOrders.assert_called_once()
    ib_client._eclient.reqOpenOrders.assert_not_called()


def test_next_order_id(ib_client):
    # Arrange
    ib_client._eclient.reqIds = Mock()
    first_id = ib_client.next_order_id()

    # Act
    second_id = ib_client.next_order_id()

    # Assert
    assert first_id + 1 == second_id
    ib_client._eclient.reqIds.assert_called_with(-1)


@pytest.mark.asyncio
async def test_openOrder(ib_client):
    # Arrange
    mock_request = Mock()
    mock_request.result = []
    ib_client._requests = Mock()
    ib_client._requests.get = Mock(return_value=mock_request)
    handler_mock = Mock()
    ib_client._event_subscriptions = Mock()
    ib_client._event_subscriptions.get = Mock(return_value=handler_mock)

    order_id = 1
    contract = IBTestContractStubs.aapl_equity_contract()
    order = IBTestExecStubs.aapl_buy_ib_order(order_id=order_id)
    order_state = IBTestExecStubs.ib_order_state(state="PreSubmitted")

    # Act
    await ib_client.process_open_order(
        order_id=order_id,
        contract=contract,
        order=order,
        order_state=order_state,
    )

    # Assert
    assert ib_client._order_id_to_order_ref[order.orderId]
    assert mock_request.result == [order]
    handler_mock.assert_not_called()


@pytest.mark.asyncio
async def test_process_open_order_when_request_not_present(ib_client):
    # Arrange
    handler_mock = Mock()
    ib_client._event_subscriptions = Mock()
    ib_client._event_subscriptions.get = Mock(return_value=handler_mock)

    order_id = 1
    contract = IBTestContractStubs.aapl_equity_contract()
    order = IBTestExecStubs.aapl_buy_ib_order(order_id=order_id)
    order_state = IBTestExecStubs.ib_order_state(state="PreSubmitted")

    # Act
    await ib_client.process_open_order(
        order_id=order_id,
        contract=contract,
        order=order,
        order_state=order_state,
    )

    # Assert
    kwargs = handler_mock.call_args_list[-1].kwargs
    assert kwargs["order_ref"] == "O-20240102-1754-001-000-1"
    assert kwargs["order"].contract == IBContract(**contract.__dict__)
    assert kwargs["order"].order_state == order_state


@pytest.mark.asyncio
async def test_orderStatus(ib_client):
    # Arrange
    ib_client._order_id_to_order_ref = {
        1: AccountOrderRef(order_id=1, account_id="DU123456"),
    }
    handler_func = Mock()
    ib_client._event_subscriptions = Mock()
    ib_client._event_subscriptions.get = MagicMock(return_value=handler_func)

    # Act
    await ib_client.process_order_status(
        order_id=1,
        status="Filled",
        filled=Decimal("100"),
        remaining=Decimal("0"),
        avg_fill_price=100.0,
        perm_id=1916994655,
        parent_id=0,
        last_fill_price=100.0,
        client_id=1,
        why_held="",
        mkt_cap_price=0.0,
    )

    # Assert
    ib_client._event_subscriptions.get.assert_called_with("orderStatus-DU123456", None)
    handler_func.assert_called_with(
        order_ref=1,
        order_status="Filled",
        avg_fill_price=100.0,
        filled=Decimal("100"),
        remaining=Decimal("0"),
    )


@pytest.mark.asyncio
async def test_orderStatus_with_zero_avg_fill_price(ib_client):
    # Arrange
    ib_client._order_id_to_order_ref = {
        1: AccountOrderRef(order_id=1, account_id="DU123456"),
    }
    handler_func = Mock()
    ib_client._event_subscriptions = Mock()
    ib_client._event_subscriptions.get = MagicMock(return_value=handler_func)

    # Act - Test with zero avg_fill_price (partial fill or edge case)
    await ib_client.process_order_status(
        order_id=1,
        status="PartiallyFilled",
        filled=Decimal("50"),
        remaining=Decimal("50"),
        avg_fill_price=0.0,
        perm_id=1916994655,
        parent_id=0,
        last_fill_price=100.0,
        client_id=1,
        why_held="",
        mkt_cap_price=0.0,
    )

    # Assert
    ib_client._event_subscriptions.get.assert_called_with("orderStatus-DU123456", None)
    handler_func.assert_called_with(
        order_ref=1,
        order_status="PartiallyFilled",
        avg_fill_price=0.0,
        filled=Decimal("50"),
        remaining=Decimal("50"),
    )


@pytest.mark.asyncio
async def test_execDetails(ib_client):
    # Arrange
    req_id = 1
    # Create a proper contract object instead of Mock
    from ibapi.contract import Contract

    contract = Contract()
    contract.symbol = "AAPL"
    contract.secType = "STK"
    contract.exchange = "SMART"
    contract.currency = "USD"

    execution = IBTestExecStubs.execution(
        order_id=1,
        account_id="DU123456",
    )

    commission_report_mock = Mock()

    ib_client._exec_id_details = {
        execution.execId: {
            "execution": execution,
            "order_ref": execution.orderRef,
            "commission_report": commission_report_mock,
        },
    }

    handler_func = Mock()
    ib_client._event_subscriptions = Mock()
    ib_client._event_subscriptions.get = MagicMock(return_value=handler_func)

    # Act
    await ib_client.process_exec_details(
        req_id=req_id,
        contract=contract,
        execution=execution,
    )

    # Assert
    # The contract should be converted to IBContract
    from nautilus_trader.adapters.interactive_brokers.parsing.instruments import IBContract

    expected_contract = IBContract(**contract.__dict__)
    handler_func.assert_called_with(
        order_ref="O-20220104-1432-001-000-1",
        execution=execution,
        commission_report=commission_report_mock,
        contract=expected_contract,
    )


@pytest.mark.asyncio
async def test_commissionReport(ib_client):
    # Arrange
    execution = IBTestExecStubs.execution(
        order_id=1,
        account_id="DU123456",
    )
    commission_report = IBTestExecStubs.commission()

    # Create a contract for the test
    from nautilus_trader.adapters.interactive_brokers.parsing.instruments import IBContract

    contract = IBContract(
        symbol="AAPL",
        secType="STK",
        exchange="SMART",
        currency="USD",
    )

    ib_client._exec_id_details = {
        commission_report.execId: {
            "execution": execution,
            "order_ref": execution.orderRef.rsplit(":", 1)[0],
            "commission_report": commission_report,
            "contract": contract,
        },
    }

    handler_func = Mock()
    ib_client._event_subscriptions = Mock()
    ib_client._event_subscriptions.get = MagicMock(return_value=handler_func)

    # Act
    await ib_client.process_commission_report(commission_report=commission_report)

    # Assert
    handler_func.assert_called_with(
        order_ref="O-20220104-1432-001-000-1",
        execution=execution,
        commission_report=commission_report,
        contract=contract,
    )
