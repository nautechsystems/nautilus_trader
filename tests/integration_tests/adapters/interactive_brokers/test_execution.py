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

import asyncio
from decimal import Decimal
from functools import partial

import pytest
from ibapi.order_state import OrderState as IBOrderState

# fmt: off
from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveExecClientFactory
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import instrument_id_to_ib_contract
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestExecStubs


# fmt: on

pytestmark = pytest.mark.skip(reason="Skip due currently flaky mocks")


@pytest.fixture()
def contract_details():
    return IBTestContractStubs.aapl_equity_ib_contract_details()


@pytest.fixture()
def contract(contract_details):
    return IBTestContractStubs.aapl_equity_ib_contract()


def instrument_setup(exec_client, cache, instrument=None, contract_details=None):
    instrument = instrument or IBTestContractStubs.aapl_instrument()
    contract_details = contract_details or IBTestContractStubs.aapl_equity_contract_details()
    exec_client._instrument_provider.contract_details[instrument.id.value] = contract_details
    exec_client._instrument_provider.contract_id_to_instrument_id[
        contract_details.contract.conId
    ] = instrument.id
    exec_client._instrument_provider.add(instrument)
    cache.add_instrument(instrument)


def order_setup(
    exec_client,
    instrument,
    client_order_id,
    venue_order_id,
    status: OrderStatus = OrderStatus.SUBMITTED,
):
    order = TestExecStubs.limit_order(
        instrument=instrument,
        client_order_id=client_order_id,
    )
    if status == OrderStatus.SUBMITTED:
        order = TestExecStubs.make_submitted_order(order)
    elif status == OrderStatus.ACCEPTED:
        order = TestExecStubs.make_accepted_order(order, venue_order_id=venue_order_id)
    else:
        raise ValueError(status)
    exec_client._cache.add_order(order, PositionId("1"))
    return order


def account_summary_setup(client, **kwargs):
    account_values = IBTestDataStubs.account_values()
    for summary in account_values:
        client.accountSummary(
            req_id=kwargs["reqId"],
            account=summary["account"],
            tag=summary["tag"],
            value=summary["value"],
            currency=summary["currency"],
        )


def on_open_order_setup(client, status, order_id, contract, order):
    order_state = IBOrderState()
    order_state.status = status
    client.openOrder(
        order_id=order_id,
        contract=contract,
        order=order,
        order_state=order_state,
    )


def on_cancel_order_setup(client, status, order_id, manual_cancel_order_time):
    client.orderStatus(
        order_id=order_id,
        status=status,
        filled=0,
        remaining=100,
        avg_fill_price=0,
        perm_id=1,
        parent_id=0,
        last_fill_price=0,
        client_id=1,
        why_held="",
        mkt_cap_price=0,
    )


@pytest.mark.asyncio()
async def test_factory(exec_client_config, venue, loop, msgbus, cache, clock):
    # Act
    exec_client = InteractiveBrokersLiveExecClientFactory.create(
        loop=loop,
        name=venue.value,
        config=exec_client_config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Assert
    assert exec_client is not None


@pytest.mark.asyncio()
async def test_connect(mocker, exec_client):
    # Arrange
    mocker.patch.object(
        exec_client._client._eclient,
        "reqAccountSummary",
        side_effect=partial(account_summary_setup, exec_client._client),
    )

    # Act
    exec_client.connect()
    await asyncio.sleep(0)

    # Assert
    assert exec_client.is_connected


@pytest.mark.asyncio()
async def test_disconnect(mocker, exec_client):
    # Arrange
    mocker.patch.object(
        exec_client._client._eclient,
        "reqAccountSummary",
        side_effect=partial(account_summary_setup, exec_client._client),
    )
    exec_client.connect()
    await asyncio.sleep(0)

    # Act
    exec_client.disconnect()
    await asyncio.sleep(0)

    # Assert
    assert not exec_client._client.is_ready.is_set()
    assert not exec_client.is_connected


@pytest.mark.asyncio()
async def test_submit_order(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    exec_client.connect()
    await asyncio.sleep(0)
    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client._client, "Submitted"),
    )

    # Act
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Assert
    expected = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
    )
    assert cache.order(client_order_id).instrument_id == expected.instrument_id
    assert cache.order(client_order_id).side == expected.side
    assert cache.order(client_order_id).quantity == expected.quantity
    assert cache.order(client_order_id).price == expected.price
    assert cache.order(client_order_id).status == OrderStatus.ACCEPTED


@pytest.mark.asyncio()
async def test_submit_order_what_if(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    exec_client.connect()
    await asyncio.sleep(0)
    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client._client, "PreSubmitted"),
    )

    # Act
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
        tags=IBOrderTags(whatIf=True).value,
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Assert
    assert cache.order(client_order_id).status == OrderStatus.REJECTED


@pytest.mark.asyncio()
async def test_submit_order_rejected(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # TODO: Rejected
    pass


@pytest.mark.asyncio()
async def test_submit_order_list(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    exec_client.connect()
    await asyncio.sleep(0)
    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client._client, "Submitted"),
    )

    # Act
    entry_client_order_id = TestIdStubs.client_order_id(1)
    sl_client_order_id = TestIdStubs.client_order_id(2)
    order_list = TestExecStubs.limit_with_stop_market(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        price=Price.from_str("55.0"),
        sl_trigger_price=Price.from_str("50.0"),
        entry_client_order_id=entry_client_order_id,
        sl_client_order_id=sl_client_order_id,
    )
    cache.add_order_list(order_list)
    for order in order_list.orders:
        cache.add_order(order, None)
    command = TestCommandStubs.submit_order_list_command(order_list=order_list)
    exec_client.submit_order_list(command=command)
    await asyncio.sleep(0)

    # Assert
    assert cache.order(entry_client_order_id).side == OrderSide.BUY
    assert cache.order(entry_client_order_id).price == Price.from_str("55.0")
    assert cache.order(entry_client_order_id).status == OrderStatus.ACCEPTED
    assert cache.order(sl_client_order_id).side == OrderSide.SELL
    assert cache.order(sl_client_order_id).trigger_price == Price.from_str("50.0")
    assert cache.order(sl_client_order_id).status == OrderStatus.ACCEPTED


