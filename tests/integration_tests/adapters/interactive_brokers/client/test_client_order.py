# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.interactive_brokers.client.common import AccountOrderRef
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestExecStubs


def test_place_order(ib_client):
    # Arrange
    ib_order = IBTestExecStubs.aapl_buy_ib_order(order_id=1)
    ib_order.contract = IBTestContractStubs.aapl_equity_ib_contract()
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
    )


@pytest.mark.asyncio
async def test_execDetails(ib_client):
    # Arrange
    req_id = 1
    contract = Mock()
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
    handler_func.assert_called_with(
        order_ref="O-20220104-1432-001-000-1",
        execution=execution,
        commission_report=commission_report_mock,
    )


@pytest.mark.asyncio
async def test_commissionReport(ib_client):
    # Arrange
    execution = IBTestExecStubs.execution(
        order_id=1,
        account_id="DU123456",
    )
    commission_report = IBTestExecStubs.commission()

    ib_client._exec_id_details = {
        commission_report.execId: {
            "execution": execution,
            "order_ref": execution.orderRef.rsplit(":", 1)[0],
            "commission_report": commission_report,
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
    )
