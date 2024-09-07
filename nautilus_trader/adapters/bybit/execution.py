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
from decimal import Decimal

import msgspec
import pandas as pd

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.credentials import get_api_key
from nautilus_trader.adapters.bybit.common.credentials import get_api_secret
from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitOrderStatus
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.enums import BybitStopOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.common.enums import BybitTpSlMode
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerDirection
from nautilus_trader.adapters.bybit.common.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrder
from nautilus_trader.adapters.bybit.endpoints.trade.batch_place_order import BybitBatchPlaceOrder
from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.errors import BybitError
from nautilus_trader.adapters.bybit.http.errors import should_retry
from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
from nautilus_trader.adapters.bybit.schemas.common import BybitWsSubscriptionMsg
from nautilus_trader.adapters.bybit.schemas.ws import BYBIT_PONG
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountExecution
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountExecutionMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountOrderMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountPositionMsg
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsMessageGeneral
from nautilus_trader.adapters.bybit.websocket.client import BybitWebsocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.live.retry import RetryManagerPool
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import account_type_to_str
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitIfTouchedOrder
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketIfTouchedOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.model.orders import TrailingStopLimitOrder
from nautilus_trader.model.orders import TrailingStopMarketOrder
from nautilus_trader.model.position import Position


class BybitExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Bybit centralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BybitHttpClient
        The Bybit HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BybitInstrumentProvider
        The instrument provider.
    product_types : list[BybitProductType]
        The product types for the client.
    base_url_ws : str
        The base URL for the WebSocket client.
    config : BybitExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BybitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BybitInstrumentProvider,
        product_types: list[BybitProductType],
        base_url_ws: str,
        config: BybitExecClientConfig,
        name: str | None,
    ) -> None:
        if BybitProductType.SPOT in product_types:
            if len(set(product_types)) > 1:
                raise ValueError("Cannot configure SPOT with other product types")
            account_type = AccountType.CASH
        else:
            account_type = AccountType.MARGIN

        super().__init__(
            loop=loop,
            client_id=ClientId(name or BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=account_type,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Configuration
        self._product_types = product_types
        self._use_gtd = config.use_gtd
        self._use_reduce_only = config.use_reduce_only
        self._use_position_ids = config.use_position_ids
        self._max_retries = config.max_retries or 0
        self._retry_delay = config.retry_delay or 1.0
        self._log.info(f"Account type: {account_type_to_str(account_type)}", LogColor.BLUE)
        self._log.info(f"Product types: {[p.value for p in product_types]}", LogColor.BLUE)
        self._log.info(f"{config.use_gtd=}", LogColor.BLUE)
        self._log.info(f"{config.use_reduce_only=}", LogColor.BLUE)
        self._log.info(f"{config.use_position_ids=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay=}", LogColor.BLUE)

        self._enum_parser = BybitEnumParser()

        account_id = AccountId(f"{name or BYBIT_VENUE.value}-UNIFIED")
        self._set_account_id(account_id)

        # WebSocket API
        self._ws_client = BybitWebsocketClient(
            clock=clock,
            handler=self._handle_ws_message,
            handler_reconnect=None,
            base_url=base_url_ws,
            is_private=True,
            api_key=config.api_key or get_api_key(config.demo, config.testnet),
            api_secret=config.api_secret or get_api_secret(config.demo, config.testnet),
            loop=loop,
        )

        # HTTP API
        self._http_account = BybitAccountHttpAPI(
            client=client,
            clock=clock,
        )

        # Order submission
        self._submit_order_methods = {
            OrderType.MARKET: self._submit_market_order,
            OrderType.LIMIT: self._submit_limit_order,
            OrderType.STOP_MARKET: self._submit_stop_market_order,
            OrderType.STOP_LIMIT: self._submit_stop_limit_order,
            OrderType.MARKET_IF_TOUCHED: self._submit_market_if_touched_order,
            OrderType.LIMIT_IF_TOUCHED: self._submit_limit_if_touched_order,
            OrderType.TRAILING_STOP_MARKET: self._submit_trailing_stop_market,
        }

        # Decoders
        self._decoder_ws_msg_general = msgspec.json.Decoder(BybitWsMessageGeneral)
        self._decoder_ws_subscription = msgspec.json.Decoder(BybitWsSubscriptionMsg)
        self._decoder_ws_account_order_update = msgspec.json.Decoder(BybitWsAccountOrderMsg)
        self._decoder_ws_account_execution_update = msgspec.json.Decoder(
            BybitWsAccountExecutionMsg,
        )
        self._decoder_ws_account_position_update = msgspec.json.Decoder(
            BybitWsAccountPositionMsg,
        )

        # Hot caches
        self._instrument_ids: dict[str, InstrumentId] = {}
        self._pending_trailing_stops: dict[ClientOrderId, Order] = {}

        self._retry_manager_pool = RetryManagerPool(
            pool_size=100,
            max_retries=config.max_retries or 0,
            retry_delay_secs=config.retry_delay or 0.0,
            logger=self._log,
            exc_types=(BybitError,),
            retry_check=should_retry,
        )

    async def _connect(self) -> None:
        await self._update_account_state()

        await self._ws_client.connect()
        await self._ws_client.subscribe_executions_update()
        await self._ws_client.subscribe_orders_update()

    async def _disconnect(self) -> None:
        await self._ws_client.disconnect()

    def _stop(self) -> None:
        self._retry_manager_pool.shutdown()

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

        try:
            _symbol = instrument_id.symbol.value if instrument_id is not None else None
            symbol = BybitSymbol(_symbol) if _symbol is not None else None
            # active_symbols = self._get_cache_active_symbols()
            # active_symbols.update(await self._get_active_position_symbols(symbol))
            # open_orders: dict[BybitProductType, list[BybitOrder]] = dict()
            for product_type in self._product_types:
                bybit_orders = await self._http_account.query_order_history(product_type, symbol)
                for bybit_order in bybit_orders:
                    # Uncomment for development
                    # self._log.info(f"Generating report {bybit_order}", LogColor.MAGENTA)
                    bybit_symbol = BybitSymbol(
                        bybit_order.symbol + f"-{product_type.value.upper()}",
                    )

                    client_order_id = (
                        ClientOrderId(bybit_order.orderLinkId) if bybit_order.orderLinkId else None
                    )
                    if client_order_id is None:
                        client_order_id = self._cache.client_order_id(
                            VenueOrderId(bybit_order.orderId),
                        )
                    report = bybit_order.parse_to_order_status_report(
                        client_order_id=client_order_id,
                        account_id=self.account_id,
                        instrument_id=bybit_symbol.to_instrument_id(),
                        report_id=UUID4(),
                        enum_parser=self._enum_parser,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    reports.append(report)
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
        except BybitError as e:
            self._log.error(f"Failed to generate OrderStatusReports: {e}")

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
        PyCondition.false(
            client_order_id is None and venue_order_id is None,
            "both `client_order_id` and `venue_order_id` were `None`",
        )

        if client_order_id:
            order = self._cache.order(client_order_id)
            if order and order.order_type in (
                OrderType.TRAILING_STOP_MARKET,
                OrderType.TRAILING_STOP_LIMIT,
            ):
                self._log.warning("Cannot query with client order ID for trailing stops")
                client_order_id = None

        self._log.info(
            f"Generating OrderStatusReport for "
            f"{repr(client_order_id) if client_order_id else ''} "
            f"{repr(venue_order_id) if venue_order_id else ''}",
        )
        try:
            bybit_symbol = BybitSymbol(instrument_id.symbol.value)
            product_type = bybit_symbol.product_type
            bybit_orders = await self._http_account.query_order(
                product_type=product_type,
                symbol=instrument_id.symbol.value,
                client_order_id=client_order_id.value if client_order_id else None,
                order_id=venue_order_id.value if venue_order_id else None,
            )
            if len(bybit_orders) == 0:
                self._log.error(f"Received no order for {venue_order_id}")
                return None
            targetOrder = bybit_orders[0]
            if len(bybit_orders) > 1:
                self._log.warning(f"Received more than one order for {venue_order_id}")
                targetOrder = bybit_orders[0]

            order_link_id = bybit_orders[0].orderLinkId
            client_order_id = ClientOrderId(order_link_id) if order_link_id else None
            venue_order_id = VenueOrderId(bybit_orders[0].orderId)
            if client_order_id is None:
                client_order_id = self._cache.client_order_id(venue_order_id)

            order_report = targetOrder.parse_to_order_status_report(
                client_order_id=client_order_id,
                account_id=self.account_id,
                instrument_id=instrument_id,
                report_id=UUID4(),
                enum_parser=self._enum_parser,
                ts_init=self._clock.timestamp_ns(),
            )
            self._log.debug(f"Received {order_report}", LogColor.MAGENTA)
            return order_report
        except BybitError as e:
            self._log.error(f"Failed to generate OrderStatusReport: {e}")
        return None

    async def generate_fill_reports(
        self,
        instrument_id: InstrumentId | None = None,
        venue_order_id: VenueOrderId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[FillReport]:
        self._log.info("Requesting FillReports...")
        reports: list[FillReport] = []

        try:
            _symbol = instrument_id.symbol.value if instrument_id is not None else None
            symbol = BybitSymbol(_symbol) if _symbol is not None else None
            # active_symbols = self._get_cache_active_symbols()
            # active_symbols.update(await self._get_active_position_symbols(symbol))
            # open_orders: dict[BybitProductType, list[BybitOrder]] = dict()
            for product_type in self._product_types:
                bybit_fills = await self._http_account.query_trade_history(product_type, symbol)
                for bybit_fill in bybit_fills:
                    # Uncomment for development
                    # self._log.info(f"Generating fill {bybit_fill}", LogColor.MAGENTA)
                    bybit_symbol = BybitSymbol(
                        bybit_fill.symbol + f"-{product_type.value.upper()}",
                    )
                    report = bybit_fill.parse_to_fill_report(
                        account_id=self.account_id,
                        instrument_id=bybit_symbol.to_instrument_id(),
                        report_id=UUID4(),
                        enum_parser=self._enum_parser,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    reports.append(report)
                    self._log.debug(f"Received {report}")
        except BybitError as e:
            self._log.error(f"Failed to generate FillReports: {e}")

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

        try:
            if instrument_id:
                self._log.info(f"Requesting PositionStatusReport for {instrument_id}")
                bybit_symbol = BybitSymbol(instrument_id.symbol.value)
                positions = await self._http_account.query_position_info(
                    bybit_symbol.product_type,
                    bybit_symbol.raw_symbol,
                )
                for position in positions:
                    position_report = position.parse_to_position_status_report(
                        account_id=self.account_id,
                        instrument_id=instrument_id,
                        report_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                    )
                    self._log.debug(f"Received {position_report}")
                    reports.append(position_report)
            else:
                self._log.info("Requesting PositionStatusReports...")
                for product_type in self._product_types:
                    if product_type == BybitProductType.SPOT:
                        continue  # No positions on spot
                    positions = await self._http_account.query_position_info(product_type)
                    for position in positions:
                        symbol = position.symbol
                        bybit_symbol = BybitSymbol(f"{symbol}-{product_type.value.upper()}")
                        position_report = position.parse_to_position_status_report(
                            account_id=self.account_id,
                            instrument_id=bybit_symbol.to_instrument_id(),
                            report_id=UUID4(),
                            ts_init=self._clock.timestamp_ns(),
                        )
                        self._log.debug(f"Received {position_report}")
                        reports.append(position_report)
        except BybitError as e:
            self._log.error(f"Failed to generate PositionReports: {e}")

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} PositionReport{plural}")

        return reports

    def _get_cached_instrument_id(
        self,
        symbol: str,
        product_type: BybitProductType,
    ) -> InstrumentId:
        bybit_symbol = BybitSymbol(f"{symbol}-{product_type.value.upper()}")
        return bybit_symbol.to_instrument_id()

    def _get_cache_active_symbols(self) -> set[str]:
        # Check cache for all active orders
        open_orders: list[Order] = self._cache.orders_open(venue=self.venue)
        open_positions: list[Position] = self._cache.positions_open(venue=self.venue)
        active_symbols: set[str] = set()
        for order in open_orders:
            active_symbols.add(BybitSymbol(order.instrument_id.symbol.value))
        for position in open_positions:
            active_symbols.add(BybitSymbol(position.instrument_id.symbol.value))
        return active_symbols

    def _determine_time_in_force(self, order: Order) -> BybitTimeInForce:
        time_in_force: TimeInForce = order.time_in_force
        if order.time_in_force == TimeInForce.GTD:
            if not self._use_gtd:
                time_in_force = TimeInForce.GTC
                self._log.info(
                    f"Converted GTD `time_in_force` to GTC for {order.client_order_id}",
                    LogColor.BLUE,
                )
            else:
                raise RuntimeError("invalid time in force GTD unsupported by Bybit")

        if order.is_post_only:
            return BybitTimeInForce.POST_ONLY
        return self._enum_parser.parse_nautilus_time_in_force(time_in_force)

    async def _get_active_position_symbols(self, symbol: str | None) -> set[str]:
        active_symbols: set[str] = set()
        for product_type in self._product_types:
            bybit_positions = await self._http_account.query_position_info(
                product_type,
                symbol,
            )
            for position in bybit_positions:
                active_symbols.add(position.symbol)

        return active_symbols

    async def _update_account_state(self) -> None:
        # positions = await self._http_account.query_position_info()
        (balances, ts_event) = await self._http_account.query_wallet_balance()
        if balances:
            self._log.info("Bybit API key authenticated", LogColor.GREEN)
            self._log.info(f"API key {self._http_account.client.api_key} has trading permissions")
        for balance in balances:
            balances = balance.parse_to_account_balance()
            margins = balance.parse_to_margin_balance()
            try:
                self.generate_account_state(
                    balances=balances,
                    margins=margins,
                    reported=True,
                    ts_event=millis_to_nanos(ts_event),
                )
            except Exception as e:
                self._log.error(f"Failed to generate AccountState: {e}")

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _cancel_order(self, command: CancelOrder) -> None:
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

        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        client_order_id = command.client_order_id.value
        venue_order_id = str(command.venue_order_id) if command.venue_order_id else None

        async with self._retry_manager_pool as retry_manager:
            await retry_manager.run(
                "cancel_order",
                [client_order_id, venue_order_id],
                self._http_account.cancel_order,
                bybit_symbol.product_type,
                bybit_symbol.raw_symbol,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
            )

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        # https://bybit-exchange.github.io/docs/v5/order/batch-cancel

        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type
        max_batch = 20 if product_type == BybitProductType.OPTION else 10

        # Check open orders for instrument
        open_order_ids = self._cache.client_order_ids_open(instrument_id=command.instrument_id)

        # Filter orders that are actually open
        valid_cancels: list[(CancelOrder)] = []
        for cancel in command.cancels:
            if cancel.client_order_id in open_order_ids:
                valid_cancels.append(cancel)
                continue
            self._log.warning(f"{cancel.client_order_id!r} not open for cancel")

        if not valid_cancels:
            self._log.warning(f"No orders open for {command.instrument_id} batch cancel")
            return

        for i in range(0, len(valid_cancels), max_batch):
            batch_cancels = valid_cancels[i : i + max_batch]
            cancel_orders: list[BybitBatchCancelOrder] = []

            for cancel in batch_cancels:
                cancel_orders.append(
                    BybitBatchCancelOrder(
                        symbol=bybit_symbol.raw_symbol,
                        orderId=cancel.venue_order_id.value if cancel.venue_order_id else None,
                        orderLinkId=cancel.client_order_id.value,
                    ),
                )

            async with self._retry_manager_pool as retry_manager:
                await retry_manager.run(
                    "batch_cancel_orders",
                    None,
                    self._http_account.batch_cancel_orders,
                    product_type=product_type,
                    cancel_orders=cancel_orders,
                )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)

        async with self._retry_manager_pool as retry_manager:
            await retry_manager.run(
                "cancel_all_orders",
                None,
                self._http_account.cancel_all_orders,
                product_type=bybit_symbol.product_type,
                symbol=bybit_symbol.raw_symbol,
            )

    async def _modify_order(self, command: ModifyOrder) -> None:
        order: Order | None = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id!r} not found in cache")
            return

        if order.is_closed:
            self._log.warning(
                f"`ModifyOrder` command for {command.client_order_id!r} when order already {order.status_string()} "
                "(will not send to exchange)",
            )
            return

        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        client_order_id = command.client_order_id.value
        venue_order_id = str(command.venue_order_id) if command.venue_order_id else None
        price = str(command.price) if command.price else None
        trigger_price = str(command.trigger_price) if command.trigger_price else None
        quantity = str(command.quantity) if command.quantity else None

        async with self._retry_manager_pool as retry_manager:
            await retry_manager.run(
                "modify_order",
                [client_order_id, venue_order_id],
                self._http_account.amend_order,
                bybit_symbol.product_type,
                bybit_symbol.raw_symbol,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                trigger_price=trigger_price,
                quantity=quantity,
                price=price,
            )

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order
        if order.is_closed:
            self._log.warning(f"Order {order} is already closed")
            return

        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        if not self._check_order_validity(order, bybit_symbol.product_type):
            return

        # Generate order submitted event, to ensure correct ordering of event
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        async with self._retry_manager_pool as retry_manager:
            await retry_manager.run(
                "submit_order",
                [command.order.client_order_id],
                self._submit_order_methods[order.order_type],
                order,
            )
            if not retry_manager.result:
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=retry_manager.message,
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type
        max_batch = 20 if product_type == BybitProductType.OPTION else 10

        for i in range(0, len(command.order_list.orders), max_batch):
            batch_submits = command.order_list.orders[i : i + max_batch]
            submit_orders: list[BybitBatchPlaceOrder] = []

            for order in batch_submits:
                if not self._check_order_validity(order, product_type):
                    self._log.error(f"Error on {command}")
                    return  # Do not submit batch

                match order.order_type:
                    case OrderType.MARKET:
                        batch_order = self._create_market_batch_order(order)
                    case OrderType.LIMIT:
                        batch_order = self._create_limit_batch_order(order)
                    case OrderType.LIMIT_IF_TOUCHED:
                        batch_order = self._create_limit_if_touched_batch_order(order)
                    case OrderType.STOP_MARKET:
                        batch_order = self._create_stop_market_batch_order(order)
                    case OrderType.MARKET_IF_TOUCHED:
                        batch_order = self._create_market_if_touched_batch_order(order)
                    case _:
                        self._log.error(f"Unsupported order type for 'submit_order_list': {order}")
                        self._log.error(f"Error on {command}")
                        return

                submit_orders.append(batch_order)

            now_ns = self._clock.timestamp_ns()
            for order in batch_submits:
                self.generate_order_submitted(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    ts_event=now_ns,
                )

            async with self._retry_manager_pool as retry_manager:
                await retry_manager.run(
                    "submit_order_list",
                    None,
                    self._http_account.batch_place_orders,
                    product_type=product_type,
                    submit_orders=submit_orders,
                )

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

    async def _submit_market_order(self, order: MarketOrder) -> None:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        await self._http_account.place_order(
            product_type=bybit_symbol.product_type,
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            order_type=BybitOrderType.MARKET,
            quantity=str(order.quantity),
            quote_quantity=order.is_quote_quantity,
            time_in_force=time_in_force,
            client_order_id=str(order.client_order_id),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
        )

    async def _submit_limit_order(self, order: LimitOrder) -> None:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        await self._http_account.place_order(
            product_type=bybit_symbol.product_type,
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            order_type=BybitOrderType.LIMIT,
            quantity=str(order.quantity),
            quote_quantity=order.is_quote_quantity,
            price=str(order.price),
            time_in_force=time_in_force,
            client_order_id=str(order.client_order_id),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
        )

    async def _submit_stop_market_order(self, order: StopMarketOrder) -> None:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        trigger_direction = self._enum_parser.parse_trigger_direction(order.order_type, order.side)
        trigger_type = self._enum_parser.parse_nautilus_trigger_type(order.trigger_type)
        await self._http_account.place_order(
            product_type=product_type,
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            order_type=BybitOrderType.MARKET,
            quantity=str(order.quantity),
            quote_quantity=order.is_quote_quantity,
            time_in_force=time_in_force,
            client_order_id=str(order.client_order_id),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
            close_on_trigger=True,  # Conservative for stop-loss orders
            tpsl_mode=BybitTpSlMode.FULL,
            trigger_direction=trigger_direction,
            trigger_type=trigger_type,
            sl_trigger_price=str(order.trigger_price),
            sl_order_type=BybitOrderType.MARKET,
        )

    async def _submit_stop_limit_order(self, order: StopLimitOrder) -> None:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        trigger_direction = self._enum_parser.parse_trigger_direction(order.order_type, order.side)
        trigger_type = self._enum_parser.parse_nautilus_trigger_type(order.trigger_type)
        await self._http_account.place_order(
            product_type=product_type,
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            order_type=BybitOrderType.LIMIT,
            quantity=str(order.quantity),
            quote_quantity=order.is_quote_quantity,
            time_in_force=time_in_force,
            client_order_id=str(order.client_order_id),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
            tpsl_mode=BybitTpSlMode.PARTIAL,
            trigger_direction=trigger_direction,
            trigger_type=trigger_type,
            sl_trigger_price=str(order.trigger_price),
            sl_order_type=BybitOrderType.LIMIT,
        )

    async def _submit_market_if_touched_order(self, order: MarketIfTouchedOrder) -> None:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        order_type = BybitOrderType.MARKET
        trigger_direction = self._enum_parser.parse_trigger_direction(order.order_type, order.side)
        trigger_type = self._enum_parser.parse_nautilus_trigger_type(order.trigger_type)
        await self._http_account.place_order(
            product_type=product_type,
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            order_type=order_type,
            quantity=str(order.quantity),
            quote_quantity=order.is_quote_quantity,
            time_in_force=time_in_force,
            client_order_id=str(order.client_order_id),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
            tpsl_mode=BybitTpSlMode.FULL,
            trigger_direction=trigger_direction,
            trigger_type=trigger_type,
            trigger_price=str(order.trigger_price),
        )

    async def _submit_limit_if_touched_order(self, order: LimitIfTouchedOrder) -> None:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        trigger_direction = self._enum_parser.parse_trigger_direction(order.order_type, order.side)
        trigger_type = self._enum_parser.parse_nautilus_trigger_type(order.trigger_type)
        await self._http_account.place_order(
            product_type=product_type,
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            order_type=BybitOrderType.MARKET,
            quantity=str(order.quantity),
            quote_quantity=order.is_quote_quantity,
            time_in_force=time_in_force,
            client_order_id=str(order.client_order_id),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
            tpsl_mode=BybitTpSlMode.PARTIAL,
            trigger_direction=trigger_direction,
            trigger_type=trigger_type,
            tp_trigger_price=str(order.trigger_price),
            tp_limit_price=str(order.price),
            tp_order_type=BybitOrderType.LIMIT,
        )

    async def _submit_trailing_stop_market(self, order: TrailingStopMarketOrder) -> None:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type
        trigger_type = self._enum_parser.parse_nautilus_trigger_type(order.trigger_type)
        self._pending_trailing_stops[order.client_order_id] = order
        await self._http_account.set_trading_stop(
            product_type=product_type,
            symbol=bybit_symbol.raw_symbol,
            sl_order_type=BybitOrderType.MARKET,
            sl_quantity=str(order.quantity),
            tpsl_mode=BybitTpSlMode.FULL,
            trigger_type=trigger_type,
            trailing_offset=str(order.trailing_offset),
        )

    def _handle_ws_message(self, raw: bytes) -> None:
        try:
            ws_message = self._decoder_ws_msg_general.decode(raw)
            if ws_message.op == BYBIT_PONG:
                return
            if ws_message.success is False:
                self._log.error(f"WebSocket error: {ws_message}")
                return
            if not ws_message.topic:
                return

            if "order" in ws_message.topic:
                self._handle_account_order_update(raw)
            elif "execution" in ws_message.topic:
                self._handle_account_execution_update(raw)
            else:
                self._log.error(f"Unknown websocket message topic: {ws_message.topic}")
        except Exception as e:
            self._log.error(f"Failed to parse websocket message: {raw.decode()} with error {e}")

    def _handle_account_execution_update(self, raw: bytes) -> None:
        try:
            msg = self._decoder_ws_account_execution_update.decode(raw)
            for trade in msg.data:
                self._process_execution(trade)
        except Exception as e:
            self._log.exception(f"Failed to handle account execution update: {e}", e)

    def _process_execution(self, execution: BybitWsAccountExecution) -> None:
        instrument_id = self._get_cached_instrument_id(execution.symbol, execution.category)
        client_order_id = ClientOrderId(execution.orderLinkId) if execution.orderLinkId else None
        venue_order_id = VenueOrderId(execution.orderId)
        order_side = self._enum_parser.parse_bybit_order_side(execution.side)

        if client_order_id is None:
            client_order_id = self._cache.client_order_id(venue_order_id)

        if client_order_id is None:
            self._log.debug(
                f"Cannot process order execution for {venue_order_id!r}: no `ClientOrderId` found "
                "(most likely due to being an external order)",
            )
            return

        order = self._cache.order(client_order_id)
        if order is None:
            strategy_id = self._cache.strategy_id_for_order(client_order_id)
            trigger_direction = BybitTriggerDirection.NONE
            if execution.stopOrderType != BybitStopOrderType.NONE:
                trigger_direction = (
                    BybitTriggerDirection.RISES_TO
                    if order_side == OrderSide.SELL
                    else BybitTriggerDirection.FALLS_TO
                )

            order_type = self._enum_parser.parse_bybit_order_type(
                execution.orderType,
                execution.stopOrderType,
                order_side,
                trigger_direction,
            )
        else:
            strategy_id = order.strategy_id
            order_type = order.order_type

        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            raise ValueError(f"Cannot handle trade event: instrument {instrument_id} not found")

        self.generate_order_filled(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            venue_position_id=None,
            trade_id=TradeId(execution.execId),
            order_side=order_side,
            order_type=order_type,
            last_qty=Quantity(float(execution.execQty), instrument.size_precision),
            last_px=Price(float(execution.execPrice), instrument.price_precision),
            quote_currency=instrument.quote_currency,
            commission=Money(Decimal(execution.execFee), instrument.quote_currency),
            liquidity_side=(
                LiquiditySide.MAKER if order_type == OrderType.LIMIT else LiquiditySide.TAKER
            ),
            ts_event=millis_to_nanos(float(execution.execTime)),
        )

    def _handle_account_order_update(self, raw: bytes) -> None:  # noqa: C901 (too complex)
        try:
            msg = self._decoder_ws_account_order_update.decode(raw)
            for bybit_order in msg.data:
                instrument_id = self._get_cached_instrument_id(
                    bybit_order.symbol,
                    bybit_order.category,
                )
                client_order_id = (
                    ClientOrderId(bybit_order.orderLinkId) if bybit_order.orderLinkId else None
                )
                venue_order_id = VenueOrderId(bybit_order.orderId)
                if client_order_id is None:
                    client_order_id = self._cache.client_order_id(venue_order_id)

                order_side = self._enum_parser.parse_bybit_order_side(bybit_order.side)

                if (
                    client_order_id is None
                    and bybit_order.stopOrderType == BybitStopOrderType.TRAILING_STOP
                ):
                    for order in self._pending_trailing_stops.values():
                        if order.instrument_id != instrument_id or order.side != order_side:
                            continue
                        if order.quantity != Quantity.from_str(bybit_order.qty):
                            continue
                        if (
                            bybit_order.orderType == BybitOrderType.MARKET
                            and not isinstance(
                                order,
                                TrailingStopMarketOrder,
                            )
                            or bybit_order.orderType == BybitOrderType.LIMIT
                            and not isinstance(
                                order,
                                TrailingStopLimitOrder,
                            )
                        ):
                            continue

                        if bybit_order.orderStatus == BybitOrderStatus.UNTRIGGERED:
                            self.generate_order_accepted(
                                strategy_id=order.strategy_id,
                                instrument_id=order.instrument_id,
                                client_order_id=order.client_order_id,
                                venue_order_id=venue_order_id,
                                ts_event=millis_to_nanos(int(bybit_order.updatedTime)),
                            )
                        self._pending_trailing_stops.pop(order.client_order_id)
                        return

                report = bybit_order.parse_to_order_status_report(
                    client_order_id=client_order_id,
                    account_id=self.account_id,
                    instrument_id=self._get_cached_instrument_id(
                        bybit_order.symbol,
                        bybit_order.category,
                    ),
                    enum_parser=self._enum_parser,
                    ts_init=self._clock.timestamp_ns(),
                )

                strategy_id = None
                if report.client_order_id:
                    strategy_id = self._cache.strategy_id_for_order(report.client_order_id)

                if strategy_id is None:
                    # External order
                    self._send_order_status_report(report)
                    return

                order = self._cache.order(report.client_order_id)
                if order is None:
                    self._log.error(f"Cannot find {report.client_order_id!r}")
                    return

                if bybit_order.orderStatus == BybitOrderStatus.REJECTED:
                    self.generate_order_rejected(
                        strategy_id=strategy_id,
                        instrument_id=report.instrument_id,
                        client_order_id=report.client_order_id,
                        reason=bybit_order.rejectReason,
                        ts_event=report.ts_last,
                    )
                elif bybit_order.orderStatus == BybitOrderStatus.NEW:
                    if order.status == OrderStatus.PENDING_UPDATE:
                        self.generate_order_updated(
                            strategy_id=strategy_id,
                            instrument_id=report.instrument_id,
                            client_order_id=report.client_order_id,
                            venue_order_id=report.venue_order_id,
                            quantity=report.quantity,
                            price=report.price,
                            trigger_price=report.trigger_price,
                            ts_event=report.ts_last,
                        )
                    else:
                        self.generate_order_accepted(
                            strategy_id=strategy_id,
                            instrument_id=report.instrument_id,
                            client_order_id=report.client_order_id,
                            venue_order_id=report.venue_order_id,
                            ts_event=report.ts_last,
                        )
                elif (
                    bybit_order.orderStatus == BybitOrderStatus.CANCELED
                    or bybit_order.orderStatus == BybitOrderStatus.DEACTIVATED
                ):
                    self.generate_order_canceled(
                        strategy_id=strategy_id,
                        instrument_id=report.instrument_id,
                        client_order_id=report.client_order_id,
                        venue_order_id=report.venue_order_id,
                        ts_event=report.ts_last,
                    )
                elif (
                    bybit_order.orderStatus == BybitOrderStatus.TRIGGERED
                    and order.order_type
                    not in (
                        OrderType.MARKET_IF_TOUCHED,
                        OrderType.STOP_MARKET,
                        OrderType.TRAILING_STOP_MARKET,
                    )
                ):
                    self.generate_order_triggered(
                        strategy_id=strategy_id,
                        instrument_id=report.instrument_id,
                        client_order_id=report.client_order_id,
                        venue_order_id=report.venue_order_id,
                        ts_event=report.ts_last,
                    )
                elif bybit_order.orderStatus == BybitOrderStatus.UNTRIGGERED:
                    self.generate_order_updated(
                        strategy_id=strategy_id,
                        instrument_id=report.instrument_id,
                        client_order_id=report.client_order_id,
                        venue_order_id=report.venue_order_id,
                        quantity=report.quantity,
                        price=report.price,
                        trigger_price=report.trigger_price,
                        ts_event=report.ts_last,
                    )
        except Exception as e:
            self._log.error(repr(e))

    def _create_market_batch_order(
        self,
        order: MarketOrder,
    ) -> BybitBatchPlaceOrder:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        return BybitBatchPlaceOrder(
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            orderType=BybitOrderType.MARKET,
            qty=str(order.quantity),
            marketUnit="baseCoin" if not order.is_quote_quantity else "quoteCoin",
            timeInForce=time_in_force,
            orderLinkId=str(order.client_order_id),
            reduceOnly=order.is_reduce_only if order.is_reduce_only else None,
        )

    def _create_limit_batch_order(
        self,
        order: LimitOrder,
    ) -> BybitBatchPlaceOrder:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        return BybitBatchPlaceOrder(
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            orderType=BybitOrderType.LIMIT,
            qty=str(order.quantity),
            marketUnit="baseCoin" if not order.is_quote_quantity else "quoteCoin",
            price=str(order.price),
            timeInForce=time_in_force,
            orderLinkId=str(order.client_order_id),
            reduceOnly=order.is_reduce_only if order.is_reduce_only else None,
        )

    def _create_limit_if_touched_batch_order(
        self,
        order: LimitIfTouchedOrder,
    ) -> BybitBatchPlaceOrder:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        trigger_direction = self._enum_parser.parse_trigger_direction(order.order_type, order.side)
        trigger_type = self._enum_parser.parse_nautilus_trigger_type(order.trigger_type)
        return BybitBatchPlaceOrder(
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            orderType=BybitOrderType.MARKET,
            qty=str(order.quantity),
            marketUnit="baseCoin" if not order.is_quote_quantity else "quoteCoin",
            price=str(order.price),
            timeInForce=time_in_force,
            orderLinkId=order.client_order_id.value,
            reduceOnly=order.reduce_only,
            tpslMode=BybitTpSlMode.PARTIAL if product_type != BybitProductType.SPOT else None,
            triggerPrice=str(order.trigger_price),
            triggerDirection=trigger_direction,
            triggerBy=trigger_type,
            takeProfit=str(order.trigger_price) if product_type == BybitProductType.SPOT else None,
            tpTriggerBy=trigger_type if product_type != BybitProductType.SPOT else None,
            tpLimitPrice=str(order.price) if product_type != BybitProductType.SPOT else None,
            tpOrderType=BybitOrderType.LIMIT,
        )

    def _create_stop_market_batch_order(
        self,
        order: MarketOrder,
    ) -> BybitBatchPlaceOrder:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        trigger_direction = self._enum_parser.parse_trigger_direction(order.order_type, order.side)
        trigger_type = self._enum_parser.parse_nautilus_trigger_type(order.trigger_type)
        return BybitBatchPlaceOrder(
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            orderType=BybitOrderType.MARKET,
            qty=str(order.quantity),
            marketUnit="baseCoin" if not order.is_quote_quantity else "quoteCoin",
            timeInForce=time_in_force,
            orderLinkId=str(order.client_order_id),
            reduceOnly=order.is_reduce_only if order.is_reduce_only else None,
            closeOnTrigger=True,  # Conservative for stop-loss orders
            tpslMode=BybitTpSlMode.FULL if product_type != BybitProductType.SPOT else None,
            stopLoss=str(order.trigger_price) if product_type == BybitProductType.SPOT else None,
            triggerDirection=trigger_direction if product_type != BybitProductType.SPOT else None,
            slTriggerBy=trigger_type if product_type != BybitProductType.SPOT else None,
            slOrderType=BybitOrderType.MARKET,
        )

    def _create_market_if_touched_batch_order(
        self,
        order: MarketOrder,
    ) -> BybitBatchPlaceOrder:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        product_type = bybit_symbol.product_type
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        trigger_direction = self._enum_parser.parse_trigger_direction(order.order_type, order.side)
        trigger_type = self._enum_parser.parse_nautilus_trigger_type(order.trigger_type)
        return BybitBatchPlaceOrder(
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            orderType=BybitOrderType.MARKET,
            qty=str(order.quantity),
            marketUnit="baseCoin" if not order.is_quote_quantity else "quoteCoin",
            timeInForce=time_in_force,
            orderLinkId=str(order.client_order_id),
            reduceOnly=order.is_reduce_only if order.is_reduce_only else None,
            tpslMode=BybitTpSlMode.FULL if product_type != BybitProductType.SPOT else None,
            triggerPrice=(
                str(order.trigger_price) if product_type == BybitProductType.SPOT else None
            ),
            triggerDirection=trigger_direction if product_type != BybitProductType.SPOT else None,
            slTriggerBy=trigger_type if product_type != BybitProductType.SPOT else None,
            slOrderType=BybitOrderType.MARKET,
        )
