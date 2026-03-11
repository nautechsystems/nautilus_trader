from decimal import Decimal
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.interactive_brokers.client.common import AccountOrderRef
from nautilus_trader.model.identifiers import VenueOrderId
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestExecStubs


def test_place_and_cancel_order_capture_gateway_send_timestamps(ib_client):
    # Arrange
    ib_order = IBTestExecStubs.aapl_buy_ib_order(order_id=1)
    ib_order.contract = IBTestContractStubs.aapl_equity_ib_contract()
    ib_client._eclient.placeOrder = MagicMock()
    ib_client._eclient.cancelOrder = MagicMock()

    submit_gateway_send_ns = 1_704_067_201_000_000_000
    cancel_gateway_send_ns = 1_704_067_202_000_000_000

    # Act
    ib_client._clock.set_time(submit_gateway_send_ns)
    ib_client.place_order(ib_order)
    ib_client._clock.set_time(cancel_gateway_send_ns)
    ib_client.cancel_order(order_id=1)

    # Assert
    order_ref = ib_client._order_id_to_order_ref[VenueOrderId(str(ib_order.orderId))].order_id
    latency = ib_client._order_latency_by_order_ref[order_ref]
    assert latency["ts_submit_gateway_send_ns"] == submit_gateway_send_ns
    assert latency["ts_cancel_gateway_send_ns"] == cancel_gateway_send_ns


def test_order_latency_cache_is_bounded_and_evicts_oldest_entries(ib_client):
    ib_client._order_latency_cache_max_entries = 2

    ib_client._clock.set_time(1_704_067_210_000_000_000)
    ib_client._record_order_latency("ORDER-1", "ts_submit_gateway_send_ns")
    ib_client._clock.set_time(1_704_067_211_000_000_000)
    ib_client._record_order_latency("ORDER-2", "ts_submit_gateway_send_ns")
    ib_client._clock.set_time(1_704_067_212_000_000_000)
    ib_client._record_order_latency("ORDER-3", "ts_submit_gateway_send_ns")

    assert list(ib_client._order_latency_by_order_ref) == ["ORDER-2", "ORDER-3"]


def test_wrapper_callbacks_capture_receipt_timestamps_before_queue_handoff(ib_client):
    # Arrange
    wrapper = ib_client._eclient.wrapper
    ib_client.submit_to_msg_handler_queue = MagicMock()

    order_status_recv_ns = 1_704_067_203_000_000_000
    open_order_recv_ns = 1_704_067_204_000_000_000
    exec_details_recv_ns = 1_704_067_205_000_000_000

    # Act
    ib_client._clock.set_time(order_status_recv_ns)
    wrapper.orderStatus(
        orderId=1,
        status="Submitted",
        filled=Decimal(0),
        remaining=Decimal(100),
        avgFillPrice=0.0,
        permId=1916994655,
        parentId=0,
        lastFillPrice=0.0,
        clientId=1,
        whyHeld="",
        mktCapPrice=0.0,
    )

    ib_client._clock.set_time(open_order_recv_ns)
    wrapper.openOrder(
        orderId=1,
        contract=IBTestContractStubs.aapl_equity_contract(),
        order=IBTestExecStubs.aapl_buy_ib_order(order_id=1),
        orderState=IBTestExecStubs.ib_order_state(state="Submitted"),
    )

    ib_client._clock.set_time(exec_details_recv_ns)
    wrapper.execDetails(
        reqId=-1,
        contract=IBTestContractStubs.aapl_equity_contract(),
        execution=IBTestExecStubs.execution(order_id=1, account_id="DU123456"),
    )

    # Assert
    queued_tasks = [
        call.args[0]
        for call in ib_client.submit_to_msg_handler_queue.call_args_list
    ]
    assert queued_tasks[0].keywords["ts_order_status_recv_ns"] == order_status_recv_ns
    assert queued_tasks[1].keywords["ts_open_order_recv_ns"] == open_order_recv_ns
    assert queued_tasks[2].keywords["ts_exec_details_recv_ns"] == exec_details_recv_ns


@pytest.mark.asyncio
async def test_terminal_order_status_evicts_latency_cache_for_canceled_order(ib_client):
    venue_order_id = VenueOrderId("1")
    ib_client._order_id_to_order_ref[venue_order_id] = AccountOrderRef(
        account_id="DU123456",
        order_id="ORDER-1",
    )
    ib_client._order_latency_by_order_ref["ORDER-1"] = {
        "ts_submit_gateway_send_ns": 100,
    }

    await ib_client.process_order_status(
        order_id=1,
        status="Cancelled",
        filled=Decimal(0),
        remaining=Decimal(1),
        avg_fill_price=0.0,
        perm_id=0,
        parent_id=0,
        last_fill_price=0.0,
        client_id=1,
        why_held="",
        mkt_cap_price=0.0,
        ts_order_status_recv_ns=200,
    )

    assert "ORDER-1" not in ib_client._order_latency_by_order_ref


@pytest.mark.asyncio
async def test_order_error_evicts_latency_cache_for_rejected_order(ib_client):
    venue_order_id = VenueOrderId("2")
    ib_client._order_id_to_order_ref[venue_order_id] = AccountOrderRef(
        account_id="DU123456",
        order_id="ORDER-2",
    )
    ib_client._order_latency_by_order_ref["ORDER-2"] = {
        "ts_submit_gateway_send_ns": 300,
    }

    await ib_client._handle_order_error(
        req_id=2,
        error_code=201,
        error_string="rejected",
    )

    assert "ORDER-2" not in ib_client._order_latency_by_order_ref
