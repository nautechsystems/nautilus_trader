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

import json
from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.bullet.config import BulletExecClientConfig
from nautilus_trader.adapters.bullet.execution import BulletExecutionClient
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.bullet.conftest import _TEST_ADDRESS
from tests.integration_tests.adapters.bullet.conftest import _create_ws_mock


@pytest.fixture
def exec_client_builder(
    event_loop,
    mock_http_client,
    mock_order_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    def builder(monkeypatch, *, config_kwargs: dict | None = None):
        ws_client = _create_ws_mock()

        monkeypatch.setattr(
            "nautilus_trader.adapters.bullet.execution.BulletExecutionClient._await_account_registered",
            AsyncMock(),
        )

        mock_http_client.reset_mock()
        mock_order_client.reset_mock()
        mock_order_client.account_address = _TEST_ADDRESS
        mock_order_client.connect = AsyncMock()
        mock_order_client.place_order = AsyncMock(return_value="0xtxid")
        mock_order_client.cancel_order = AsyncMock(return_value="0xtxcancel")
        mock_order_client.amend_order = AsyncMock(return_value="0xtxamend")
        mock_order_client.cancel_all_orders = AsyncMock(return_value="0xtxcancall")
        mock_order_client.cancel_market_orders = AsyncMock(return_value="0xtxcancmkt")
        mock_order_client.batch_cancel_orders = AsyncMock(return_value="0xtxbatch")

        mock_instrument_provider.load_all_async.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = []

        _account_json = '{"totalWalletBalance":"20000.00","availableBalance":"19900.00","positions":[]}'
        mock_http_client.account_json = AsyncMock(return_value=_account_json)
        mock_http_client.open_orders_json = AsyncMock(return_value="[]")

        config = BulletExecClientConfig(
            base_url_http="https://tradingapi.testnet.bullet.xyz",
            base_url_ws="wss://tradingapi.testnet.bullet.xyz/ws",
            private_key="deadbeef" * 8,
            **(config_kwargs or {}),
        )

        client = BulletExecutionClient(
            loop=event_loop,
            http_client=mock_http_client,
            order_client=mock_order_client,
            ws_client=ws_client,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
        )

        return client, ws_client, mock_http_client, mock_order_client

    return builder


# ── Lifecycle ─────────────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_account_id_is_bullet_master(exec_client_builder, monkeypatch):
    client, _, _, _ = exec_client_builder(monkeypatch)
    assert client.account_id.value == "BULLET-master"


@pytest.mark.asyncio
async def test_connect_calls_all_dependencies(exec_client_builder, monkeypatch, mock_instrument_provider):
    client, ws_client, http_client, order_client = exec_client_builder(monkeypatch)

    await client._connect()

    try:
        mock_instrument_provider.load_all_async.assert_awaited_once()
        order_client.connect.assert_awaited_once()
        http_client.account_json.assert_awaited_once_with(_TEST_ADDRESS)
        ws_client.connect.assert_awaited_once()
        ws_client.wait_until_active.assert_awaited_once()
        ws_client.subscribe_order_updates.assert_awaited_once_with(_TEST_ADDRESS)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_disconnect_closes_ws(exec_client_builder, monkeypatch):
    client, ws_client, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    await client._disconnect()

    ws_client.close.assert_awaited_once()


# ── WS message dispatch ───────────────────────────────────────────────────────

def _make_order_update(
    symbol: str,
    order_id: int,
    cloid_int: int,
    status: str,
    side: str = "BUY",
    price: str = "90.00",
    qty: str = "0.01",
    last_fill_qty: str = "0",
    last_fill_price: str = "0",
    event_time_us: int = 1_000_000,
) -> str:
    return json.dumps({
        "e": "ORDER_TRADE_UPDATE",
        "E": event_time_us,
        "s": symbol,
        "orderId": order_id,
        "clientOrderId": cloid_int,
        "status": status,
        "side": side,
        "price": price,
        "origQty": qty,
        "lastFilledQty": last_fill_qty,
        "lastFilledPrice": last_fill_price,
        "T": event_time_us,
    })


def _setup_order_in_cache(client, cache, instrument, cloid_int: int = 1) -> tuple:
    """Add a limit order to the cache and populate cloid maps on the client."""
    client_order_id = ClientOrderId("O-TEST-001")
    venue_order_id = VenueOrderId("85000001")

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=client_order_id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("90.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)
    cache.add_venue_order_id(client_order_id, venue_order_id)

    # Populate the cloid maps as _place_order would
    client._cloid_map[cloid_int] = client_order_id
    client._nt_to_cloid[client_order_id.value] = cloid_int

    return client_order_id, venue_order_id, order


@pytest.mark.asyncio
async def test_handle_order_update_new_generates_accepted(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    client_order_id, _, order = _setup_order_in_cache(client, cache, instrument, cloid_int=42)

    with patch.object(client, "generate_order_accepted") as mock_accepted:
        msg = _make_order_update("SOL-USD", 85000001, 42, "NEW")
        client._handle_msg(msg)

        mock_accepted.assert_called_once()
        call_kwargs = mock_accepted.call_args.kwargs
        assert call_kwargs["client_order_id"] == client_order_id
        assert call_kwargs["venue_order_id"] == VenueOrderId("85000001")


@pytest.mark.asyncio
async def test_handle_order_update_canceled_generates_canceled(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    client_order_id, _, _ = _setup_order_in_cache(client, cache, instrument, cloid_int=42)

    with patch.object(client, "generate_order_canceled") as mock_canceled:
        msg = _make_order_update("SOL-USD", 85000001, 42, "CANCELED")
        client._handle_msg(msg)

        mock_canceled.assert_called_once()
        assert mock_canceled.call_args.kwargs["client_order_id"] == client_order_id

    # Cloid maps cleaned up on cancel
    assert 42 not in client._cloid_map
    assert client_order_id.value not in client._nt_to_cloid


@pytest.mark.asyncio
async def test_handle_order_update_filled_generates_filled(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    client_order_id, _, _ = _setup_order_in_cache(client, cache, instrument, cloid_int=42)

    with patch.object(client, "generate_order_filled") as mock_filled:
        msg = _make_order_update(
            "SOL-USD", 85000001, 42, "FILLED",
            last_fill_qty="0.01", last_fill_price="90.00",
        )
        client._handle_msg(msg)

        mock_filled.assert_called_once()
        call_kwargs = mock_filled.call_args.kwargs
        assert call_kwargs["client_order_id"] == client_order_id
        assert call_kwargs["last_qty"] == Quantity.from_str("0.01")
        assert call_kwargs["last_px"] == Price.from_str("90.00")

    # Cloid maps cleaned up on full fill
    assert 42 not in client._cloid_map


@pytest.mark.asyncio
async def test_handle_order_update_partially_filled_generates_filled(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    _setup_order_in_cache(client, cache, instrument, cloid_int=42)

    with patch.object(client, "generate_order_filled") as mock_filled:
        msg = _make_order_update(
            "SOL-USD", 85000001, 42, "PARTIALLY_FILLED",
            qty="0.02", last_fill_qty="0.01", last_fill_price="90.00",
        )
        client._handle_msg(msg)

        mock_filled.assert_called_once()

    # Cloid maps NOT cleaned up for partial fill
    assert 42 in client._cloid_map


@pytest.mark.asyncio
async def test_handle_order_update_zero_fill_qty_skips_fill(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    _setup_order_in_cache(client, cache, instrument, cloid_int=42)

    with patch.object(client, "generate_order_filled") as mock_filled:
        msg = _make_order_update("SOL-USD", 85000001, 42, "FILLED",
                                  last_fill_qty="0", last_fill_price="0")
        client._handle_msg(msg)
        mock_filled.assert_not_called()


@pytest.mark.asyncio
async def test_handle_order_update_unknown_instrument_skips(
    exec_client_builder, monkeypatch, cache
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    # No instrument in cache

    with patch.object(client, "generate_order_accepted") as mock_accepted:
        msg = _make_order_update("XYZ-USD", 99, 1, "NEW")
        client._handle_msg(msg)
        mock_accepted.assert_not_called()


@pytest.mark.asyncio
async def test_handle_order_update_unknown_cloid_skips(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    # No order in cache, no cloid map entry

    with patch.object(client, "generate_order_accepted") as mock_accepted:
        msg = _make_order_update("SOL-USD", 99999, 9999, "NEW")
        client._handle_msg(msg)
        mock_accepted.assert_not_called()


@pytest.mark.asyncio
async def test_handle_msg_non_order_event_ignored(
    exec_client_builder, monkeypatch
):
    client, _, _, _ = exec_client_builder(monkeypatch)

    with patch.object(client, "_handle_order_update") as mock_handler:
        client._handle_msg(json.dumps({"e": "markPriceUpdate", "p": "90.00"}))
        mock_handler.assert_not_called()


@pytest.mark.asyncio
async def test_handle_msg_invalid_json_ignored(exec_client_builder, monkeypatch):
    client, _, _, _ = exec_client_builder(monkeypatch)
    # Should not raise
    client._handle_msg("not-json{{{")


@pytest.mark.asyncio
async def test_handle_msg_non_string_ignored(exec_client_builder, monkeypatch):
    client, _, _, _ = exec_client_builder(monkeypatch)
    # Should not raise
    client._handle_msg({"e": "ORDER_TRADE_UPDATE"})


# ── Order submission ──────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_submit_limit_buy_order(exec_client_builder, monkeypatch, instrument, cache):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-SUBMIT-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("90.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        await client._submit_order(command)

        order_client.place_order.assert_awaited_once()
        call_kwargs = order_client.place_order.call_args.kwargs
        assert call_kwargs["symbol"] == "SOL-USD"
        assert call_kwargs["is_buy"] is True
        assert call_kwargs["is_limit"] is True
        assert call_kwargs["price"] == "90.00"
        assert call_kwargs["qty"] == "0.01"
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_limit_sell_order(exec_client_builder, monkeypatch, instrument, cache):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-SUBMIT-002"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("95.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        await client._submit_order(command)

        call_kwargs = order_client.place_order.call_args.kwargs
        assert call_kwargs["is_buy"] is False
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_cloid_mapping_populated(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    # Record the cloid counter before submission
    initial_cloid = client._next_cloid

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-CLOID-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("90.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    command = SubmitOrder(
        trader_id=order.trader_id, strategy_id=order.strategy_id, order=order,
        command_id=TestIdStubs.uuid(), ts_init=0, position_id=None, client_id=None,
    )

    try:
        await client._submit_order(command)

        assert initial_cloid in client._cloid_map
        assert client._cloid_map[initial_cloid] == ClientOrderId("O-CLOID-001")
        assert client._nt_to_cloid["O-CLOID-001"] == initial_cloid
        assert client._next_cloid == initial_cloid + 1
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_rejection_clears_cloid_map(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    order_client.place_order.side_effect = Exception("Insufficient margin")

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-REJ-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("90.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    command = SubmitOrder(
        trader_id=order.trader_id, strategy_id=order.strategy_id, order=order,
        command_id=TestIdStubs.uuid(), ts_init=0, position_id=None, client_id=None,
    )

    try:
        with patch.object(client, "generate_order_rejected") as mock_rejected:
            await client._submit_order(command)

            mock_rejected.assert_called_once()
            # Cloid map cleaned up on rejection
            assert all(v != ClientOrderId("O-REJ-001") for v in client._cloid_map.values())
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_non_bullet_symbol_rejected(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)

    # Create order with non-PERP suffix instrument
    from nautilus_trader.model.instruments import CryptoPerpetual as CP
    wrong_instrument_id = InstrumentId(Symbol("SOL-USDT"), instrument.venue)

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=wrong_instrument_id,
        client_order_id=ClientOrderId("O-WRONG-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("90.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    command = SubmitOrder(
        trader_id=order.trader_id, strategy_id=order.strategy_id, order=order,
        command_id=TestIdStubs.uuid(), ts_init=0, position_id=None, client_id=None,
    )

    await client._submit_order(command)

    order_client.place_order.assert_not_awaited()


# ── Cancel ────────────────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_cancel_order_calls_order_client(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-CXL-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("90.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)
    client._nt_to_cloid["O-CXL-001"] = 77

    command = CancelOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("85000001"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        await client._cancel_order(command)

        order_client.cancel_order.assert_awaited_once()
        call_kwargs = order_client.cancel_order.call_args.kwargs
        assert call_kwargs["symbol"] == "SOL-USD"
        assert call_kwargs["venue_order_id"] == 85000001
        assert call_kwargs["client_order_id"] == 77
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_order_not_in_cache_skips(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    command = CancelOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-UNKNOWN"),
        venue_order_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        await client._cancel_order(command)
        order_client.cancel_order.assert_not_awaited()
    finally:
        await client._disconnect()


# ── Modify ────────────────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_modify_order_calls_amend(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-MOD-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("90.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)
    cache.add_venue_order_id(order.client_order_id, VenueOrderId("85000001"))
    client._nt_to_cloid["O-MOD-001"] = 55

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("85000001"),
        quantity=Quantity.from_str("0.02"),
        price=Price.from_str("91.00"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        await client._modify_order(command)

        order_client.amend_order.assert_awaited_once()
        call_kwargs = order_client.amend_order.call_args.kwargs
        assert call_kwargs["symbol"] == "SOL-USD"
        assert call_kwargs["new_price"] == "91.00"
        assert call_kwargs["new_qty"] == "0.02"
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_not_in_cache_skips(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    command = ModifyOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-UNKNOWN"),
        venue_order_id=VenueOrderId("99"),
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("90.00"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        await client._modify_order(command)
        order_client.amend_order.assert_not_awaited()
    finally:
        await client._disconnect()


# ── Cancel all ────────────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_cancel_all_orders_for_instrument_calls_cancel_market(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    command = CancelAllOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        order_side=OrderSide.NO_ORDER_SIDE,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        await client._cancel_all_orders(command)

        order_client.cancel_market_orders.assert_awaited_once()
        call_kwargs = order_client.cancel_market_orders.call_args.kwargs
        assert call_kwargs["symbol"] == "SOL-USD"
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_all_orders_no_instrument_calls_cancel_all(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    # Simulate the exec engine calling _cancel_all_orders with no instrument_id
    # by calling the internal method directly with a patched command
    mock_command = MagicMock()
    mock_command.instrument_id = None

    try:
        await client._cancel_all_orders(mock_command)

        order_client.cancel_all_orders.assert_awaited_once()
        order_client.cancel_market_orders.assert_not_awaited()
    finally:
        await client._disconnect()


# ── Reports ───────────────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_generate_order_status_reports_parses_open_orders(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)

    sol_orders_json = json.dumps([{
        "orderId": 85000001,
        "clientOrderId": 42,
        "side": "BUY",
        "origQty": "0.01",
        "executedQty": "0",
        "price": "90.00",
        "type": "LIMIT",
        "updateTime": 1_000_000,
    }])
    # SOL-USD returns one order; BTC-USD returns empty
    http_client.open_orders_json = AsyncMock(side_effect=lambda addr, sym: (
        sol_orders_json if sym == "SOL-USD" else "[]"
    ))

    command = GenerateOrderStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        open_only=True,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    reports = await client.generate_order_status_reports(command)

    sol_reports = [r for r in reports if r.instrument_id == instrument.id]
    assert len(sol_reports) == 1
    assert sol_reports[0].venue_order_id == VenueOrderId("85000001")
    assert sol_reports[0].order_side.name == "BUY"


@pytest.mark.asyncio
async def test_generate_order_status_reports_returns_empty_on_http_error(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    http_client.open_orders_json = AsyncMock(side_effect=Exception("network error"))

    command = GenerateOrderStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        open_only=True,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    reports = await client.generate_order_status_reports(command)
    assert reports == []


@pytest.mark.asyncio
async def test_generate_order_status_report_found(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)

    open_orders_json = json.dumps([{
        "orderId": 85000001,
        "clientOrderId": 42,
        "side": "BUY",
        "origQty": "0.01",
        "executedQty": "0",
        "price": "90.00",
        "type": "LIMIT",
        "updateTime": 1_000_000,
    }])
    http_client.open_orders_json = AsyncMock(return_value=open_orders_json)

    command = GenerateOrderStatusReport(
        instrument_id=instrument.id,
        client_order_id=None,
        venue_order_id=VenueOrderId("85000001"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    report = await client.generate_order_status_report(command)

    assert report is not None
    assert report.venue_order_id == VenueOrderId("85000001")


@pytest.mark.asyncio
async def test_generate_order_status_report_not_found_returns_none(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    http_client.open_orders_json = AsyncMock(return_value="[]")

    command = GenerateOrderStatusReport(
        instrument_id=instrument.id,
        client_order_id=None,
        venue_order_id=VenueOrderId("99999"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    report = await client.generate_order_status_report(command)
    assert report is None


@pytest.mark.asyncio
async def test_generate_order_status_report_no_address_returns_none(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    order_client.account_address = None

    command = GenerateOrderStatusReport(
        instrument_id=instrument.id,
        client_order_id=None,
        venue_order_id=VenueOrderId("85000001"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    report = await client.generate_order_status_report(command)
    assert report is None


@pytest.mark.asyncio
async def test_generate_position_status_reports_parses_positions(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)

    account_json = json.dumps({
        "totalWalletBalance": "20000.00",
        "availableBalance": "19900.00",
        "positions": [{
            "symbol": "SOL-USD",
            "positionAmt": "0.50",
            "entryPrice": "89.00",
            "updateTime": 1_000_000,
        }],
    })
    http_client.account_json = AsyncMock(return_value=account_json)

    command = GeneratePositionStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    reports = await client.generate_position_status_reports(command)

    assert len(reports) == 1
    assert reports[0].instrument_id == instrument.id
    assert reports[0].position_side.name == "LONG"
    assert reports[0].quantity == Quantity.from_str("0.50")


@pytest.mark.asyncio
async def test_generate_position_status_reports_skips_zero_qty(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)

    account_json = json.dumps({
        "totalWalletBalance": "20000.00",
        "availableBalance": "19900.00",
        "positions": [{"symbol": "SOL-USD", "positionAmt": "0", "entryPrice": "0", "updateTime": 0}],
    })
    http_client.account_json = AsyncMock(return_value=account_json)

    command = GeneratePositionStatusReports(
        instrument_id=None, start=None, end=None,
        command_id=TestIdStubs.uuid(), ts_init=0,
    )

    reports = await client.generate_position_status_reports(command)
    assert reports == []


@pytest.mark.asyncio
async def test_generate_fill_reports_returns_empty(exec_client_builder, monkeypatch):
    from nautilus_trader.execution.messages import GenerateFillReports
    client, _, _, _ = exec_client_builder(monkeypatch)

    command = GenerateFillReports(
        instrument_id=None, venue_order_id=None, start=None, end=None,
        command_id=TestIdStubs.uuid(), ts_init=0,
    )

    reports = await client.generate_fill_reports(command)
    assert reports == []


# ── Batch cancel ──────────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_batch_cancel_groups_by_symbol(
    exec_client_builder, monkeypatch, instrument, btc_instrument, cache
):
    """Two SOL orders + one BTC order → one SOL transaction + one BTC transaction."""
    client, _, _, order_client = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    cache.add_instrument(btc_instrument)
    await client._connect()

    def _make_limit(coid_str, inst, side, price_str, qty_str):
        return LimitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=inst.id,
            client_order_id=ClientOrderId(coid_str),
            order_side=side,
            quantity=Quantity.from_str(qty_str),
            price=Price.from_str(price_str),
            init_id=TestIdStubs.uuid(),
            ts_init=0,
        )

    sol1 = _make_limit("O-SOL-001", instrument, OrderSide.BUY, "90.00", "0.01")
    sol2 = _make_limit("O-SOL-002", instrument, OrderSide.BUY, "89.00", "0.01")
    btc1 = _make_limit("O-BTC-001", btc_instrument, OrderSide.SELL, "80000.0", "0.0001")

    for o, vid, cid in [(sol1, "91000001", 10), (sol2, "91000002", 11), (btc1, "92000001", 20)]:
        cache.add_order(o, None)
        cache.add_venue_order_id(o.client_order_id, VenueOrderId(vid))
        client._cloid_map[cid] = o.client_order_id
        client._nt_to_cloid[o.client_order_id.value] = cid

    from nautilus_trader.execution.messages import BatchCancelOrders
    from nautilus_trader.execution.messages import CancelOrder

    cancels = [
        CancelOrder(
            trader_id=o.trader_id, strategy_id=o.strategy_id,
            instrument_id=o.instrument_id, client_order_id=o.client_order_id,
            venue_order_id=cache.venue_order_id(o.client_order_id),
            command_id=TestIdStubs.uuid(), ts_init=0, client_id=None,
        )
        for o in [sol1, sol2, btc1]
    ]
    command = BatchCancelOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        cancels=cancels,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        await client._batch_cancel_orders(command)

        # Should be called twice: once for SOL-USD, once for BTC-USD
        assert order_client.batch_cancel_orders.await_count == 2
        calls_by_symbol = {
            call.kwargs["symbol"]: call.kwargs["orders"]
            for call in order_client.batch_cancel_orders.call_args_list
        }
        assert "SOL-USD" in calls_by_symbol
        assert "BTC-USD" in calls_by_symbol
        assert len(calls_by_symbol["SOL-USD"]) == 2
        assert len(calls_by_symbol["BTC-USD"]) == 1
    finally:
        await client._disconnect()


# ── Reconnect monitor ─────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_reconnect_monitor_refreshes_account_on_reconnect(
    exec_client_builder, monkeypatch, cache
):
    """Monitor detects disconnected→connected transition and calls _update_account_state."""
    client, ws_client, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    address = client._order_client.account_address

    with patch.object(client, "_update_account_state", new_callable=AsyncMock) as mock_update:
        # Simulate: was disconnected, now reconnected
        ws_client.is_connected.return_value = True
        was_connected = False
        now_connected = ws_client.is_connected()
        if not was_connected and now_connected:
            await client._update_account_state(address)

        mock_update.assert_awaited_once_with(address)

    await client._disconnect()


@pytest.mark.asyncio
async def test_reconnect_monitor_no_refresh_when_stable(
    exec_client_builder, monkeypatch, cache
):
    """No account refresh when connection stays up."""
    client, ws_client, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    address = client._order_client.account_address

    with patch.object(client, "_update_account_state", new_callable=AsyncMock) as mock_update:
        ws_client.is_connected.return_value = True
        was_connected = True
        now_connected = ws_client.is_connected()
        if not was_connected and now_connected:
            await client._update_account_state(address)

        mock_update.assert_not_awaited()

    await client._disconnect()


# ── Cancel-replace amend tracking ─────────────────────────────────────────────

@pytest.mark.asyncio
async def test_modify_order_populates_pending_amend(
    exec_client_builder, monkeypatch, instrument, cache
):
    client, _, _, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-AMEND-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("90.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)
    cache.add_venue_order_id(order.client_order_id, VenueOrderId("85000001"))
    client._nt_to_cloid["O-AMEND-001"] = 55

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("85000001"),
        quantity=Quantity.from_str("0.02"),
        price=Price.from_str("89.00"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        await client._modify_order(command)
        assert "O-AMEND-001" in client._pending_amend_cloids
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_replace_amend_emits_order_updated(
    exec_client_builder, monkeypatch, instrument, cache
):
    """Simulates Bullet's cancel-replace amend: CANCELED (old) suppressed, NEW emits OrderUpdated."""
    client, _, _, _ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    client_order_id, _, order = _setup_order_in_cache(client, cache, instrument, cloid_int=42)

    # Mark the order as pending amend (as _modify_order would)
    client._pending_amend_cloids.add(client_order_id.value)

    with (
        patch.object(client, "generate_order_canceled") as mock_canceled,
        patch.object(client, "generate_order_accepted") as mock_accepted,
        patch.object(client, "generate_order_updated") as mock_updated,
    ):
        # Step 1: CANCELED fires for the old order — should be suppressed
        canceled_msg = _make_order_update("SOL-USD", 85000001, 42, "CANCELED")
        client._handle_msg(canceled_msg)

        mock_canceled.assert_not_called()
        # Cloid maps must remain intact for the replacement lookup
        assert 42 in client._cloid_map
        assert client_order_id.value in client._nt_to_cloid

        # Step 2: NEW fires for replacement with different venue_order_id, same cloid
        new_msg = _make_order_update(
            "SOL-USD", 85999999, 42, "NEW",
            price="89.00", qty="0.02",
        )
        client._handle_msg(new_msg)

        mock_accepted.assert_not_called()
        mock_updated.assert_called_once()
        call_kwargs = mock_updated.call_args.kwargs
        assert call_kwargs["client_order_id"] == client_order_id
        assert call_kwargs["venue_order_id"] == VenueOrderId("85999999")
        assert call_kwargs["venue_order_id_modified"] is True
        assert call_kwargs["price"] == Price.from_str("89.00")
        assert call_kwargs["quantity"] == Quantity.from_str("0.02")

    # Pending set cleared after the replacement is processed
    assert client_order_id.value not in client._pending_amend_cloids
