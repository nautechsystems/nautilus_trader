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
"""
Execution client for the dYdX v4 decentralized crypto exchange.

This client uses Rust-backed HTTP, WebSocket, and gRPC clients for order execution.

Supported order types:
  - MARKET: Immediate execution at best available price
  - LIMIT: Maker orders with optional post-only flag
  - STOP_MARKET: Triggered when price crosses trigger_price
  - STOP_LIMIT: Triggered stop with limit price
  - MARKET_IF_TOUCHED: Take profit market (triggers on price touch)
  - LIMIT_IF_TOUCHED: Take profit limit (triggers on price touch)

"""

import asyncio
import os

from nautilus_trader.adapters.dydx.config import DydxExecClientConfig
from nautilus_trader.adapters.dydx.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx.providers import DydxInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import nanos_to_secs
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.orders import LimitIfTouchedOrder
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketIfTouchedOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.model.orders import StopMarketOrder


def _get_expire_time_secs(order: Order) -> int | None:
    if hasattr(order, "expire_time_ns") and order.expire_time_ns:
        return int(nanos_to_secs(order.expire_time_ns))
    return None


class DydxExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the dYdX v4 decentralized crypto exchange.

    This client uses Rust-backed HTTP, WebSocket, and gRPC clients for order execution.
    Order submission uses the gRPC client for low-latency Cosmos SDK transactions.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.DydxHttpClient
        The dYdX HTTP client (Rust-backed).
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : DydxInstrumentProvider
        The instrument provider.
    config : DydxExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.DydxHttpClient,  # type: ignore[name-defined]
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: DydxInstrumentProvider,
        config: DydxExecClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or DYDX_VENUE.value),
            venue=DYDX_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._instrument_provider: DydxInstrumentProvider = instrument_provider

        # Configuration
        self._config = config
        self._subaccount = config.subaccount
        self._is_testnet = config.is_testnet
        self._log.info(f"{config.is_testnet=}", LogColor.BLUE)
        self._log.info(f"{config.subaccount=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)

        # HTTP API
        self._http_client = client

        # Resolve URLs
        ws_url = config.base_url_ws or nautilus_pyo3.get_dydx_ws_url(config.is_testnet)  # type: ignore[attr-defined]
        grpc_urls = config.base_url_grpc or nautilus_pyo3.get_dydx_grpc_urls(config.is_testnet)  # type: ignore[attr-defined]

        # Initialize gRPC and order submitter (created on connect)
        self._grpc_client: nautilus_pyo3.DydxGrpcClient | None = None  # type: ignore[name-defined]
        self._order_submitter: nautilus_pyo3.DydxOrderSubmitter | None = None  # type: ignore[name-defined]
        self._grpc_urls = grpc_urls

        # Bidirectional client order ID encoder (set from WS client in _connect)
        self._encoder: nautilus_pyo3.DydxClientOrderIdEncoder | None = None  # type: ignore[name-defined]

        # Order context for cancellation (client_id_u32 -> (tif_value, expire_time_ns))
        self._order_contexts: dict[int, tuple[int | None, int | None]] = {}

        # WebSocket API (private client for account updates)
        self._ws_client: nautilus_pyo3.DydxWebSocketClient | None = None  # type: ignore[name-defined]
        self._ws_url = ws_url

        # Account tracking
        self._wallet_address: str | None = None
        self._pyo3_account_id: nautilus_pyo3.AccountId | None = None

    @property
    def pyo3_account_id(self) -> nautilus_pyo3.AccountId:
        """
        Return the PyO3 account ID, caching it if not already created.
        """
        if self._pyo3_account_id is None:
            self._pyo3_account_id = nautilus_pyo3.AccountId(self.account_id.value)
        return self._pyo3_account_id

    async def _connect(self) -> None:
        # Load instruments
        await self._instrument_provider.initialize()

        # Fetch and cache instruments with full market params (needed for order quantization)
        await self._http_client.fetch_and_cache_instruments()

        # Initialize wallet from private key
        private_key = self._config.private_key
        if not private_key:
            env_var = "DYDX_TESTNET_PRIVATE_KEY" if self._is_testnet else "DYDX_PRIVATE_KEY"
            private_key = os.environ.get(env_var)

        if not private_key:
            self._log.error(
                f"No private key provided. Set via config or "
                f"{'DYDX_TESTNET_PRIVATE_KEY' if self._is_testnet else 'DYDX_PRIVATE_KEY'} env var",
            )
            return

        # Resolve wallet address: config → env var → derived from private key
        temp_wallet = nautilus_pyo3.DydxWallet.from_private_key(private_key)  # type: ignore[attr-defined]
        wallet_address = self._config.wallet_address
        if not wallet_address:
            wallet_env = (
                "DYDX_TESTNET_WALLET_ADDRESS" if self._is_testnet else "DYDX_WALLET_ADDRESS"
            )
            wallet_address = os.environ.get(wallet_env)
        if not wallet_address:
            wallet_address = temp_wallet.address()
        self._wallet_address = wallet_address

        # Set account ID based on wallet address
        account_id = AccountId(f"{DYDX_VENUE.value}-{self._wallet_address}-{self._subaccount}")
        self._set_account_id(account_id)

        self._log.info(f"Wallet address: {self._wallet_address}", LogColor.BLUE)

        # Create gRPC client
        if isinstance(self._grpc_urls, str):
            # Single URL provided via config
            self._grpc_client = await nautilus_pyo3.DydxGrpcClient.connect_with_fallback(  # type: ignore[attr-defined]
                [self._grpc_urls],
            )
        else:
            # List of URLs from get_grpc_urls
            self._grpc_client = await nautilus_pyo3.DydxGrpcClient.connect_with_fallback(  # type: ignore[attr-defined]
                self._grpc_urls,
            )

        # Create order submitter (wallet owned internally)
        chain_id = "dydx-testnet-4" if self._is_testnet else "dydx-mainnet-1"
        self._order_submitter = nautilus_pyo3.DydxOrderSubmitter(  # type: ignore[attr-defined]
            grpc_client=self._grpc_client,
            http_client=self._http_client,
            private_key=private_key,
            wallet_address=self._wallet_address,
            subaccount_number=self._subaccount,
            chain_id=chain_id,
            grpc_rate_limit_per_second=self._config.grpc_rate_limit_per_second,
        )

        # Resolve authenticators for permissioned key trading
        await self._order_submitter.resolve_authenticators()

        # Connect private WebSocket for account updates
        self._ws_client = nautilus_pyo3.DydxWebSocketClient.new_private(  # type: ignore[attr-defined]
            url=self._ws_url,
            private_key=private_key,
            authenticator_ids=self._config.authenticator_ids or [],
            account_id=nautilus_pyo3.AccountId(account_id.value),
            heartbeat=20,
        )

        self._encoder = self._ws_client.encoder()
        self._ws_client.share_instrument_cache(self._http_client)

        instruments = self._instrument_provider.instruments_pyo3()
        await self._ws_client.connect(
            instruments=instruments,
            callback=self._handle_msg,
        )

        # Wait for connection
        await self._ws_client.wait_until_active(timeout_secs=30.0)
        self._log.info(f"Connected to WebSocket {self._ws_client.py_url}", LogColor.BLUE)

        # Subscribe to account updates
        await self._ws_client.subscribe_subaccount(
            address=self._wallet_address,
            subaccount_number=self._subaccount,
        )

        # Subscribe to block height for order timing
        await self._ws_client.subscribe_block_height()

        # Fetch initial block height via gRPC and feed to submitter
        try:
            initial_height = await self._grpc_client.latest_block_height()
            self._order_submitter.set_block_height(initial_height)
            self._log.info(f"Initial block height: {initial_height}", LogColor.BLUE)
        except Exception as e:
            self._log.warning(f"Failed to fetch initial block height: {e}")

        await self._await_account_registered(timeout_secs=30.0)

    async def _disconnect(self) -> None:
        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown websocket
        if self._ws_client is not None and not self._ws_client.is_closed():
            self._log.debug("Disconnecting WebSocket")
            await self._ws_client.disconnect()
            self._log.debug("Disconnected from WebSocket")

    def _handle_msg(self, raw: object) -> None:
        try:
            if isinstance(raw, dict):
                self._handle_dict_message(raw)
            elif isinstance(raw, nautilus_pyo3.AccountState):
                self._handle_account_state(raw)
            elif isinstance(raw, nautilus_pyo3.OrderStatusReport):
                report = OrderStatusReport.from_pyo3(raw)
                self._cleanup_order_context(report)
                self._send_order_status_report(report)
            elif isinstance(raw, nautilus_pyo3.FillReport):
                report = FillReport.from_pyo3(raw)
                self._send_fill_report(report)
            elif isinstance(raw, nautilus_pyo3.PositionStatusReport):
                report = PositionStatusReport.from_pyo3(raw)
                self._send_position_status_report(report)
            else:
                self._log.warning(f"Ignoring message of type {type(raw).__name__}")
        except Exception as e:
            self._log.error(f"Error handling WebSocket message: {e}")

    def _handle_dict_message(self, msg: dict) -> None:
        msg_type = msg.get("type")
        if msg_type == "block_height":
            self._handle_block_height(msg)
        elif msg_type == "subaccounts_channel_data":
            pass  # Handled by Rust WS handler, parsed into typed reports

    def _handle_block_height(self, msg: dict) -> None:
        height = msg.get("height")
        time_str = msg.get("time")
        if height is not None and self._order_submitter is not None:
            try:
                self._order_submitter.record_block(height, time_str)
            except Exception as e:
                self._log.warning(f"Failed to record block height: {e}")

    def _handle_account_state(self, msg: nautilus_pyo3.AccountState) -> None:
        account_state = AccountState.from_dict(msg.to_dict())
        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=account_state.is_reported,
            ts_event=account_state.ts_event,
        )

    async def _query_account(self, command: QueryAccount) -> None:
        if not self._wallet_address:
            self._log.warning("Cannot query account: wallet not initialized")
            return

        try:
            pyo3_account_state = await self._http_client.request_account_state(
                address=self._wallet_address,
                subaccount_number=self._subaccount,
                account_id=self.pyo3_account_id,
            )
            account_state = AccountState.from_dict(pyo3_account_state.to_dict())
            self.generate_account_state(
                balances=account_state.balances,
                margins=account_state.margins,
                reported=account_state.is_reported,
                ts_event=account_state.ts_event,
            )
        except Exception as e:
            self._log.error(f"Failed to query account state: {e}")

    _TERMINAL_STATUSES = frozenset(
        {
            OrderStatus.FILLED,
            OrderStatus.CANCELED,
            OrderStatus.EXPIRED,
            OrderStatus.REJECTED,
            OrderStatus.DENIED,
        },
    )

    def _cleanup_order_context(self, report: OrderStatusReport) -> None:
        if report.order_status not in self._TERMINAL_STATUSES:
            return
        if self._encoder is None or report.client_order_id is None:
            return
        client_order_id_u32, _ = self._encoder.encode(str(report.client_order_id))
        if self._order_contexts.pop(client_order_id_u32, None) is not None:
            self._log.debug(
                f"Cleaned up order context for {report.client_order_id} "
                f"(status={report.order_status.name})",
            )

    async def _submit_order(self, command: SubmitOrder) -> None:
        if self._order_submitter is None:
            self.generate_order_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.order.instrument_id,
                client_order_id=command.order.client_order_id,
                reason="Order submitter not initialized - connect first",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Check block height is available for short-term orders
        if self._order_submitter.get_block_height() == 0:
            reason = "Block height not initialized"
            self._log.warning(
                f"Cannot submit order {command.order.client_order_id}: {reason}",
                LogColor.YELLOW,
            )
            self.generate_order_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.order.instrument_id,
                client_order_id=command.order.client_order_id,
                reason=reason,
                ts_event=self._clock.timestamp_ns(),
            )
            return

        order = command.order
        instrument = self._instrument_provider.find(order.instrument_id)

        if instrument is None:
            self.generate_order_rejected(
                strategy_id=command.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Instrument {order.instrument_id} not found",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Encode client_order_id to (u32, u32) pair using bidirectional encoder
        assert self._encoder is not None
        client_order_id_u32, client_metadata = self._encoder.encode(str(order.client_order_id))

        # Register order context for cancellation
        tif_value = order.time_in_force.value if hasattr(order, "time_in_force") else None
        expire_ns = order.expire_time_ns if hasattr(order, "expire_time_ns") else None
        self._order_contexts[client_order_id_u32] = (tif_value, expire_ns)

        self._log.debug(f"Submit {order}")

        # Generate OrderSubmitted event before dispatch
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        try:
            await self._dispatch_order(order, client_order_id_u32, client_metadata)
            self._log.debug(f"Submitted order {order.client_order_id}")
        except Exception as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Order submission failed: {e}",
                ts_event=self._clock.timestamp_ns(),
            )

    async def _dispatch_order(
        self,
        order: Order,
        client_order_id_u32: int,
        client_metadata: int,
    ) -> None:
        dispatch_map = {
            OrderType.MARKET: self._submit_market_order,
            OrderType.LIMIT: self._submit_limit_order,
            OrderType.STOP_MARKET: self._submit_stop_market_order,
            OrderType.STOP_LIMIT: self._submit_stop_limit_order,
            OrderType.MARKET_IF_TOUCHED: self._submit_take_profit_market_order,
            OrderType.LIMIT_IF_TOUCHED: self._submit_take_profit_limit_order,
        }

        handler = dispatch_map.get(order.order_type)
        if handler is None:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Unsupported order type: {order.order_type}",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        await handler(order, client_order_id_u32, client_metadata)

    async def _submit_market_order(
        self,
        order: MarketOrder,
        client_order_id_u32: int,
        client_metadata: int,
    ) -> None:
        assert self._order_submitter is not None
        await self._order_submitter.submit_market_order(
            instrument_id=str(order.instrument_id),
            client_order_id=client_order_id_u32,
            side=order.side.value,
            quantity=str(order.quantity),
            client_metadata=client_metadata,
        )

    async def _submit_limit_order(
        self,
        order: LimitOrder,
        client_order_id_u32: int,
        client_metadata: int,
    ) -> None:
        assert self._order_submitter is not None
        tif_value = order.time_in_force.value

        self._log.debug(
            f"Submitting limit order: "
            f"price={order.price}, qty={order.quantity}, tif={order.time_in_force}",
        )

        await self._order_submitter.submit_limit_order(
            instrument_id=str(order.instrument_id),
            client_order_id=client_order_id_u32,
            side=order.side.value,
            price=str(order.price),
            quantity=str(order.quantity),
            time_in_force=tif_value,
            post_only=order.is_post_only,
            reduce_only=order.is_reduce_only,
            expire_time=_get_expire_time_secs(order),
            client_metadata=client_metadata,
        )

    async def _submit_stop_market_order(
        self,
        order: StopMarketOrder,
        client_order_id_u32: int,
        client_metadata: int,
    ) -> None:
        assert self._order_submitter is not None
        await self._order_submitter.submit_stop_market_order(
            instrument_id=str(order.instrument_id),
            client_order_id=client_order_id_u32,
            side=order.side.value,
            trigger_price=str(order.trigger_price),
            quantity=str(order.quantity),
            reduce_only=order.is_reduce_only,
            expire_time=_get_expire_time_secs(order),
            client_metadata=client_metadata,
        )

    async def _submit_stop_limit_order(
        self,
        order: StopLimitOrder,
        client_order_id_u32: int,
        client_metadata: int,
    ) -> None:
        assert self._order_submitter is not None
        tif_value = order.time_in_force.value

        await self._order_submitter.submit_stop_limit_order(
            instrument_id=str(order.instrument_id),
            client_order_id=client_order_id_u32,
            side=order.side.value,
            trigger_price=str(order.trigger_price),
            limit_price=str(order.price),
            quantity=str(order.quantity),
            time_in_force=tif_value,
            post_only=order.is_post_only,
            reduce_only=order.is_reduce_only,
            expire_time=_get_expire_time_secs(order),
            client_metadata=client_metadata,
        )

    async def _submit_take_profit_market_order(
        self,
        order: MarketIfTouchedOrder,
        client_order_id_u32: int,
        client_metadata: int,
    ) -> None:
        assert self._order_submitter is not None
        await self._order_submitter.submit_take_profit_market_order(
            instrument_id=str(order.instrument_id),
            client_order_id=client_order_id_u32,
            side=order.side.value,
            trigger_price=str(order.trigger_price),
            quantity=str(order.quantity),
            reduce_only=order.is_reduce_only,
            expire_time=_get_expire_time_secs(order),
            client_metadata=client_metadata,
        )

    async def _submit_take_profit_limit_order(
        self,
        order: LimitIfTouchedOrder,
        client_order_id_u32: int,
        client_metadata: int,
    ) -> None:
        assert self._order_submitter is not None
        tif_value = order.time_in_force.value

        await self._order_submitter.submit_take_profit_limit_order(
            instrument_id=str(order.instrument_id),
            client_order_id=client_order_id_u32,
            side=order.side.value,
            trigger_price=str(order.trigger_price),
            limit_price=str(order.price),
            quantity=str(order.quantity),
            time_in_force=tif_value,
            post_only=order.is_post_only,
            reduce_only=order.is_reduce_only,
            expire_time=_get_expire_time_secs(order),
            client_metadata=client_metadata,
        )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        # Submit orders individually for now
        for order in command.order_list.orders:
            submit_cmd = SubmitOrder(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                order=order,
                position_id=command.position_id,
                command_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )
            await self._submit_order(submit_cmd)

    async def _modify_order(self, command: ModifyOrder) -> None:
        # dYdX doesn't support order modification, reject
        self._log.warning("dYdX does not support order modification")
        self.generate_order_modify_rejected(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            reason="Order modification not supported by dYdX",
            ts_event=self._clock.timestamp_ns(),
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        if self._order_submitter is None:
            self.generate_order_cancel_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason="Order submitter not initialized - connect first",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Get the order from cache to get instrument_id
        order = self._cache.order(command.client_order_id)
        if order is None:
            self.generate_order_cancel_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=f"Order {command.client_order_id} not found in cache",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Encode client_order_id using bidirectional encoder
        assert self._encoder is not None
        client_order_id_u32, _ = self._encoder.encode(str(command.client_order_id))

        # Get order context for time_in_force/expire_time_ns
        tif_value, expire_ns = self._order_contexts.get(client_order_id_u32, (None, None))

        try:
            await self._order_submitter.cancel_order(
                instrument_id=str(order.instrument_id),
                client_order_id=client_order_id_u32,
                time_in_force=tif_value,
                expire_time_ns=expire_ns,
            )
            self._log.debug(f"Cancelled order {command.client_order_id}")
        except Exception as e:
            self.generate_order_cancel_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=f"Order cancellation failed: {e}",
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        if self._order_submitter is None:
            self._log.error("Order submitter not initialized - connect first")
            return

        # Get all open orders from cache
        if command.instrument_id:
            open_orders = self._cache.orders_open(instrument_id=command.instrument_id)
        else:
            open_orders = self._cache.orders_open(venue=self.venue)

        if not open_orders:
            self._log.info("No open orders to cancel")
            return

        # Collect all open orders into batch list
        assert self._encoder is not None
        batch = []
        for order in open_orders:
            client_order_id_u32, _ = self._encoder.encode(str(order.client_order_id))
            if client_order_id_u32 not in self._order_contexts:
                self._log.debug(
                    f"Skipping cancel for {order.client_order_id}: "
                    "order context already cleaned up (terminal)",
                )
                continue
            tif_value, expire_ns = self._order_contexts[client_order_id_u32]
            batch.append((str(order.instrument_id), client_order_id_u32, tif_value, expire_ns))

        if batch:
            try:
                await self._order_submitter.cancel_orders_batch(batch)
                self._log.info(
                    f"Batch cancelled {len(batch)} orders in single transaction"
                    f" for {command.instrument_id or 'all instruments'}",
                )
            except Exception as e:
                self._log.error(f"Batch cancel failed: {e}")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        if self._order_submitter is None:
            self._log.error("Order submitter not initialized - connect first")
            return

        if not command.cancels:
            self._log.info("No orders to cancel in batch")
            return

        # Collect all orders into batch list
        assert self._encoder is not None
        batch = []
        for cancel in command.cancels:
            order = self._cache.order(cancel.client_order_id)
            if order is None:
                self._log.warning(
                    f"Order {cancel.client_order_id} not found in cache, skipping",
                )
                continue
            client_order_id_u32, _ = self._encoder.encode(str(cancel.client_order_id))
            if client_order_id_u32 not in self._order_contexts:
                self._log.debug(
                    f"Skipping cancel for {cancel.client_order_id}: "
                    "order context already cleaned up (terminal)",
                )
                continue
            tif_value, expire_ns = self._order_contexts[client_order_id_u32]
            batch.append((str(order.instrument_id), client_order_id_u32, tif_value, expire_ns))

        if batch:
            try:
                await self._order_submitter.cancel_orders_batch(batch)
                self._log.info(f"Batch cancelled {len(batch)} orders in single transaction")
            except Exception as e:
                self._log.error(f"Batch cancel failed: {e}")

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        """
        Generate a single order status report by searching for the specified order.
        """
        reports = await self.generate_order_status_reports(
            GenerateOrderStatusReports(
                instrument_id=command.instrument_id,
                start=None,
                end=None,
                open_only=False,
                command_id=command.id,
                ts_init=command.ts_init,
            ),
        )

        # Search for matching order by client_order_id or venue_order_id
        for report in reports:
            if command.client_order_id and report.client_order_id == command.client_order_id:
                return report
            if command.venue_order_id and report.venue_order_id == command.venue_order_id:
                return report

        return None

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        """
        Generate order status reports for the configured subaccount.
        """
        if not self._wallet_address:
            self._log.warning("Cannot generate order reports: wallet not initialized")
            return []

        self._log.debug(
            f"Requesting OrderStatusReports"
            f" {repr(command.instrument_id) if command.instrument_id else ''}"
            " ...",
        )

        reports: list[OrderStatusReport] = []

        try:
            pyo3_instrument_id = None
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            pyo3_reports = await self._http_client.request_order_status_reports(
                address=self._wallet_address,
                subaccount_number=self._subaccount,
                account_id=self.pyo3_account_id,
                instrument_id=pyo3_instrument_id,
            )

            for pyo3_report in pyo3_reports:
                report = OrderStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.NORMAL)
                reports.append(report)
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "OrderStatusReports")

        self._log_report_receipt(
            len(reports),
            "OrderStatusReport",
            command.log_receipt_level,
        )

        return reports

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        """
        Generate fill reports for the configured subaccount.
        """
        if not self._wallet_address:
            self._log.warning("Cannot generate fill reports: wallet not initialized")
            return []

        self._log.debug(
            f"Requesting FillReports"
            f" {repr(command.instrument_id) if command.instrument_id else ''}"
            " ...",
        )

        reports: list[FillReport] = []

        try:
            pyo3_instrument_id = None
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            pyo3_reports = await self._http_client.request_fill_reports(
                address=self._wallet_address,
                subaccount_number=self._subaccount,
                account_id=self.pyo3_account_id,
                instrument_id=pyo3_instrument_id,
            )

            for pyo3_report in pyo3_reports:
                report = FillReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.NORMAL)
                reports.append(report)
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "FillReports")

        self._log_report_receipt(len(reports), "FillReport", LogLevel.INFO)

        return reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        """
        Generate position status reports for the configured subaccount.
        """
        if not self._wallet_address:
            self._log.warning("Cannot generate position reports: wallet not initialized")
            return []

        self._log.debug(
            f"Requesting PositionStatusReports"
            f" {repr(command.instrument_id) if command.instrument_id else ''}"
            " ...",
        )

        reports: list[PositionStatusReport] = []

        try:
            pyo3_instrument_id = None
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            pyo3_reports = await self._http_client.request_position_status_reports(
                address=self._wallet_address,
                subaccount_number=self._subaccount,
                account_id=self.pyo3_account_id,
                instrument_id=pyo3_instrument_id,
            )

            for pyo3_report in pyo3_reports:
                report = PositionStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.NORMAL)
                reports.append(report)
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "PositionStatusReports")

        self._log_report_receipt(
            len(reports),
            "PositionStatusReport",
            command.log_receipt_level,
        )

        return reports
