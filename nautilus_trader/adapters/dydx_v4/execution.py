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
import hashlib

from nautilus_trader.adapters.dydx_v4.common.urls import get_grpc_urls
from nautilus_trader.adapters.dydx_v4.common.urls import get_ws_url
from nautilus_trader.adapters.dydx_v4.config import DYDXv4ExecClientConfig
from nautilus_trader.adapters.dydx_v4.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx_v4.providers import DYDXv4InstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
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
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orders import LimitIfTouchedOrder
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketIfTouchedOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.model.orders import StopMarketOrder


def _client_order_id_to_u32(client_order_id: ClientOrderId) -> int:
    """
    Hash a ClientOrderId string to a u32 for dYdX order submission.

    dYdX requires a u32 client_id for orders. We use a hash of the string to ensure
    uniqueness while fitting in the u32 range.

    """
    digest = hashlib.sha256(client_order_id.value.encode()).digest()
    return int.from_bytes(digest[:4], byteorder="big")


def _get_expire_time_secs(order: Order) -> int | None:
    """
    Extract expire_time from an order and convert to seconds.

    dYdX conditional orders require expire_time in Unix seconds. Returns None if the
    order has no expiry time set.

    """
    if hasattr(order, "expire_time_ns") and order.expire_time_ns:
        return int(nanos_to_secs(order.expire_time_ns))
    return None


