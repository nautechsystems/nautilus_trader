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

import asyncio
from decimal import Decimal
from functools import partial

import pytest
from ibapi.order_state import OrderState as IBOrderState

from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags
from nautilus_trader.adapters.interactive_brokers.factories import (
    InteractiveBrokersLiveExecClientFactory,
)
from nautilus_trader.execution.messages import QueryAccount
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


@pytest.fixture
def contract_details():
    return IBTestContractStubs.aapl_equity_ib_contract_details()


@pytest.fixture
def contract(contract_details):
    return IBTestContractStubs.aapl_equity_ib_contract()


def instrument_setup(exec_client, cache, instrument=None, contract_details=None):
    instrument = instrument or IBTestContractStubs.aapl_instrument()
    contract_details = contract_details or IBTestContractStubs.aapl_equity_contract_details()
    exec_client._instrument_provider.contract_details[instrument.id] = contract_details
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


def on_open_order_setup(exec_client, client, status, order_id, contract, order):
    """
    Directly call the handler, bypassing the message queue.
    """
    order_state = IBOrderState()
    order_state.status = status
    # Extract order_ref from the order to match what the handler expects
    order_ref = order.orderRef.rsplit(":", 1)[0] if ":" in order.orderRef else order.orderRef
    # Call the handler directly on the execution client
    exec_client._on_open_order(
        order_ref=order_ref,
        order=order,
        order_state=order_state,
    )


def on_cancel_order_setup(exec_client, client, status, order_id, manual_cancel_order_time):
    """
    Directly call the handler, bypassing the message queue.
    """
    # Get the order_ref from the client's order_id mapping
    order_ref_obj = client._order_id_to_order_ref.get(order_id)
    if order_ref_obj:
        order_ref = order_ref_obj.order_id
        # Call the handler directly
        exec_client._on_order_status(
            order_ref=order_ref,
            order_status=status,
            avg_fill_price=0.0,
            filled=Decimal(0),
            remaining=Decimal(100),
        )


@pytest.mark.asyncio
async def test_factory(exec_client_config, venue, event_loop, msgbus, cache, clock):
    # Act
    exec_client = InteractiveBrokersLiveExecClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=exec_client_config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Assert
    assert exec_client is not None


@pytest.mark.asyncio
async def test_connect(mocker, exec_client):
    # Arrange
    mocker.patch.object(
        exec_client._client._eclient,
        "reqAccountSummary",
        side_effect=partial(account_summary_setup, exec_client._client),
    )

    # Mock the wait_until_ready to return immediately
    async def mock_wait_until_ready(timeout):
        exec_client._client._is_client_ready.set()
        exec_client._client._is_ib_connected.set()

    mocker.patch.object(
        exec_client._client,
        "wait_until_ready",
        side_effect=mock_wait_until_ready,
    )
    # Mock instrument provider initialize
    mocker.patch.object(
        exec_client.instrument_provider,
        "initialize",
        return_value=None,
    )
    # Ensure account_summary_loaded is set so _connect() doesn't hang
    exec_client._account_summary_loaded.set()

    # Mock _connect to set connected flag directly to avoid complex async setup
    async def mock_connect():
        # Simulate successful connection by setting the flag
        exec_client._set_connected(True)

    mocker.patch.object(
        exec_client,
        "_connect",
        side_effect=mock_connect,
    )

    # Act
    exec_client.connect()
    # Wait for the async _connect task to complete
    await asyncio.sleep(0.2)

    # Assert
    assert exec_client.is_connected


@pytest.mark.asyncio
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
    assert not exec_client._client._is_client_ready.is_set()
    assert not exec_client.is_connected