@pytest.mark.asyncio()
async def test_modify_order(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    exec_client.connect()
    await asyncio.sleep(0)
    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
        price=Price.from_int(90),
        quantity=Quantity.from_str("100"),
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Act
    command = TestCommandStubs.modify_order_command(
        price=Price.from_int(95),
        quantity=Quantity.from_str("150"),
        order=order,
    )
    exec_client.modify_order(command=command)
    await asyncio.sleep(0)

    # Assert
    assert cache.order(client_order_id).quantity == command.quantity
    assert cache.order(client_order_id).price == command.price
    assert cache.order(client_order_id).status == OrderStatus.ACCEPTED


@pytest.mark.asyncio()
async def test_modify_order_quantity(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    exec_client.connect()
    await asyncio.sleep(0)
    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
        quantity=Quantity.from_str("100"),
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Act
    command = TestCommandStubs.modify_order_command(
        price=Price.from_int(95),
        quantity=Quantity.from_str("150"),
        order=order,
    )
    exec_client.modify_order(command=command)
    await asyncio.sleep(0)

    # Assert
    assert cache.order(client_order_id).quantity == command.quantity
    assert cache.order(client_order_id).status == OrderStatus.ACCEPTED


@pytest.mark.asyncio()
async def test_modify_order_price(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    exec_client.connect()
    await asyncio.sleep(0)
    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
        price=Price.from_int(90),
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Act
    command = TestCommandStubs.modify_order_command(
        price=Price.from_int(95),
        order=order,
    )
    exec_client.modify_order(command=command)
    await asyncio.sleep(0)

    # Assert
    assert cache.order(client_order_id).price == command.price
    assert cache.order(client_order_id).status == OrderStatus.ACCEPTED


@pytest.mark.asyncio()
async def test_cancel_order(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    exec_client.connect()
    await asyncio.sleep(0)
    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client._client, "Submitted"),
    )
    mocker.patch.object(
        exec_client._client._eclient,
        "cancelOrder",
        side_effect=partial(on_cancel_order_setup, exec_client._client, "Cancelled"),
    )
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
        price=Price.from_int(90),
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Act
    command = TestCommandStubs.cancel_order_command(order=order)
    exec_client.cancel_order(command=command)
    await asyncio.sleep(0)

    # Assert
    assert cache.order(client_order_id).status == OrderStatus.CANCELED


@pytest.mark.asyncio()
async def test_on_exec_details(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    exec_client.connect()
    await asyncio.sleep(0)
    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Act
    exec_client._client.execDetails(
        req_id=-1,
        contract=instrument_id_to_ib_contract(instrument.id),
        execution=IBTestExecStubs.execution(client_order_id=client_order_id),
    )
    exec_client._client.commissionReport(
        commission_report=IBTestExecStubs.commission(),
    )

    # Assert
    expected = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
    )
    assert cache.order(client_order_id).instrument_id == expected.instrument_id
    assert cache.order(client_order_id).filled_qty == Quantity(100, 0)
    assert cache.order(client_order_id).avg_px == Price(50, 0)
    assert cache.order(client_order_id).status == OrderStatus.FILLED


@pytest.mark.asyncio()
async def test_on_order_status_with_avg_px(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    exec_client.connect()
    await asyncio.sleep(0)
    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Act - Simulate order status update with average fill price
    exec_client._on_order_status(
        order_ref=str(client_order_id),
        order_status="Filled",
        avg_fill_price=125.50,
        filled=Decimal(100),
        remaining=Decimal(0),
    )

    # Assert - Check that avg_px is stored correctly
    assert client_order_id in exec_client._order_avg_prices
    stored_avg_px = exec_client._order_avg_prices[client_order_id]
    # Price magnifier for AAPL is 1.0, so 125.50 should be stored as Price(125.50)
    assert stored_avg_px == Price.from_str("125.50")


@pytest.mark.asyncio()
async def test_on_exec_details_uses_stored_avg_px(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    exec_client.connect()
    await asyncio.sleep(0)
    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # First update order status with avg_fill_price
    exec_client._on_order_status(
        order_ref=str(client_order_id),
        order_status="Filled",
        avg_fill_price=99.75,
        filled=Decimal(100),
        remaining=Decimal(0),
    )

    # Act - Process execution details
    exec_client._client.execDetails(
        req_id=-1,
        contract=instrument_id_to_ib_contract(instrument.id),
        execution=IBTestExecStubs.execution(client_order_id=client_order_id),
    )
    exec_client._client.commissionReport(
        commission_report=IBTestExecStubs.commission(),
    )

    # Assert - avg_px should be the one from order_status, not from execution
    assert cache.order(client_order_id).avg_px == Price.from_str("99.75")
    assert cache.order(client_order_id).status == OrderStatus.FILLED


@pytest.mark.asyncio()
async def test_on_account_update(mocker, exec_client):
    # TODO:
    pass