class DYDXv4ExecutionClient(LiveExecutionClient):
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
    instrument_provider : DYDXv4InstrumentProvider
        The instrument provider.
    config : DYDXv4ExecClientConfig
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
        instrument_provider: DYDXv4InstrumentProvider,
        config: DYDXv4ExecClientConfig,
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

        self._instrument_provider: DYDXv4InstrumentProvider = instrument_provider

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
        ws_url = config.base_url_ws or get_ws_url(is_testnet=config.is_testnet)
        grpc_urls = config.base_url_grpc or get_grpc_urls(is_testnet=config.is_testnet)

        # Initialize wallet and gRPC client from mnemonic
        self._wallet: nautilus_pyo3.DydxWallet | None = None  # type: ignore[name-defined]
        self._grpc_client: nautilus_pyo3.DydxGrpcClient | None = None  # type: ignore[name-defined]
        self._order_submitter: nautilus_pyo3.DydxOrderSubmitter | None = None  # type: ignore[name-defined]
        self._grpc_urls = grpc_urls

        # WebSocket API (private client for account updates)
        # Note: The private WebSocket requires mnemonic for authentication
        self._ws_client: nautilus_pyo3.DydxWebSocketClient | None = None  # type: ignore[name-defined]
        self._ws_url = ws_url

        # Account tracking
        self._wallet_address: str | None = None
        self._block_height: int = 0
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

        # Initialize wallet from mnemonic
        mnemonic = self._config.mnemonic
        if not mnemonic:
            # Try to get from environment
            import os

            env_var = "DYDX_TESTNET_MNEMONIC" if self._is_testnet else "DYDX_MNEMONIC"
            mnemonic = os.environ.get(env_var)

        if not mnemonic:
            self._log.error(
                f"No mnemonic provided. Set via config or "
                f"{'DYDX_TESTNET_MNEMONIC' if self._is_testnet else 'DYDX_MNEMONIC'} env var",
            )
            return

        # Create wallet
        self._wallet = nautilus_pyo3.DydxWallet.from_mnemonic(  # type: ignore[attr-defined]
            mnemonic=mnemonic,
        )
        self._wallet_address = self._wallet.address()

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

        # Create order submitter
        chain_id = "dydx-testnet-4" if self._is_testnet else "dydx-mainnet-1"
        self._order_submitter = nautilus_pyo3.DydxOrderSubmitter(  # type: ignore[attr-defined]
            grpc_client=self._grpc_client,
            http_client=self._http_client,
            wallet_address=self._wallet_address,
            subaccount_number=self._subaccount,
            chain_id=chain_id,
        )

        # Connect private WebSocket for account updates
        self._ws_client = nautilus_pyo3.DydxWebSocketClient.new_private(  # type: ignore[attr-defined]
            url=self._ws_url,
            mnemonic=mnemonic,
            account_index=self._subaccount,
            authenticator_ids=[],
            account_id=nautilus_pyo3.AccountId(account_id.value),
            heartbeat=20,
        )

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

        # Fetch initial block height via gRPC for order submission
        try:
            self._block_height = await self._grpc_client.latest_block_height()
            self._log.info(f"Initial block height: {self._block_height}", LogColor.BLUE)
        except Exception as e:
            self._log.warning(f"Failed to fetch initial block height: {e}")
            self._block_height = 0

    async def _disconnect(self) -> None:
        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown websocket
        if self._ws_client is not None and not self._ws_client.is_closed():
            self._log.info("Disconnecting WebSocket")
            await self._ws_client.disconnect()
            self._log.info("Disconnected from WebSocket", LogColor.BLUE)

    def _handle_msg(self, capsule: object) -> None:
        try:
            data = capsule_to_data(capsule)
            self._handle_data(data)
        except Exception as e:
            self._log.error(f"Error handling WebSocket message: {e}")

    # -- COMMANDS ---------------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        if self._order_submitter is None or self._wallet is None:
            self._generate_order_rejected(
                command.order.client_order_id,
                "Order submitter not initialized - connect first",
            )
            return

        order = command.order
        instrument = self._instrument_provider.find(order.instrument_id)

        if instrument is None:
            self._generate_order_rejected(
                order.client_order_id,
                f"Instrument {order.instrument_id} not found",
            )
            return

        # Cache instruments in HTTP client for order quantization
        instruments_pyo3 = self._instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.cache_instrument(inst)

        # Generate u32 client_order_id from the string
        client_order_id_u32 = _client_order_id_to_u32(order.client_order_id)

        self._log.info(f"Submit {order}", LogColor.BLUE)

        try:
            await self._dispatch_order(order, client_order_id_u32)
            self._log.debug(f"Submitted order {order.client_order_id}")
        except Exception as e:
            self._generate_order_rejected(
                order.client_order_id,
                f"Order submission failed: {e}",
            )

    async def _dispatch_order(self, order: Order, client_order_id_u32: int) -> None:
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
            self._generate_order_rejected(
                order.client_order_id,
                f"Unsupported order type: {order.order_type}",
            )
            return

        await handler(order, client_order_id_u32)

    async def _submit_market_order(self, order: MarketOrder, client_order_id_u32: int) -> None:
        assert self._order_submitter is not None  # Checked in _submit_order
        assert self._wallet is not None  # Checked in _submit_order
        await self._order_submitter.submit_market_order(
            wallet=self._wallet,
            instrument_id=str(order.instrument_id),
            client_order_id=client_order_id_u32,
            side=order.side.value,
            quantity=str(order.quantity),
            block_height=self._block_height,
        )

    async def _submit_limit_order(self, order: LimitOrder, client_order_id_u32: int) -> None:
        assert self._order_submitter is not None  # Checked in _submit_order
        assert self._wallet is not None  # Checked in _submit_order
        # Convert TimeInForce enum to int value
        tif_value = order.time_in_force.value

        await self._order_submitter.submit_limit_order(
            wallet=self._wallet,
            instrument_id=str(order.instrument_id),
            client_order_id=client_order_id_u32,
            side=order.side.value,
            price=str(order.price),
            quantity=str(order.quantity),
            time_in_force=tif_value,
            post_only=order.is_post_only,
            reduce_only=order.is_reduce_only,
            block_height=self._block_height,
            expire_time=_get_expire_time_secs(order),
        )

    async def _submit_stop_market_order(
        self,
        order: StopMarketOrder,
        client_order_id_u32: int,
    ) -> None:
        assert self._order_submitter is not None  # Checked in _submit_order
        assert self._wallet is not None  # Checked in _submit_order
        await self._order_submitter.submit_stop_market_order(
            wallet=self._wallet,
            instrument_id=str(order.instrument_id),
            client_order_id=client_order_id_u32,
            side=order.side.value,
            trigger_price=str(order.trigger_price),
            quantity=str(order.quantity),
            reduce_only=order.is_reduce_only,
            expire_time=_get_expire_time_secs(order),
        )

    async def _submit_stop_limit_order(
        self,
        order: StopLimitOrder,
        client_order_id_u32: int,
    ) -> None:
        assert self._order_submitter is not None  # Checked in _submit_order
        assert self._wallet is not None  # Checked in _submit_order
        tif_value = order.time_in_force.value

        await self._order_submitter.submit_stop_limit_order(
            wallet=self._wallet,
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
        )

    async def _submit_take_profit_market_order(
        self,
        order: MarketIfTouchedOrder,
        client_order_id_u32: int,
    ) -> None:
        assert self._order_submitter is not None  # Checked in _submit_order
        assert self._wallet is not None  # Checked in _submit_order
        await self._order_submitter.submit_take_profit_market_order(
            wallet=self._wallet,
            instrument_id=str(order.instrument_id),
            client_order_id=client_order_id_u32,
            side=order.side.value,
            trigger_price=str(order.trigger_price),
            quantity=str(order.quantity),
            reduce_only=order.is_reduce_only,
            expire_time=_get_expire_time_secs(order),
        )

    async def _submit_take_profit_limit_order(
        self,
        order: LimitIfTouchedOrder,
        client_order_id_u32: int,
    ) -> None:
        assert self._order_submitter is not None  # Checked in _submit_order
        assert self._wallet is not None  # Checked in _submit_order
        tif_value = order.time_in_force.value

        await self._order_submitter.submit_take_profit_limit_order(
            wallet=self._wallet,
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
        self._generate_order_modify_rejected(
            command.client_order_id,
            "Order modification not supported by dYdX",
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        if self._order_submitter is None or self._wallet is None:
            self._generate_order_cancel_rejected(
                command.client_order_id,
                "Order submitter not initialized - connect first",
            )
            return

        # Get the order from cache to get instrument_id
        order = self._cache.order(command.client_order_id)
        if order is None:
            self._generate_order_cancel_rejected(
                command.client_order_id,
                f"Order {command.client_order_id} not found in cache",
            )
            return

        # Generate u32 client_order_id from the string (same as submit)
        client_order_id_u32 = _client_order_id_to_u32(command.client_order_id)

        try:
            await self._order_submitter.cancel_order(
                wallet=self._wallet,
                instrument_id=str(order.instrument_id),
                client_order_id=client_order_id_u32,
                block_height=self._block_height,
            )
            self._log.debug(f"Cancelled order {command.client_order_id}")
        except Exception as e:
            self._generate_order_cancel_rejected(
                command.client_order_id,
                f"Order cancellation failed: {e}",
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        if self._order_submitter is None or self._wallet is None:
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

        # Build batch cancel list: (instrument_id, client_order_id_u32)
        cancel_list = []
        for order in open_orders:
            client_order_id_u32 = _client_order_id_to_u32(order.client_order_id)
            cancel_list.append((str(order.instrument_id), client_order_id_u32))

        try:
            await self._order_submitter.cancel_orders_batch(
                wallet=self._wallet,
                orders=cancel_list,
                block_height=self._block_height,
            )
            self._log.debug(
                f"Cancelled {len(cancel_list)} orders for "
                f"{command.instrument_id or 'all instruments'}",
            )
        except Exception as e:
            self._log.error(f"Cancel all orders failed: {e}")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        if self._order_submitter is None or self._wallet is None:
            self._log.error("Order submitter not initialized - connect first")
            return

        if not command.cancels:
            self._log.info("No orders to cancel in batch")
            return

        # Build batch cancel list: (instrument_id, client_order_id_u32)
        cancel_list = []
        for cancel in command.cancels:
            # Get the order from cache to get instrument_id
            order = self._cache.order(cancel.client_order_id)
            if order is None:
                self._log.warning(
                    f"Order {cancel.client_order_id} not found in cache, skipping",
                )
                continue
            client_order_id_u32 = _client_order_id_to_u32(cancel.client_order_id)
            cancel_list.append((str(order.instrument_id), client_order_id_u32))

        if not cancel_list:
            self._log.warning("No valid orders to cancel in batch")
            return

        try:
            await self._order_submitter.cancel_orders_batch(
                wallet=self._wallet,
                orders=cancel_list,
                block_height=self._block_height,
            )
            self._log.debug(f"Batch cancelled {len(cancel_list)} orders")
        except Exception as e:
            self._log.error(f"Batch cancel orders failed: {e}")

    # -- REPORTS ----------------------------------------------------------------------------------

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
                command_id=command.command_id,
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
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except ValueError as e:
            if "request canceled" in str(e).lower():
                self._log.debug("OrderStatusReports request cancelled during shutdown")
            else:
                self._log.exception("Failed to generate OrderStatusReports", e)
        except Exception as e:
            self._log.exception("Failed to generate OrderStatusReports", e)

        self._log.info(f"Received {len(reports)} OrderStatusReport(s)")

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
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except ValueError as e:
            if "request canceled" in str(e).lower():
                self._log.debug("FillReports request cancelled during shutdown")
            else:
                self._log.exception("Failed to generate FillReports", e)
        except Exception as e:
            self._log.exception("Failed to generate FillReports", e)

        self._log.info(f"Received {len(reports)} FillReport(s)")

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
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except ValueError as e:
            if "request canceled" in str(e).lower():
                self._log.debug("PositionStatusReports request cancelled during shutdown")
            else:
                self._log.exception("Failed to generate PositionStatusReports", e)
        except Exception as e:
            self._log.exception("Failed to generate PositionStatusReports", e)

        self._log.info(f"Received {len(reports)} PositionStatusReport(s)")

        return reports

    # -- HELPERS ----------------------------------------------------------------------------------

    def _generate_order_rejected(
        self,
        client_order_id: ClientOrderId,
        reason: str,
    ) -> None:
        self._log.error(f"Order rejected: {reason}")
        event = OrderRejected(
            trader_id=self.trader_id,
            strategy_id=self._cache.strategy_id_for_order(client_order_id)
            or self.trader_id.get_strategy_id(),
            instrument_id=InstrumentId.from_str("UNKNOWN.DYDX"),
            client_order_id=client_order_id,
            account_id=self.account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=self._clock.timestamp_ns(),
            ts_init=self._clock.timestamp_ns(),
        )
        self._send_order_event(event)

    def _generate_order_modify_rejected(
        self,
        client_order_id: ClientOrderId,
        reason: str,
    ) -> None:
        self._log.error(f"Order modify rejected: {reason}")
        from nautilus_trader.model.events import OrderModifyRejected

        event = OrderModifyRejected(
            trader_id=self.trader_id,
            strategy_id=self._cache.strategy_id_for_order(client_order_id)
            or self.trader_id.get_strategy_id(),
            instrument_id=InstrumentId.from_str("UNKNOWN.DYDX"),
            client_order_id=client_order_id,
            venue_order_id=None,
            account_id=self.account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=self._clock.timestamp_ns(),
            ts_init=self._clock.timestamp_ns(),
        )
        self._send_order_event(event)

    def _generate_order_cancel_rejected(
        self,
        client_order_id: ClientOrderId,
        reason: str,
    ) -> None:
        self._log.error(f"Order cancel rejected: {reason}")
        event = OrderCancelRejected(
            trader_id=self.trader_id,
            strategy_id=self._cache.strategy_id_for_order(client_order_id)
            or self.trader_id.get_strategy_id(),
            instrument_id=InstrumentId.from_str("UNKNOWN.DYDX"),
            client_order_id=client_order_id,
            venue_order_id=None,
            account_id=self.account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=self._clock.timestamp_ns(),
            ts_init=self._clock.timestamp_ns(),
        )
        self._send_order_event(event)
