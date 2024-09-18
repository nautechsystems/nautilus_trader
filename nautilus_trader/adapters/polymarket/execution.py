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

import asyncio
from collections.abc import Coroutine
from typing import Any

import msgspec
import pandas as pd
from py_clob_client.client import BalanceAllowanceParams
from py_clob_client.client import ClobClient
from py_clob_client.client import OpenOrderParams
from py_clob_client.client import OrderArgs
from py_clob_client.client import PartialCreateOrderOptions
from py_clob_client.client import TradeParams
from py_clob_client.clob_types import AssetType
from py_clob_client.exceptions import PolyApiException

from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.polymarket.common.cache import get_polymarket_trades_key
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MAX_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MIN_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.constants import USDC_POS
from nautilus_trader.adapters.polymarket.common.constants import VALID_POLYMARKET_TIME_IN_FORCE
from nautilus_trader.adapters.polymarket.common.credentials import PolymarketWebSocketAuth
from nautilus_trader.adapters.polymarket.common.enums import PolymarketEventType
from nautilus_trader.adapters.polymarket.common.parsing import parse_order_side
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_condition_id
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_instrument_id
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_token_id
from nautilus_trader.adapters.polymarket.common.types import JSON
from nautilus_trader.adapters.polymarket.config import PolymarketExecClientConfig
from nautilus_trader.adapters.polymarket.http.conversion import convert_tif_to_polymarket_order_type
from nautilus_trader.adapters.polymarket.http.errors import should_retry
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider
from nautilus_trader.adapters.polymarket.schemas.trade import PolymarketTradeReport
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketOpenOrder
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserOrder
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserTrade
from nautilus_trader.adapters.polymarket.websocket.client import PolymarketWebSocketChannel
from nautilus_trader.adapters.polymarket.websocket.client import PolymarketWebSocketClient
from nautilus_trader.adapters.polymarket.websocket.types import USER_WS_MESSAGE
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import nanos_to_secs
from nautilus_trader.core.stats import basis_points_as_percentage
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.live.retry import RetryManagerPool
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orders import Order


class PolymarketExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Polymarket, a decentralized predication market.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : py_clob_client.client.ClobClient
        The Polymarket HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : PolymarketInstrumentProvider
        The instrument provider.
    config : PolymarketExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: ClobClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: PolymarketInstrumentProvider,
        ws_auth: PolymarketWebSocketAuth,
        config: PolymarketExecClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or POLYMARKET_VENUE.value),
            venue=POLYMARKET_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.CASH,
            base_currency=USDC_POS,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Configuration
        self._config = config
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay=}", LogColor.BLUE)

        self._enum_parser = BybitEnumParser()

        account_id = AccountId(f"{name or POLYMARKET_VENUE.value}-001")
        self._set_account_id(account_id)

        wallet_address = http_client.get_address()
        if wallet_address is None:
            raise RuntimeError("Auth error: could not determine `wallet_address`")
        self._wallet_address = wallet_address
        self._log.info(f"wallet_address={wallet_address}", LogColor.BLUE)

        # HTTP API
        self._signature_type = 0
        self._http_client = http_client
        self._retry_manager_pool = RetryManagerPool(
            pool_size=100,
            max_retries=config.max_retries or 0,
            retry_delay_secs=config.retry_delay or 0.0,
            logger=self._log,
            exc_types=(PolyApiException,),
            retry_check=should_retry,
        )
        self._decoder_order_report = msgspec.json.Decoder(PolymarketOpenOrder)
        self._decoder_trade_report = msgspec.json.Decoder(PolymarketTradeReport)

        # WebSocket API
        self._ws_auth = ws_auth
        self._ws_client: PolymarketWebSocketClient = self._create_websocket_client()
        self._ws_clients: dict[InstrumentId, PolymarketWebSocketClient] = {}
        self._decoder_user_msg = msgspec.json.Decoder(USER_WS_MESSAGE)

        # Hot caches
        self._active_markets: set[str] = set()
        self._allowances: dict[InstrumentId, str] = {}

    async def _connect(self) -> None:
        await asyncio.sleep(2.0)  # Initial delay to allow instruments to populate

        # Set up initial active markets
        instruments = self._cache.instruments(venue=POLYMARKET_VENUE)
        for instrument in instruments:
            await self._maintain_active_market(instrument.id)

        if self._ws_client.is_disconnected():
            await self._ws_client.connect()

        instrument_ids = [i.id for i in instruments]

        await self._update_allowances(instrument_ids)
        await self._update_account_state()

        # Wait for account to initialize
        while self.get_account() is None:
            await asyncio.sleep(0.01)

    async def _disconnect(self) -> None:
        # Shutdown websockets
        tasks: set[Coroutine[Any, Any, None]] = set()

        if self._ws_client.is_connected():
            tasks.add(self._ws_client.disconnect())

        for ws_client in self._ws_clients.values():
            if ws_client.is_connected():
                tasks.add(ws_client.disconnect())

        if tasks:
            await asyncio.gather(*tasks)

    def _stop(self) -> None:
        self._retry_manager_pool.shutdown()

    def _create_websocket_client(self) -> PolymarketWebSocketClient:
        self._log.info("Creating new PolymarketWebSocketClient", LogColor.MAGENTA)
        return PolymarketWebSocketClient(
            self._clock,
            base_url=self._config.base_url_ws,
            channel=PolymarketWebSocketChannel.USER,
            handler=self._handle_ws_message,
            handler_reconnect=None,
            loop=self._loop,
            auth=self._ws_auth,
        )

    async def _maintain_active_market(self, instrument_id: InstrumentId) -> None:
        condition_id = get_polymarket_condition_id(instrument_id)
        if condition_id in self._active_markets:
            return  # Already active

        token_id = get_polymarket_token_id(instrument_id)
        params = BalanceAllowanceParams(
            asset_type=AssetType.CONDITIONAL,
            token_id=token_id,
            signature_type=self._signature_type,
        )
        self._log.info(f"Updating {params}")
        await asyncio.to_thread(self._http_client.update_balance_allowance, params)

        if not self._ws_client.is_connected():
            ws_client = self._ws_client
            if condition_id in ws_client.market_subscriptions():
                return  # Already subscribed
            ws_client.subscribe_market(condition_id=condition_id)
        else:
            ws_client = self._create_websocket_client()
            if condition_id in ws_client.asset_subscriptions():
                return  # Already subscribed
            self._ws_clients[instrument_id] = ws_client
            ws_client.subscribe_market(condition_id=condition_id)
            await ws_client.connect()

        self._active_markets.add(condition_id)

    async def _update_allowances(self, instrument_ids: list[InstrumentId]) -> None:
        params = BalanceAllowanceParams(
            asset_type=AssetType.COLLATERAL,
            signature_type=self._signature_type,
        )
        self._log.info(f"Updating {params}")
        await asyncio.to_thread(self._http_client.update_balance_allowance, params)

        for instrument_id in instrument_ids:
            token_id = get_polymarket_token_id(instrument_id)
            params = BalanceAllowanceParams(
                asset_type=AssetType.CONDITIONAL,
                token_id=token_id,
                signature_type=self._signature_type,
            )
            self._log.info(f"Updating {params}")
            await asyncio.to_thread(self._http_client.update_balance_allowance, params)

            params = BalanceAllowanceParams(
                asset_type=AssetType.CONDITIONAL,
                token_id=token_id,
                signature_type=self._signature_type,
            )
            response: dict[str, Any] = await asyncio.to_thread(
                self._http_client.get_balance_allowance,
                params,
            )
            self._log.info(str(response))

    async def _update_account_state(self) -> None:
        params = BalanceAllowanceParams(
            asset_type=AssetType.COLLATERAL,
            signature_type=self._signature_type,
        )
        response = await asyncio.to_thread(
            self._http_client.get_balance_allowance,
            params,
        )
        usdc_raw = int(response["balance"]) * 1_000
        total = Money.from_raw(usdc_raw, currency=USDC_POS)
        account_balance = AccountBalance(
            total=total,
            locked=Money.from_raw(0, USDC_POS),
            free=total,
        )
        self.generate_account_state(
            balances=[account_balance],
            margins=[],  # N/A
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        self._log.info("Requesting OrderStatusReports...")
        reports: list[OrderStatusReport] = []

        if instrument_id is not None:
            condition_id = get_polymarket_condition_id(instrument_id)
            asset_id = get_polymarket_token_id(instrument_id)
            params = OpenOrderParams(market=condition_id, asset_id=asset_id)
        else:
            params = None

        # Check active orders with venue
        async with self._retry_manager_pool as retry_manager:
            response: list[JSON] | None = await retry_manager.run(
                "generate_order_status_reports",
                [instrument_id],
                asyncio.to_thread,
                self._http_client.get_orders,
                params=params,
                # TODO: Handle cursor for lookback
            )
            if response:
                # Uncomment for development
                # self._log.info(str(response), LogColor.MAGENTA)
                for json_obj in response:
                    raw = msgspec.json.encode(json_obj)
                    polymarket_order = self._decoder_order_report.decode(raw)

                    instrument_id = get_polymarket_instrument_id(
                        polymarket_order.market,
                        polymarket_order.asset_id,
                    )
                    instrument = self._cache.instrument(instrument_id)
                    if instrument is None:
                        self._log.error(
                            f"Cannot handle order report: instrument {instrument_id} not found",
                        )

                    venue_order_id = polymarket_order.get_venue_order_id()
                    client_order_id = self._cache.client_order_id(venue_order_id)
                    if client_order_id is None:
                        client_order_id = ClientOrderId(str(UUID4()))

                    report = polymarket_order.parse_to_order_status_report(
                        account_id=self.account_id,
                        instrument=instrument,
                        client_order_id=client_order_id,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    reports.append(report)

        # Check residual open orders
        reported_client_order_ids: set[ClientOrderId] = {r.client_order_id for r in reports}
        for order in self._cache.orders_open(venue=POLYMARKET_VENUE):
            if order.client_order_id in reported_client_order_ids:
                continue  # Already reported
            maybe_report = await self.generate_order_status_report(
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
            )
            if maybe_report:
                reports.append(maybe_report)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} OrderStatusReport{plural}")

        return reports

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
    ) -> OrderStatusReport | None:
        await self._maintain_active_market(instrument_id)

        if venue_order_id is None:
            venue_order_id = self._cache.venue_order_id(client_order_id)
            if venue_order_id is None:
                self._log.error(
                    "Cannot generate an order status report for Polymarket without the venue order ID",
                )
                return None  # Failed

        self._log.info(
            f"Generating OrderStatusReport for "
            f"{repr(client_order_id) if client_order_id else ''} "
            f"{repr(venue_order_id) if venue_order_id else ''}",
        )

        async with self._retry_manager_pool as retry_manager:
            response: JSON | None = await retry_manager.run(
                "generate_order_status_report",
                [client_order_id, venue_order_id],
                asyncio.to_thread,
                self._http_client.get_order,
                order_id=venue_order_id.value,
            )
            if not response:
                return None
            # Uncomment for development
            # self._log.info(str(response), LogColor.MAGENTA)
            raw_response = msgspec.json.encode(response)
            polymarket_order = self._decoder_order_report.decode(raw_response)
            instrument_id = get_polymarket_instrument_id(
                polymarket_order.market,
                polymarket_order.asset_id,
            )
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot handle order report: instrument {instrument_id} not found",
                )
            return polymarket_order.parse_to_order_status_report(
                account_id=self.account_id,
                instrument=instrument,
                client_order_id=client_order_id,
                ts_init=self._clock.timestamp_ns(),
            )

    async def generate_fill_reports(
        self,
        instrument_id: InstrumentId | None = None,
        venue_order_id: VenueOrderId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[FillReport]:
        self._log.info("Requesting FillReports...")
        reports: list[FillReport] = []

        params = TradeParams(maker_address=self._wallet_address)
        if instrument_id:
            condition_id = get_polymarket_condition_id(instrument_id)
            asset_id = get_polymarket_token_id(instrument_id)
            params.market = condition_id
            params.asset_id = asset_id

        details = []
        if instrument_id:
            details.append(instrument_id)

        async with self._retry_manager_pool as retry_manager:
            response: JSON | None = await retry_manager.run(
                "generate_fill_reports",
                details,
                asyncio.to_thread,
                self._http_client.get_trades,
                params=params,
            )
            if response:
                # Uncomment for development
                # self._log.info(str(response), LogColor.MAGENTA)
                for json_obj in response:
                    raw = msgspec.json.encode(json_obj)
                    polymarket_trade = self._decoder_trade_report.decode(raw)

                    instrument_id = get_polymarket_instrument_id(
                        polymarket_trade.market,
                        polymarket_trade.asset_id,
                    )
                    instrument = self._cache.instrument(instrument_id)
                    if instrument is None:
                        self._log.error(
                            f"Cannot handle trade report: instrument {instrument_id} not found",
                        )

                    venue_order_id = polymarket_trade.venue_order_id()
                    client_order_id = self._cache.client_order_id(venue_order_id)
                    if client_order_id is None:
                        client_order_id = ClientOrderId(str(UUID4()))

                    report = polymarket_trade.parse_to_fill_report(
                        account_id=self.account_id,
                        instrument=instrument,
                        client_order_id=client_order_id,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    reports.append(report)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} FillReport{plural}")

        return reports

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[PositionStatusReport]:
        reports: list[PositionStatusReport] = []

        # Not applicable on Polymarket

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} PositionReport{plural}")

        return reports

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _cancel_order(self, command: CancelOrder) -> None:
        # https://docs.polymarket.com/#cancel-an-order
        await self._maintain_active_market(command.instrument_id)

        order: Order | None = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id!r} not found in cache")
            return

        if order.is_closed:
            self._log.warning(
                f"`CancelOrder` command for {command.client_order_id!r} when order already {order.status_string()} "
                "(will not send to exchange)",
            )
            return

        async with self._retry_manager_pool as retry_manager:
            response: JSON | None = await retry_manager.run(
                "cancel_order",
                [order.client_order_id, order.venue_order_id],
                asyncio.to_thread,
                self._http_client.cancel,
                order_id=order.venue_order_id.value,
            )
            if not response or not retry_manager.result:
                reason = retry_manager.message
            else:
                reason = response.get("not_canceled")

            if reason:
                self.generate_order_cancel_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=reason,
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        # https://docs.polymarket.com/#cancel-orders
        await self._maintain_active_market(command.instrument_id)

        # Check open orders for instrument
        open_order_ids = self._cache.client_order_ids_open(instrument_id=command.instrument_id)

        # Filter orders that are actually open
        valid_cancels: list[CancelOrder] = []
        for cancel in command.cancels:
            if cancel.client_order_id in open_order_ids:
                valid_cancels.append(cancel)
                continue
            self._log.warning(f"{cancel.client_order_id!r} not open for cancel")

        if not valid_cancels:
            self._log.warning(f"No orders open for {command.instrument_id} batch cancel")
            return

        async with self._retry_manager_pool as retry_manager:
            order_ids = [o.venue_order_id.value for o in valid_cancels]
            response: JSON | None = await retry_manager.run(
                "batch_cancel_orders",
                [command.instrument_id],
                asyncio.to_thread,
                self._http_client.cancel_orders,
                order_ids=order_ids,
            )
            if not response or not retry_manager.result:
                reason_map = {order_id: retry_manager.message for order_id in order_ids}
            else:
                reason_map = response.get("not_canceled", {})

            for order_id, reason in reason_map.items():
                venue_order_id = VenueOrderId(order_id)
                client_order_id = self._cache.client_order_id(venue_order_id)
                if client_order_id:
                    self.generate_order_cancel_rejected(
                        strategy_id=command.strategy_id,
                        instrument_id=command.instrument_id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        reason=reason,
                        ts_event=self._clock.timestamp_ns(),
                    )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        # https://docs.polymarket.com/#cancel-orders
        await self._maintain_active_market(command.instrument_id)

        open_orders_strategy: list[Order] = self._cache.orders_open(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )
        if not open_orders_strategy:
            self._log.warning(f"No open orders to cancel for strategy {command.strategy_id}")
            return

        async with self._retry_manager_pool as retry_manager:
            order_ids = [o.venue_order_id.value for o in open_orders_strategy]
            response: JSON | None = await retry_manager.run(
                "cancel_all_orders",
                [command.instrument_id],
                asyncio.to_thread,
                self._http_client.cancel_orders,
                order_ids=order_ids,
            )
            if not response or not retry_manager.result:
                reason_map = {order_id: retry_manager.message for order_id in order_ids}
            else:
                reason_map = response.get("not_canceled", {})

            for order_id, reason in reason_map.items():
                venue_order_id = VenueOrderId(order_id)
                client_order_id = self._cache.client_order_id(venue_order_id)
                if client_order_id:
                    self.generate_order_cancel_rejected(
                        strategy_id=command.strategy_id,
                        instrument_id=command.instrument_id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        reason=reason,
                        ts_event=self._clock.timestamp_ns(),
                    )

    async def _submit_order(self, command: SubmitOrder) -> None:
        await self._maintain_active_market(command.instrument_id)

        order = command.order
        if order.is_closed:
            self._log.warning(f"Order {order} is already closed")
            return

        if order.is_reduce_only:
            self._log.warning("Reduce-only orders not supported on Polymarket")

        if order.is_post_only:
            self._log.warning("Post-only orders not supported on Polymarket")

        if order.time_in_force not in VALID_POLYMARKET_TIME_IN_FORCE:
            self._log.error(
                f"Order time in force {order.tif_string()} not supported on Polymarket, "
                "use any of FOK, GTC, GTD",
            )
            return

        if order.order_type == OrderType.MARKET:
            price = POLYMARKET_MAX_PRICE if order.side == OrderSide.BUY else POLYMARKET_MIN_PRICE
        elif order.order_type == OrderType.LIMIT:
            price = float(order.price)
        else:
            self._log.error(
                f"Order type {order.type_string()} not supported on Polymarket, "
                "use either MARKET, LIMIT",
            )
            return

        # Create signed Polymarket order
        order_args = OrderArgs(
            price=price,
            token_id=get_polymarket_token_id(order.instrument_id),
            size=float(order.quantity),
            side=order_side_to_str(order.side),
            expiration=int(nanos_to_secs(order.expire_time_ns)),
        )
        options = PartialCreateOrderOptions(neg_risk=False)
        signing_start = self._clock.timestamp()
        signed_order = await asyncio.to_thread(  # Send to thread to avoid blocking event loop
            self._http_client.create_order,
            order_args,
            options=options,
        )
        interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed Polymarket order in {interval:.3f}s", LogColor.BLUE)

        # Generate order submitted event, to ensure correct ordering of events
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        async with self._retry_manager_pool as retry_manager:
            response: JSON | None = await retry_manager.run(
                "submit_order",
                [order.client_order_id],
                asyncio.to_thread,
                self._http_client.post_order,
                signed_order,
                convert_tif_to_polymarket_order_type(order.time_in_force),
            )
            if not response or not response.get("success"):
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=retry_manager.message,
                    ts_event=self._clock.timestamp_ns(),
                )
            else:
                venue_order_id = VenueOrderId(response["orderID"])
                self._log.info(
                    f"Submitted {order.client_order_id!r} {venue_order_id!r}",
                    LogColor.MAGENTA,
                )
                self._cache.add_venue_order_id(order.client_order_id, venue_order_id)

    def _check_order_validity(self, order: Order, product_type: BybitProductType) -> bool:
        # Check post only
        if order.is_post_only and order.order_type != OrderType.LIMIT:
            self._log.error(
                f"Cannot submit {order} has invalid post only {order.is_post_only}, unsupported on Bybit",
            )
            return False

        # Check reduce only
        if order.is_reduce_only and product_type == BybitProductType.SPOT:
            self._log.error(
                f"Cannot submit {order} is reduce_only, unsupported on Bybit SPOT",
            )
            return False

        return True

    def _handle_ws_message(self, raw: bytes) -> None:
        # Uncomment for development
        # self._log.info(str(json.dumps(msgspec.json.decode(raw), indent=4)), color=LogColor.MAGENTA)
        try:
            ws_message = self._decoder_user_msg.decode(raw)

            if isinstance(ws_message, PolymarketUserOrder):
                self._handle_ws_order_msg(ws_message, wait_for_ack=True)
            elif isinstance(ws_message, PolymarketUserTrade):
                self._add_trade_to_cache(ws_message, raw)
                self._handle_ws_trade_msg(ws_message, wait_for_ack=True)
            else:
                self._log.error(f"Unrecognized websocket message {ws_message}")
        except Exception as e:
            self._log.error(f"Error handling websocket message: {e} {raw.decode()}")

    def _add_trade_to_cache(self, msg: PolymarketUserTrade, raw: bytes) -> None:
        start_ns = self._clock.timestamp_ns()
        cache_key = get_polymarket_trades_key(msg.taker_order_id, msg.id)
        self._cache.add(cache_key, raw)
        interval_us = int((self._clock.timestamp_ns() - start_ns) / 1_000)
        self._log.info(
            f"Added trade {msg.id} {msg.status.value} to {cache_key} in {interval_us}μs",
            LogColor.BLUE,
        )

    async def _wait_for_ack_order(
        self,
        msg: PolymarketUserOrder,
        venue_order_id: VenueOrderId,
    ) -> None:
        start_time = self._clock.timestamp()
        client_order_id = self._cache.client_order_id(venue_order_id)
        while client_order_id is None:
            if self._clock.timestamp() - start_time > 5.0:
                break
            await asyncio.sleep(0.001)
            client_order_id = self._cache.client_order_id(venue_order_id)

        if client_order_id is None:
            self._log.warning(f"Timed out awaiting placement ack for {venue_order_id!r}")

        self._handle_ws_order_msg(msg, wait_for_ack=False)

    async def _wait_for_ack_trade(
        self,
        msg: PolymarketUserTrade,
        venue_order_id: VenueOrderId,
    ) -> None:
        start_time = self._clock.timestamp()
        client_order_id = self._cache.client_order_id(venue_order_id)
        while client_order_id is None:
            if self._clock.timestamp() - start_time > 5.0:
                break
            await asyncio.sleep(0.001)
            client_order_id = self._cache.client_order_id(venue_order_id)

        if client_order_id is None:
            self._log.warning(f"Timed out awaiting placement ack for {venue_order_id!r}")

        self._handle_ws_trade_msg(msg, wait_for_ack=False)

    def _handle_ws_order_msg(self, msg: PolymarketUserOrder, wait_for_ack: bool):
        venue_order_id = msg.get_venue_order_id()

        instrument_id = get_polymarket_instrument_id(msg.market, msg.asset_id)
        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            raise ValueError(f"Cannot handle ws message: instrument {instrument_id} not found")

        if wait_for_ack:
            self._loop.create_task(self._wait_for_ack_order(msg, venue_order_id))
            return

        client_order_id = self._cache.client_order_id(venue_order_id)

        strategy_id = None
        if client_order_id:
            strategy_id = self._cache.strategy_id_for_order(client_order_id)

        if strategy_id is None:
            report = msg.parse_to_order_status_report(
                account_id=self.account_id,
                instrument=instrument,
                client_order_id=client_order_id,
                ts_init=self._clock.timestamp_ns(),
            )
            self._send_order_status_report(report)
            return

        match msg.type:
            case PolymarketEventType.PLACEMENT:
                self.generate_order_accepted(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=self._clock.timestamp_ns(),
                )
            case PolymarketEventType.CANCELLATION:
                self.generate_order_canceled(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=millis_to_nanos(int(msg.timestamp)),
                )
            case PolymarketEventType.UPDATE:  # Matched
                pass  # Use trade messages only for matches

    def _handle_ws_trade_msg(self, msg: PolymarketUserTrade, wait_for_ack: bool):
        venue_order_id = msg.venue_order_id()

        instrument_id = get_polymarket_instrument_id(msg.market, msg.asset_id)
        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            raise ValueError(f"Cannot handle ws message: instrument {instrument_id} not found")

        if wait_for_ack:
            self._loop.create_task(self._wait_for_ack_trade(msg, venue_order_id))
            return

        client_order_id = self._cache.client_order_id(venue_order_id)

        strategy_id = None
        if client_order_id:
            strategy_id = self._cache.strategy_id_for_order(client_order_id)

        if strategy_id is None:
            report = msg.parse_to_fill_report(
                account_id=self.account_id,
                instrument=instrument,
                client_order_id=client_order_id,
                ts_init=self._clock.timestamp_ns(),
            )
            self._send_fill_report(report)
            return

        order = self._cache.order(client_order_id)
        if order.is_closed:
            return  # Already closed (only status update)

        last_qty = instrument.make_qty(msg.size)
        last_px = instrument.make_price(msg.price)
        commission = float(last_qty * last_px) * basis_points_as_percentage(float(msg.fee_rate_bps))

        self.generate_order_filled(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            venue_position_id=None,  # Not applicable on Polymarket
            trade_id=TradeId(msg.id),
            order_side=parse_order_side(msg.side),
            order_type=order.order_type,
            last_qty=instrument.make_qty(msg.size),
            last_px=instrument.make_price(msg.price),
            quote_currency=USDC_POS,
            commission=Money(commission, USDC_POS),
            liquidity_side=msg.liqudity_side(),
            ts_event=millis_to_nanos(int(msg.match_time)),
            info=msgspec.structs.asdict(msg),
        )

        self._loop.create_task(self._update_account_state())
