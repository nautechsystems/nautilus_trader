# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from datetime import datetime
from typing import Any, Dict, List, Optional, Set

import msgspec

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceExecutionType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.functions import format_symbol
from nautilus_trader.adapters.binance.common.functions import parse_symbol
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesTimeInForce
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceError
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotEventType
from nautilus_trader.adapters.binance.spot.http.account import BinanceSpotAccountHttpAPI
from nautilus_trader.adapters.binance.spot.http.market import BinanceSpotMarketHttpAPI
from nautilus_trader.adapters.binance.spot.http.user import BinanceSpotUserDataHttpAPI
from nautilus_trader.adapters.binance.spot.parsing.account import parse_account_balances_http
from nautilus_trader.adapters.binance.spot.parsing.account import parse_account_balances_ws
from nautilus_trader.adapters.binance.spot.parsing.execution import binance_order_type
from nautilus_trader.adapters.binance.spot.parsing.execution import parse_order_report_http
from nautilus_trader.adapters.binance.spot.parsing.execution import parse_order_type
from nautilus_trader.adapters.binance.spot.parsing.execution import parse_time_in_force
from nautilus_trader.adapters.binance.spot.parsing.execution import parse_trade_report_http
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.adapters.binance.spot.rules import BINANCE_SPOT_VALID_ORDER_TYPES
from nautilus_trader.adapters.binance.spot.rules import BINANCE_SPOT_VALID_TIF
from nautilus_trader.adapters.binance.spot.schemas.account import BinanceSpotAccountInfo
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotAccountUpdateMsg
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotAccountUpdateWrapper
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotOrderUpdateData
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotOrderUpdateWrapper
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotUserMsgWrapper
from nautilus_trader.adapters.binance.websocket.client import BinanceWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import secs_to_millis
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.c_enums.order_type import OrderTypeParser
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderSideParser
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForceParser
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.market import MarketOrder
from nautilus_trader.model.orders.stop_limit import StopLimitOrder
from nautilus_trader.msgbus.bus import MessageBus


class BinanceSpotExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the `Binance Spot/Margin` exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BinanceHttpClient
        The binance HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    instrument_provider : BinanceInstrumentProvider
        The instrument provider.
    account_type : BinanceAccountType
        The account type for the client.
    base_url_ws : str, optional
        The base URL for the WebSocket client.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BinanceHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: BinanceSpotInstrumentProvider,
        account_type: BinanceAccountType = BinanceAccountType.SPOT,
        base_url_ws: Optional[str] = None,
    ):
        super().__init__(
            loop=loop,
            client_id=ClientId(BINANCE_VENUE.value),
            venue=BINANCE_VENUE,
            oms_type=OMSType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.CASH,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._binance_account_type = account_type
        self._log.info(f"Account type: {self._binance_account_type.value}.", LogColor.BLUE)

        self._set_account_id(AccountId(f"{BINANCE_VENUE.value}-spot-master"))

        # HTTP API
        self._http_client = client
        self._http_account = BinanceSpotAccountHttpAPI(client=client)
        self._http_market = BinanceSpotMarketHttpAPI(client=client)
        self._http_user = BinanceSpotUserDataHttpAPI(client=client, account_type=account_type)

        # Listen keys
        self._ping_listen_keys_interval: int = 60 * 5  # Once every 5 mins (hardcode)
        self._ping_listen_keys_task: Optional[asyncio.Task] = None
        self._listen_key: Optional[str] = None

        # WebSocket API
        self._ws_client = BinanceWebSocketClient(
            loop=loop,
            clock=clock,
            logger=logger,
            handler=self._handle_user_ws_message,
            base_url=base_url_ws,
        )

        # Hot caches
        self._instrument_ids: Dict[str, InstrumentId] = {}

        self._log.info(f"Base URL HTTP {self._http_client.base_url}.", LogColor.BLUE)
        self._log.info(f"Base URL WebSocket {base_url_ws}.", LogColor.BLUE)

    def connect(self) -> None:
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    def disconnect(self) -> None:
        self._log.info("Disconnecting...")
        self._loop.create_task(self._disconnect())

    async def _connect(self) -> None:
        # Connect HTTP client
        if not self._http_client.connected:
            await self._http_client.connect()
        try:
            await self._instrument_provider.initialize()
        except BinanceError as e:
            self._log.exception("Error on connect", e)
            return

        # Authenticate API key and update account(s)
        info: BinanceSpotAccountInfo = await self._http_account.account(recv_window=5000)

        self._authenticate_api_key(info=info)
        self._update_account_state(info=info)

        # Get listen keys
        response = await self._http_user.create_listen_key()

        self._listen_key = response["listenKey"]
        self._ping_listen_keys_task = self._loop.create_task(self._ping_listen_keys())
        self._log.info(f"Listen key {self._listen_key}")

        # Connect WebSocket client
        self._ws_client.subscribe(key=self._listen_key)
        await self._ws_client.connect()

        self._set_connected(True)
        self._log.info("Connected.")

    def _authenticate_api_key(self, info: BinanceSpotAccountInfo) -> None:
        if info.canTrade:
            self._log.info("Binance API key authenticated.", LogColor.GREEN)
            self._log.info(f"API key {self._http_client.api_key} has trading permissions.")
        else:
            self._log.error("Binance API key does not have trading permissions.")

    def _update_account_state(self, info: BinanceSpotAccountInfo) -> None:
        self.generate_account_state(
            balances=parse_account_balances_http(raw_balances=info.balances),
            margins=[],
            reported=True,
            ts_event=millis_to_nanos(info.updateTime),
        )

    async def _update_account_state_async(self) -> None:
        info: BinanceSpotAccountInfo = await self._http_account.account(recv_window=5000)
        self._update_account_state(info=info)

    async def _ping_listen_keys(self) -> None:
        while True:
            self._log.debug(
                f"Scheduled `ping_listen_keys` to run in " f"{self._ping_listen_keys_interval}s."
            )
            await asyncio.sleep(self._ping_listen_keys_interval)
            if self._listen_key:
                self._log.debug(f"Pinging WebSocket listen key {self._listen_key}...")
                await self._http_user.ping_listen_key(self._listen_key)

    async def _disconnect(self) -> None:
        # Cancel tasks
        if self._ping_listen_keys_task:
            self._log.debug("Canceling `ping_listen_keys` task...")
            self._ping_listen_keys_task.cancel()

        # Disconnect WebSocket clients
        if self._ws_client.is_connected:
            await self._ws_client.disconnect()

        # Disconnect HTTP client
        if self._http_client.connected:
            await self._http_client.disconnect()

        self._set_connected(False)
        self._log.info("Disconnected.")

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ) -> Optional[OrderStatusReport]:
        PyCondition.true(
            client_order_id is not None or venue_order_id is not None,
            "both `client_order_id` and `venue_order_id` were `None`",
        )

        self._log.info(
            f"Generating OrderStatusReport for "
            f"{repr(client_order_id) if client_order_id else ''} "
            f"{repr(venue_order_id) if venue_order_id else ''}..."
        )

        try:
            response = await self._http_account.get_order(
                symbol=instrument_id.symbol.value,
                order_id=venue_order_id.value,
            )
        except BinanceError as e:
            self._log.exception(
                f"Cannot generate order status report for {venue_order_id}.",
                e,
            )
            return None

        return parse_order_report_http(
            account_id=self.account_id,
            instrument_id=self._get_cached_instrument_id(response["symbol"]),
            data=response,
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

    async def generate_order_status_reports(  # noqa (C901 too complex)
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
        open_only: bool = False,
    ) -> List[OrderStatusReport]:
        self._log.info(f"Generating OrderStatusReports for {self.id}...")

        open_orders = self._cache.orders_open(venue=self.venue)
        active_symbols: Set[str] = {
            format_symbol(o.instrument_id.symbol.value) for o in open_orders
        }

        order_msgs = []
        reports: Dict[VenueOrderId, OrderStatusReport] = {}

        try:
            open_order_msgs: List[Dict[str, Any]] = await self._http_account.get_open_orders(
                symbol=instrument_id.symbol.value if instrument_id is not None else None,
            )
            if open_order_msgs:
                order_msgs.extend(open_order_msgs)
                # Add to active symbols
                for o in open_order_msgs:
                    active_symbols.add(o["symbol"])

            for symbol in active_symbols:
                response = await self._http_account.get_orders(
                    symbol=symbol,
                    start_time=secs_to_millis(start.timestamp()) if start is not None else None,
                    end_time=secs_to_millis(end.timestamp()) if end is not None else None,
                )
                order_msgs.extend(response)
        except BinanceError as e:
            self._log.exception("Cannot generate order status report: ", e)
            return []

        for msg in order_msgs:
            # Apply filter (always report open orders regardless of start, end filter)
            # TODO(cs): Time filter is WIP
            # timestamp = pd.to_datetime(data["time"], utc=True)
            # if data["status"] not in ("NEW", "PARTIALLY_FILLED", "PENDING_CANCEL"):
            #     if start is not None and timestamp < start:
            #         continue
            #     if end is not None and timestamp > end:
            #         continue

            report: OrderStatusReport = parse_order_report_http(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(msg["symbol"]),
                data=msg,
                report_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )

            self._log.debug(f"Received {report}.")
            reports[report.venue_order_id] = report  # One report per order

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} OrderStatusReport{plural}.")

        return list(reports.values())

    async def generate_trade_reports(  # noqa (C901 too complex)
        self,
        instrument_id: InstrumentId = None,
        venue_order_id: VenueOrderId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> List[TradeReport]:
        self._log.info(f"Generating TradeReports for {self.id}...")

        open_orders = self._cache.orders_open(venue=self.venue)
        active_symbols: Set[str] = {
            format_symbol(o.instrument_id.symbol.value) for o in open_orders
        }

        reports_raw: List[Dict[str, Any]] = []
        reports: List[TradeReport] = []

        try:
            for symbol in active_symbols:
                response = await self._http_account.get_account_trades(
                    symbol=symbol,
                    start_time=secs_to_millis(start.timestamp()) if start is not None else None,
                    end_time=secs_to_millis(end.timestamp()) if end is not None else None,
                )
                reports_raw.extend(response)
        except BinanceError as e:
            self._log.exception("Cannot generate trade report: ", e)
            return []

        for data in reports_raw:
            # Apply filter
            # TODO(cs): Time filter is WIP
            # timestamp = pd.to_datetime(data["time"], utc=True)
            # if start is not None and timestamp < start:
            #     continue
            # if end is not None and timestamp > end:
            #     continue

            report: TradeReport = parse_trade_report_http(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(data["symbol"]),
                data=data,
                report_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )

            self._log.debug(f"Received {report}.")
            reports.append(report)

        # Sort in ascending order
        reports = sorted(reports, key=lambda x: x.trade_id)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} TradeReport{plural}.")

        return reports

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> List[PositionStatusReport]:
        # Never cash positions

        return []

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    def submit_order(self, command: SubmitOrder) -> None:
        order: Order = command.order

        # Check order type valid
        if order.type not in BINANCE_SPOT_VALID_ORDER_TYPES:
            self._log.error(
                f"Cannot submit order: {OrderTypeParser.to_str_py(order.type)} "
                f"orders not supported by the Binance Spot/Margin exchange. "
                f"Use any of {[OrderTypeParser.to_str_py(t) for t in BINANCE_SPOT_VALID_ORDER_TYPES]}",
            )
            return

        # Check time in force valid
        if order.time_in_force not in BINANCE_SPOT_VALID_TIF:
            self._log.error(
                f"Cannot submit order: "
                f"{TimeInForceParser.to_str_py(order.time_in_force)} "
                f"not supported by the Binance Spot/Margin exchange. "
                f"Use any of {BINANCE_SPOT_VALID_TIF}.",
            )
            return

        # Check post-only
        if order.type == OrderType.STOP_LIMIT and order.is_post_only:
            self._log.error(
                "Cannot submit order: "
                "STOP_LIMIT `post_only` orders not supported by the Binance Spot/Margin exchange. "
                "This order may become a liquidity TAKER."
            )
            return

        self._log.debug(f"Submitting {order}.")

        # Generate event here to ensure correct ordering of events
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        self._loop.create_task(self._submit_order(order))

    def submit_order_list(self, command: SubmitOrderList) -> None:
        self._log.debug("Submitting Order List.")

        for order in command.list:
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

        self._loop.create_task(self._submit_order_list(command))

    def modify_order(self, command: ModifyOrder) -> None:
        self._log.error(  # pragma: no cover
            "Cannot modify order: Not supported by the exchange.",
        )

    def sync_order_status(self, command: QueryOrder) -> None:
        self._log.debug(f"Synchronizing order status {command}")
        self._loop.create_task(
            self.generate_order_status_report(
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
            )
        )

    def cancel_order(self, command: CancelOrder) -> None:
        self._log.debug(f"Canceling order {command.client_order_id.value}.")

        self.generate_order_pending_cancel(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        self._loop.create_task(self._cancel_order(command))

    def cancel_all_orders(self, command: CancelAllOrders) -> None:
        self._loop.create_task(self._cancel_all_orders(command))

    async def _submit_order(self, order: Order) -> None:

        try:
            if order.type == OrderType.MARKET:
                await self._submit_market_order(order)
            elif order.type == OrderType.LIMIT:
                await self._submit_limit_order(order)
            elif order.type in (OrderType.STOP_LIMIT, OrderType.LIMIT_IF_TOUCHED):
                await self._submit_stop_limit_order(order)
        except BinanceError as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=e.message,
                ts_event=self._clock.timestamp_ns(),
            )

    async def _submit_market_order(self, order: MarketOrder) -> None:
        await self._http_account.new_order(
            symbol=format_symbol(order.instrument_id.symbol.value),
            side=OrderSideParser.to_str_py(order.side),
            type="MARKET",
            quantity=str(order.quantity),
            new_client_order_id=order.client_order_id.value,
            recv_window=5000,
        )

    async def _submit_limit_order(self, order: LimitOrder) -> None:
        time_in_force = TimeInForceParser.to_str_py(order.time_in_force)
        if order.is_post_only:
            time_in_force = None

        await self._http_account.new_order(
            symbol=format_symbol(order.instrument_id.symbol.value),
            side=OrderSideParser.to_str_py(order.side),
            type=binance_order_type(order).value,
            time_in_force=time_in_force,
            quantity=str(order.quantity),
            price=str(order.price),
            iceberg_qty=str(order.display_qty) if order.display_qty is not None else None,
            new_client_order_id=order.client_order_id.value,
            recv_window=5000,
        )

    async def _submit_stop_limit_order(self, order: StopLimitOrder) -> None:
        await self._http_account.new_order(
            symbol=format_symbol(order.instrument_id.symbol.value),
            side=OrderSideParser.to_str_py(order.side),
            type=binance_order_type(order).value,
            time_in_force=TimeInForceParser.to_str_py(order.time_in_force),
            quantity=str(order.quantity),
            price=str(order.price),
            stop_price=str(order.trigger_price),
            iceberg_qty=str(order.display_qty) if order.display_qty is not None else None,
            new_client_order_id=order.client_order_id.value,
            recv_window=5000,
        )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        for order in command.list:
            if order.linked_order_ids:  # TODO(cs): Implement
                self._log.warning(f"Cannot yet handle OCO conditional orders, {order}.")
            await self._submit_order(order)

    async def _cancel_order(self, command: CancelOrder) -> None:

        try:
            if command.venue_order_id is not None:
                await self._http_account.cancel_order(
                    symbol=format_symbol(command.instrument_id.symbol.value),
                    order_id=command.venue_order_id.value,
                )
            else:
                await self._http_account.cancel_order(
                    symbol=format_symbol(command.instrument_id.symbol.value),
                    orig_client_order_id=command.client_order_id.value,
                )
        except BinanceError as e:
            self._log.exception(
                f"Cannot cancel order "
                f"ClientOrderId({command.client_order_id}), "
                f"VenueOrderId{command.venue_order_id}: ",
                e,
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        self._log.debug(f"Canceling all orders for {command.instrument_id.value}.")

        # Cancel all in-flight orders
        inflight_orders = self._cache.orders_inflight(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )
        for order in inflight_orders:
            self.generate_order_pending_cancel(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

        # Cancel all open orders
        open_orders = self._cache.orders_open(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )
        for order in open_orders:
            self.generate_order_pending_cancel(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

        try:
            await self._http_account.cancel_open_orders(
                symbol=format_symbol(command.instrument_id.symbol.value),
            )
        except BinanceError as e:
            self._log.exception("Cannot cancel open orders: ", e)

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        # Parse instrument ID
        nautilus_symbol: str = parse_symbol(symbol, account_type=self._binance_account_type)
        instrument_id: Optional[InstrumentId] = self._instrument_ids.get(nautilus_symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(nautilus_symbol), BINANCE_VENUE)
            self._instrument_ids[nautilus_symbol] = instrument_id
        return instrument_id

    def _handle_user_ws_message(self, raw: bytes) -> None:
        # TODO(cs): Uncomment for development
        # self._log.info(str(json.dumps(msgspec.json.decode(raw), indent=4)), color=LogColor.MAGENTA)

        wrapper = msgspec.json.decode(raw, type=BinanceSpotUserMsgWrapper)

        try:
            if wrapper.data.e == BinanceSpotEventType.outboundAccountPosition:
                msg = msgspec.json.decode(raw, type=BinanceSpotAccountUpdateWrapper)
                self._handle_account_update(msg.data)
            elif wrapper.data.e == BinanceSpotEventType.executionReport:
                msg = msgspec.json.decode(raw, type=BinanceSpotOrderUpdateWrapper)
                self._handle_execution_report(msg.data)
            elif wrapper.data.e == BinanceSpotEventType.listStatus:
                pass  # Implement (OCO order status)
            elif wrapper.data.e == BinanceSpotEventType.balanceUpdate:
                self._loop.create_task(self._update_account_state_async())
        except Exception as e:
            self._log.exception(f"Error on handling {repr(raw)}", e)

    def _handle_account_update(self, msg: BinanceSpotAccountUpdateMsg) -> None:
        self.generate_account_state(
            balances=parse_account_balances_ws(raw_balances=msg.B),
            margins=[],
            reported=True,
            ts_event=millis_to_nanos(msg.u),
        )

    def _handle_execution_report(self, data: BinanceSpotOrderUpdateData) -> None:
        instrument_id: InstrumentId = self._get_cached_instrument_id(data.s)
        venue_order_id = VenueOrderId(str(data.i))
        ts_event = millis_to_nanos(data.T)

        # Parse client order ID
        client_order_id_str: str = data.c
        if not client_order_id_str or not client_order_id_str.startswith("O"):
            client_order_id_str = data.C
        client_order_id = ClientOrderId(client_order_id_str)

        # Fetch strategy ID
        strategy_id: StrategyId = self._cache.strategy_id_for_order(client_order_id)
        if strategy_id is None:
            if strategy_id is None:
                self._generate_external_order_report(
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    data,
                    ts_event,
                )
                return

        if data.x == BinanceExecutionType.NEW:
            self.generate_order_accepted(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
        elif data.x == BinanceExecutionType.TRADE:
            instrument: Instrument = self._instrument_provider.find(instrument_id=instrument_id)

            # Determine commission
            commission_asset: str = data.N
            commission_amount: str = data.n
            if commission_asset is not None:
                commission = Money.from_str(f"{commission_amount} {commission_asset}")
            else:
                # Binance typically charges commission as base asset or BNB
                commission = Money(0, instrument.base_currency)

            self.generate_order_filled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                venue_position_id=None,  # NETTING accounts
                trade_id=TradeId(str(data.t)),  # Trade ID
                order_side=OrderSideParser.from_str_py(data.S.value),
                order_type=parse_order_type(data.o),
                last_qty=Quantity.from_str(data.l),
                last_px=Price.from_str(data.L),
                quote_currency=instrument.quote_currency,
                commission=commission,
                liquidity_side=LiquiditySide.MAKER if data.m else LiquiditySide.TAKER,
                ts_event=ts_event,
            )
        elif data.x in (BinanceExecutionType.CANCELED, BinanceExecutionType.EXPIRED):
            self.generate_order_canceled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
        else:
            self._log.warning(f"Received unhandled {data}")

    def _generate_external_order_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        data: BinanceSpotOrderUpdateData,
        ts_event: int,
    ) -> None:
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            order_side=OrderSide.BUY if data.S == BinanceOrderSide.BUY else OrderSide.SELL,
            order_type=parse_order_type(data.o),
            time_in_force=parse_time_in_force(data.f.value),
            order_status=OrderStatus.ACCEPTED,
            price=Price.from_str(data.p) if data.p is not None else None,
            trigger_price=Price.from_str(data.P) if data.P is not None else None,
            trigger_type=TriggerType.LAST,
            trailing_offset=None,
            trailing_offset_type=TrailingOffsetType.NONE,
            quantity=Quantity.from_str(data.q),
            filled_qty=Quantity.from_str(data.z),
            display_qty=Quantity.from_str(data.F) if data.F is not None else None,
            avg_px=None,
            post_only=data.f == BinanceFuturesTimeInForce.GTX,
            reduce_only=False,
            report_id=UUID4(),
            ts_accepted=ts_event,
            ts_last=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_status_report(report)