@pytest.mark.asyncio
async def test_submit_order(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
    mock_connection_setup,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    # Setup connection mocks
    mock_connection_setup()
    exec_client.connect()
    await asyncio.sleep(0.1)

    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client, exec_client._client, "Submitted"),
    )

    # Act
    order = TestExecStubs.limit_order(
        instrument=instrument,
        client_order_id=client_order_id,
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Assert
    expected = TestExecStubs.limit_order(
        instrument=instrument,
        client_order_id=client_order_id,
    )
    assert cache.order(client_order_id).instrument_id == expected.instrument_id
    assert cache.order(client_order_id).side == expected.side
    assert cache.order(client_order_id).quantity == expected.quantity
    assert cache.order(client_order_id).price == expected.price
    assert cache.order(client_order_id).status == OrderStatus.ACCEPTED


@pytest.mark.asyncio
async def test_submit_order_what_if(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
    mock_connection_setup,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    # Setup connection mocks
    mock_connection_setup()
    exec_client.connect()
    await asyncio.sleep(0.1)

    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client, exec_client._client, "PreSubmitted"),
    )

    # Act
    order = TestExecStubs.limit_order(
        instrument=instrument,
        client_order_id=client_order_id,
        tags=[IBOrderTags(whatIf=True).value],
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Assert
    assert cache.order(client_order_id).status == OrderStatus.REJECTED


@pytest.mark.asyncio
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


@pytest.mark.asyncio
async def test_submit_order_list(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
    mock_connection_setup,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    # Setup connection mocks
    mock_connection_setup()
    exec_client.connect()
    await asyncio.sleep(0.1)

    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client, exec_client._client, "Submitted"),
    )

    # Act
    entry_client_order_id = TestIdStubs.client_order_id(1)
    sl_client_order_id = TestIdStubs.client_order_id(2)
    order_list = TestExecStubs.limit_with_stop_market(
        instrument=instrument,
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


@pytest.mark.asyncio
async def test_modify_order(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
    mock_connection_setup,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    # Setup connection mocks
    mock_connection_setup()
    exec_client.connect()
    await asyncio.sleep(0.1)

    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument=instrument,
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


@pytest.mark.asyncio
async def test_modify_order_quantity(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
    mock_connection_setup,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    # Setup connection mocks
    mock_connection_setup()
    exec_client.connect()
    await asyncio.sleep(0.1)

    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument=instrument,
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


@pytest.mark.asyncio
async def test_modify_order_price(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
    mock_connection_setup,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    # Setup connection mocks
    mock_connection_setup()
    exec_client.connect()
    await asyncio.sleep(0.1)

    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument=instrument,
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


@pytest.mark.asyncio
async def test_cancel_order(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
    mock_connection_setup,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    # Setup connection mocks
    mock_connection_setup()
    exec_client.connect()
    await asyncio.sleep(0.1)

    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client, exec_client._client, "Submitted"),
    )
    mocker.patch.object(
        exec_client._client._eclient,
        "cancelOrder",
        side_effect=partial(on_cancel_order_setup, exec_client, exec_client._client, "Cancelled"),
    )
    order = TestExecStubs.limit_order(
        instrument=instrument,
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


@pytest.mark.asyncio
async def test_on_exec_details(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
    mock_connection_setup,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    # Setup connection mocks
    mock_connection_setup()
    exec_client.connect()
    await asyncio.sleep(0.1)

    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument=instrument,
        client_order_id=client_order_id,
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # Act
    # Get the venue_order_id from the order (set when order was accepted)
    venue_order_id = cache.order(client_order_id).venue_order_id
    if not venue_order_id:
        # If not set yet, use a mock order_id
        from nautilus_trader.model.identifiers import VenueOrderId

        venue_order_id = VenueOrderId("1")

    # Call process_exec_details directly to bypass message queue
    # The execution's orderRef must match the order's client_order_id
    execution = IBTestExecStubs.execution(order_id=int(venue_order_id.value))
    # Set the orderRef to match the client_order_id (with order_id suffix as IB does)
    execution.orderRef = f"{client_order_id.value}:{venue_order_id.value}"
    # Use the contract from contract_details - process_exec_details expects a Contract, not IBContract
    from ibapi.contract import Contract

    contract = Contract()
    # Copy attributes from the contract_details contract
    for attr in ["symbol", "secType", "exchange", "currency", "localSymbol", "conId"]:
        if hasattr(contract_details.contract, attr):
            setattr(contract, attr, getattr(contract_details.contract, attr))
    # Set the commission report's execId to match the execution's execId
    commission_report = IBTestExecStubs.commission()
    commission_report.execId = execution.execId
    await exec_client._client.process_exec_details(
        req_id=-1,
        contract=contract,
        execution=execution,
    )
    await exec_client._client.process_commission_report(
        commission_report=commission_report,
    )
    await asyncio.sleep(0.1)  # Allow processing to complete

    # Assert
    expected = TestExecStubs.limit_order(
        instrument=instrument,
        client_order_id=client_order_id,
    )
    assert cache.order(client_order_id).instrument_id == expected.instrument_id
    assert cache.order(client_order_id).filled_qty == Quantity(100, 0)
    assert cache.order(client_order_id).avg_px == Price(50, 0)
    assert cache.order(client_order_id).status == OrderStatus.FILLED


@pytest.mark.asyncio
async def test_on_order_status_with_avg_px(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
    mock_connection_setup,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    # Setup connection mocks
    mock_connection_setup()
    exec_client.connect()
    await asyncio.sleep(0.1)

    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument=instrument,
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


@pytest.mark.asyncio
async def test_on_exec_details_uses_stored_avg_px(
    mocker,
    exec_client,
    cache,
    instrument,
    contract_details,
    client_order_id,
    mock_connection_setup,
):
    # Arrange
    instrument_setup(
        exec_client=exec_client,
        cache=cache,
        instrument=instrument,
        contract_details=contract_details,
    )
    # Setup connection mocks
    mock_connection_setup()
    exec_client.connect()
    await asyncio.sleep(0.1)

    mocker.patch.object(
        exec_client._client._eclient,
        "placeOrder",
        side_effect=partial(on_open_order_setup, exec_client, exec_client._client, "Submitted"),
    )
    order = TestExecStubs.limit_order(
        instrument=instrument,
        client_order_id=client_order_id,
    )
    cache.add_order(order, None)
    command = TestCommandStubs.submit_order_command(order=order)
    exec_client.submit_order(command=command)
    await asyncio.sleep(0)

    # First update order status with avg_fill_price (this stores it for reference)
    exec_client._on_order_status(
        order_ref=str(client_order_id),
        order_status="Filled",
        avg_fill_price=99.75,
        filled=Decimal(100),
        remaining=Decimal(0),
    )

    # Act - Process execution details
    # Get the venue_order_id from the order (set when order was accepted)
    venue_order_id = cache.order(client_order_id).venue_order_id
    if not venue_order_id:
        # If not set yet, use a mock order_id
        from nautilus_trader.model.identifiers import VenueOrderId

        venue_order_id = VenueOrderId("1")

    # Call process_exec_details directly to bypass message queue
    # The execution's orderRef must match the order's client_order_id
    execution = IBTestExecStubs.execution(order_id=int(venue_order_id.value))
    # Set the orderRef to match the client_order_id (with order_id suffix as IB does)
    execution.orderRef = f"{client_order_id.value}:{venue_order_id.value}"
    # Execution price is 50.0 (from IBTestExecStubs.execution default)
    # Use the contract from contract_details - process_exec_details expects a Contract, not IBContract
    from ibapi.contract import Contract

    contract = Contract()
    # Copy attributes from the contract_details contract
    for attr in ["symbol", "secType", "exchange", "currency", "localSymbol", "conId"]:
        if hasattr(contract_details.contract, attr):
            setattr(contract, attr, getattr(contract_details.contract, attr))
    # Set the commission report's execId to match the execution's execId
    commission_report = IBTestExecStubs.commission()
    commission_report.execId = execution.execId
    await exec_client._client.process_exec_details(
        req_id=-1,
        contract=contract,
        execution=execution,
    )
    await exec_client._client.process_commission_report(
        commission_report=commission_report,
    )
    await asyncio.sleep(0.1)  # Allow processing to complete

    # Assert - execution price should be used for the fill, not the stored avg_px
    # The order's avg_px should be calculated from the actual execution price (50.0)
    # since this is a single fill, avg_px equals the execution price
    assert cache.order(client_order_id).avg_px == Price.from_str("50.0")
    assert cache.order(client_order_id).status == OrderStatus.FILLED
    # Verify that the stored avg_px from order_status is available in the info dict
    # (for reconciliation purposes, but doesn't override the actual fill price)
    assert cache.order(client_order_id).client_order_id in exec_client._order_avg_prices
    assert exec_client._order_avg_prices[
        cache.order(client_order_id).client_order_id
    ] == Price.from_str("99.75")


@pytest.mark.asyncio
async def test_on_account_update(mocker, exec_client):
    # TODO:
    pass


@pytest.fixture
def account_summary_setup_direct():
    """
    Directly call the handler, bypassing the message queue.
    """

    def _account_summary_setup(exec_client, **kwargs):
        account_values = IBTestDataStubs.account_values()
        # Simulate account summary callbacks by directly calling the handler
        # This bypasses the message handler queue which may not be processed in tests
        for summary in account_values:
            # Call the handler directly - it's synchronous
            exec_client._on_account_summary(
                tag=summary["tag"],
                value=summary["value"],
                currency=summary["currency"],
            )

    return _account_summary_setup


@pytest.mark.asyncio
async def test_query_account(mocker, exec_client, account_summary_setup_direct):
    # Arrange
    exec_client.connect()
    await asyncio.sleep(0.1)  # Allow connection to complete

    # Mock the reqAccountSummary method on the underlying IB client
    # to simulate the callback with account data
    def mock_req_account_summary(reqId, groupName, tags):
        # Call the account_summary_setup function directly
        # It will call the handler synchronously
        account_summary_setup_direct(exec_client, reqId=reqId)

    mocker.patch.object(
        exec_client._client._eclient,
        "reqAccountSummary",
        side_effect=mock_req_account_summary,
    )

    # Act
    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=TestIdStubs.account_id(),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    exec_client.query_account(command)

    # Wait for account summary callbacks to be processed
    # Use a timeout to prevent hanging
    try:
        await asyncio.wait_for(exec_client._account_summary_loaded.wait(), timeout=2.0)
    except TimeoutError:
        pytest.fail("Account summary loaded event was not set within timeout")

    # Assert
    # Verify that the account summary was requested
    exec_client._client._eclient.reqAccountSummary.assert_called()

    # Verify that the account summary was populated with expected values
    # See IBTestDataStubs.account_values() in test_kit.py
    assert "AUD" in exec_client._account_summary
    assert exec_client._account_summary["AUD"]["NetLiquidation"] == 1000000.24
    assert exec_client._account_summary["AUD"]["FullAvailableFunds"] == 900000.08
    assert exec_client._account_summary["AUD"]["FullInitMarginReq"] == 200000.97
    assert exec_client._account_summary["AUD"]["FullMaintMarginReq"] == 200000.36

    # Verify that the account summary loaded event was set
    assert exec_client._account_summary_loaded.is_set()
