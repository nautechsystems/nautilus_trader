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
import uuid
from datetime import datetime
from decimal import Decimal
from typing import Any, Dict, List, Optional

import msgspec
import pandas as pd

from nautilus_trader.accounting.accounts.margin import MarginAccount
from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.adapters.ftx.core.constants import FTX_VENUE
from nautilus_trader.adapters.ftx.http.client import FTXHttpClient
from nautilus_trader.adapters.ftx.http.error import FTXError
from nautilus_trader.adapters.ftx.parsing.common import parse_order_type
from nautilus_trader.adapters.ftx.parsing.common import parse_position_report
from nautilus_trader.adapters.ftx.parsing.common import parse_trade_report
from nautilus_trader.adapters.ftx.parsing.http import parse_order_status_http
from nautilus_trader.adapters.ftx.parsing.http import parse_trigger_order_status_http
from nautilus_trader.adapters.ftx.providers import FTXInstrumentProvider
from nautilus_trader.adapters.ftx.websocket.client import FTXWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderSideParser
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import OrderTypeParser
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.market import MarketOrder
from nautilus_trader.model.orders.stop_limit import StopLimitOrder
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from nautilus_trader.model.orders.trailing_stop_market import TrailingStopMarketOrder
from nautilus_trader.model.position import Position
from nautilus_trader.msgbus.bus import MessageBus


class FTXExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for FTX exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : FTXHttpClient
        The FTX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    instrument_provider : FTXInstrumentProvider
        The instrument provider.
    us : bool, default False
        If the client is for FTX US.
    account_polling_interval : int, default 60
        The interval length (seconds) between account reconciliations.
    trigger_polling_interval : float, default 1.0
        The interval length (seconds) between polling for open trigger order state.
    calculated_account : bool, default False
        If the account state will be calculated internally from each order fill.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: FTXHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: FTXInstrumentProvider,
        us: bool = False,
        trigger_polling_interval: float = 1.0,
        account_polling_interval: int = 60,
        calculated_account: bool = False,
    ):
        super().__init__(
            loop=loop,
            client_id=ClientId(FTX_VENUE.value),
            venue=FTX_VENUE,
            oms_type=OMSType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._http_client = client
        self._ws_client = FTXWebSocketClient(
            loop=loop,
            clock=clock,
            logger=logger,
            msg_handler=self._handle_ws_message,
            reconnect_handler=self._handle_ws_reconnect,
            key=client.api_key,
            secret=client.api_secret,
            us=us,
            auto_ping_interval=15.0,  # Recommended by FTX
            # log_send=True,  # Uncomment for development and debugging
            # log_recv=True,  # Uncomment for development and debugging
        )
        self._ws_buffer: List[bytes] = []

        # Tasks
        self._task_poll_account: Optional[asyncio.Task] = None
        self._task_buffer_ws_msgs: Optional[asyncio.Task] = None

        # Hot Caches
        self._instrument_ids: Dict[str, InstrumentId] = {}
        self._order_ids: Dict[VenueOrderId, ClientOrderId] = {}
        self._order_types: Dict[ClientOrderId, OrderType] = {}
        self._open_triggers: Dict[str, ClientOrderId] = {}

        # Settings
        self._trigger_polling_interval = trigger_polling_interval
        self._account_polling_interval = account_polling_interval
        self._calculated_account = calculated_account
        self._initial_leverage_set = False

        if us:
            self._log.info("Set FTX US.", LogColor.BLUE)

        self._log.info(
            f"Set account polling interval {self._account_polling_interval}s.",
            LogColor.BLUE,
        )

        if self._calculated_account:
            self._log.info("Set calculated account.", LogColor.BLUE)
            AccountFactory.register_calculated_account(FTX_VENUE.value)

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
        except FTXError as e:
            self._log.exception("Error on connect", e)
            return

        self._log.info("FTX API key authenticated.", LogColor.GREEN)
        self._log.info(f"API key {self._http_client.api_key}.")

        # Begin polling trigger orders
        self._task_poll_trigger_orders = self._loop.create_task(self._poll_trigger_orders())

        # Update account state
        await self._update_account_state()
        self._task_poll_account = self._loop.create_task(self._poll_account_state())

        # Connect WebSocket client
        await self._ws_client.connect(start=True)
        await self._ws_client.subscribe_fills()
        await self._ws_client.subscribe_orders()

        self._set_connected(True)
        self._log.info("Connected.")

    async def _disconnect(self) -> None:
        if self._task_poll_trigger_orders:
            self._task_poll_trigger_orders.cancel()

        if self._task_poll_account:
            self._task_poll_account.cancel()

        # Disconnect WebSocket client
        if self._ws_client.is_connected:
            await self._ws_client.disconnect()
            await self._ws_client.close()

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
            response = await self._http_client.get_order_status(venue_order_id.value)
        except FTXError as e:
            order_id_str = venue_order_id.value if venue_order_id is not None else "ALL orders"
            self._log.error(
                f"Cannot get order status for {order_id_str}: {e.message}",
            )
            return None

        # Get instrument
        instrument_id = self._get_cached_instrument_id(response["market"])
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot generate order status report: "
                f"no instrument found for {instrument_id}.",
            )
            return None

        return parse_order_status_http(
            account_id=self.account_id,
            instrument=instrument,
            data=response,
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
        open_only: bool = False,
    ) -> List[OrderStatusReport]:
        self._log.info(f"Generating OrderStatusReports for {self.id}...")

        reports: List[OrderStatusReport] = []
        reports += await self._get_order_status_reports(
            instrument_id=instrument_id,
            start=start,
            end=end,
            open_only=open_only,
        )

        reports += await self._get_trigger_order_status_reports(
            instrument_id=instrument_id,
            start=start,
            end=end,
            open_only=open_only,
        )

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} OrderStatusReport{plural}.")

        return reports

    async def _get_order_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
        open_only: bool = False,
    ) -> List[OrderStatusReport]:
        reports: List[OrderStatusReport] = []

        try:
            if open_only:
                response: List[Dict[str, Any]] = await self._http_client.get_open_orders(
                    market=instrument_id.symbol.value if instrument_id is not None else None,
                )
            else:
                response = await self._http_client.get_order_history(
                    market=instrument_id.symbol.value if instrument_id is not None else None,
                )
        except FTXError as e:
            self._log.exception("Cannot generate order status report: ", e)
            return []

        if response:
            for data in response:
                # Apply filter (FTX filters not working)
                created_at = pd.to_datetime(data["createdAt"], utc=True)
                if start is not None and created_at < start:
                    continue
                if end is not None and created_at > end:
                    continue

                # Get instrument
                instrument_id = self._get_cached_instrument_id(data["market"])
                instrument = self._instrument_provider.find(instrument_id)
                if instrument is None:
                    self._log.error(
                        f"Cannot generate order status report: "
                        f"no instrument found for {instrument_id}.",
                    )
                    continue

                report: OrderStatusReport = parse_order_status_http(
                    account_id=self.account_id,
                    instrument=instrument,
                    data=data,
                    report_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                )

                self._log.debug(f"Received {report}.")
                reports.append(report)

        return reports

    async def _get_trigger_order_status_reports(  # noqa (cyclomatic complexity)
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
        open_only: bool = False,
    ) -> List[OrderStatusReport]:
        reports: List[OrderStatusReport] = []

        try:
            if open_only:
                response: List[Dict[str, Any]] = await self._http_client.get_open_trigger_orders(
                    market=instrument_id.symbol.value if instrument_id is not None else None,
                )
            else:
                response = await self._http_client.get_trigger_order_history(
                    market=instrument_id.symbol.value if instrument_id is not None else None,
                    start_time=int(start.timestamp()) if start is not None else None,
                    end_time=int(end.timestamp()) if end is not None else None,
                )

            trigger_reports = await asyncio.gather(
                *[self._http_client.get_trigger_order_triggers(r["id"]) for r in response]
            )

            # TODO(cs): Uncomment for development
            # self._log.info(str(response), LogColor.CYAN)

            # Build map of trigger order IDs to parent venue order IDs
            for idx, triggers in enumerate(trigger_reports):
                for trigger in triggers:
                    venue_order_id = VenueOrderId(str(trigger.get("orderId")))
                    client_order_id = self._cache.client_order_id(venue_order_id)
                    if client_order_id is not None:
                        self._open_triggers[str(response[idx]["id"])] = client_order_id

            # TODO(cs): Uncomment for development
            # self._log.info(str(self._open_triggers), LogColor.GREEN)
        except FTXError as e:
            self._log.exception("Cannot generate trade report: ", e)
            return []

        if response:
            for data in response:
                # Apply filter (FTX filters not working)
                created_at = pd.to_datetime(data["createdAt"], utc=True)
                if start is not None and created_at < start:
                    continue
                if end is not None and created_at > end:
                    continue

                # Get instrument
                instrument_id = self._get_cached_instrument_id(data["market"])
                instrument = self._instrument_provider.find(instrument_id)
                if instrument is None:
                    self._log.error(
                        f"Cannot generate order status report: "
                        f"no instrument found for {instrument_id}.",
                    )
                    continue

                self._log.info(str(data), LogColor.MAGENTA)
                report: OrderStatusReport = parse_trigger_order_status_http(
                    account_id=self.account_id,
                    instrument=instrument,
                    triggers=self._open_triggers,
                    data=data,
                    report_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                )

                self._log.debug(f"Received {report}.", LogColor.MAGENTA)
                reports.append(report)

        return reports

    async def generate_trade_reports(
        self,
        instrument_id: InstrumentId = None,
        venue_order_id: VenueOrderId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> List[TradeReport]:
        self._log.info(f"Generating TradeReports for {self.id}...")

        reports: List[TradeReport] = []

        try:
            fills_response: List[Dict[str, Any]] = await self._http_client.get_fills(
                market=instrument_id.symbol.value if instrument_id is not None else None,
                start_time=int(start.timestamp()) if start is not None else None,
                end_time=int(end.timestamp()) if end is not None else None,
            )

            # TODO(cs): Uncomment for development
            # trigger_response = await self._http_client.get_trigger_order_history(
            #     market=instrument_id.symbol.value if instrument_id is not None else None,
            #     start_time=int(start.timestamp()) if start is not None else None,
            #     end_time=int(end.timestamp()) if end is not None else None,
            # )
            #
            # trigger_ids = [str(r["id"]) for r in trigger_response]
            # trigger_reports = await asyncio.gather(
            #     *[self._http_client.get_trigger_order_triggers(t) for t in trigger_ids]
            # )

            # trigger_map = {
            #     trigger_ids[idx]: trigger_reports[idx] for idx, _ in enumerate(trigger_ids)
            # }
            # self._log.info(str(trigger_map), LogColor.CYAN)
        except FTXError as e:
            self._log.exception("Cannot generate trade report: ", e)
            return []

        # self._log.info(trigger_reports, LogColor.GREEN)

        if fills_response:
            for data in fills_response:
                # Apply filter (FTX filters not working)
                created_at = pd.to_datetime(data["time"], utc=True)
                if start is not None and created_at < start:
                    continue
                if end is not None and created_at > end:
                    continue

                # Get instrument
                instrument_id = self._get_cached_instrument_id(data["market"])
                instrument = self._instrument_provider.find(instrument_id)
                if instrument is None:
                    self._log.error(
                        f"Cannot generate trade report: "
                        f"no instrument found for {instrument_id}.",
                    )
                    continue

                report: TradeReport = parse_trade_report(
                    account_id=self.account_id,
                    instrument=instrument,
                    data=data,
                    report_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                )

                reports.append(report)

        # Sort in ascending order (adding 'order' to `get_fills()` breaks the client)
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
        self._log.info(f"Generating PositionStatusReports for {self.id}...")

        reports: List[PositionStatusReport] = []

        try:
            response: List[Dict[str, Any]] = await self._http_client.get_positions()
        except FTXError as e:
            self._log.exception("Cannot generate position status report: ", e)
            return []

        if response:
            for data in response:
                # Get instrument
                instrument_id = self._get_cached_instrument_id(data["future"])
                instrument = self._instrument_provider.find(instrument_id)
                if instrument is None:
                    self._log.error(
                        f"Cannot generate position status report: "
                        f"no instrument found for {instrument_id}.",
                    )
                    continue

                report: PositionStatusReport = parse_position_report(
                    account_id=self.account_id,
                    instrument=instrument,
                    data=data,
                    report_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                )

                if report.quantity == 0:
                    continue  # Flat position
                self._log.debug(f"Received {report}.")
                reports.append(report)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} PositionStatusReport{plural}.")

        return reports

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    def submit_order(self, command: SubmitOrder) -> None:
        position: Optional[Position] = None
        if command.position_id is not None:
            position = self._cache.position(command.position_id)
            if position is None:
                self._log.error(
                    f"Cannot submit order {command.order}: "
                    f"position ID {command.position_id} not found.",
                )
                return

        if command.order.type == OrderType.TRAILING_STOP_MARKET:
            if command.order.trigger_price is not None:
                self._log.warning(
                    "TrailingStopMarketOrder has specified a `trigger_price`, "
                    "however FTX will use the delta of current market price and "
                    "`trailing_offset` as the placed `trigger_price`.",
                )
        elif command.order.type == OrderType.TRAILING_STOP_LIMIT:
            if command.order.trigger_price is not None or command.order.price is not None:
                self._log.warning(
                    "TrailingStopLimitOrder has specified a `trigger_price` and/or "
                    "a `price` however FTX will use the delta of current market "
                    "price and `trailing_offset` as the placed `trigger_price`.",
                )

        self._loop.create_task(self._submit_order(command.order, position))

    def submit_order_list(self, command: SubmitOrderList) -> None:
        # TODO: Implement
        self._log.error(
            f"Cannot process command {command}. Not implemented in this version.",
        )

    def modify_order(self, command: ModifyOrder) -> None:
        self._loop.create_task(self._modify_order(command))

    def cancel_order(self, command: CancelOrder) -> None:
        self._loop.create_task(self._cancel_order(command))

    def cancel_all_orders(self, command: CancelAllOrders) -> None:
        self._loop.create_task(self._cancel_all_orders(command))

    async def _submit_order(self, order: Order, position: Optional[Position]) -> None:
        self._log.debug(f"Submitting {order}.")

        # Generate event here to ensure correct ordering of events
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        self._order_types[order.client_order_id] = order.type

        try:
            if order.type == OrderType.MARKET:
                await self._submit_market_order(order)
            elif order.type == OrderType.LIMIT:
                await self._submit_limit_order(order)
            elif order.type in (OrderType.STOP_MARKET, OrderType.MARKET_IF_TOUCHED):
                await self._submit_stop_market_order(order, position)
            elif order.type in (OrderType.STOP_LIMIT, OrderType.LIMIT_IF_TOUCHED):
                await self._submit_stop_limit_order(order, position)
            elif order.type == OrderType.TRAILING_STOP_MARKET:
                await self._submit_trailing_stop_market(order)
            elif order.type == OrderType.TRAILING_STOP_LIMIT:
                self._log.error(
                    f"Cannot submit order: {OrderTypeParser.to_str_py(order.type)} "
                    "order type is not supported by the FTX exchange. "
                    "Please try submitting as a `TRAILING_STOP_MARKET` order type."
                )
                return
            else:
                self._log.error(
                    f"Cannot submit order: {OrderTypeParser.to_str_py(order.type)} "
                    "order type is not implemented."
                )
        except FTXError as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=e.message,
                ts_event=self._clock.timestamp_ns(),  # TODO(cs): Parse from response
            )
        except Exception as e:  # Catch all exceptions for now
            self._log.exception(
                f"Error on submit {repr(order)}"
                f"{f'for {position}' if position is not None else ''}",
                e,
            )

    async def _submit_market_order(self, order: MarketOrder) -> None:
        await self._http_client.place_order(
            market=order.instrument_id.symbol.value,
            side=OrderSideParser.to_str_py(order.side).lower(),
            size=str(order.quantity),
            order_type="market",
            client_id=order.client_order_id.value,
            ioc=order.time_in_force == TimeInForce.IOC,
            reduce_only=order.is_reduce_only,
        )

    async def _submit_limit_order(self, order: LimitOrder) -> None:
        await self._http_client.place_order(
            market=order.instrument_id.symbol.value,
            side=OrderSideParser.to_str_py(order.side).lower(),
            size=str(order.quantity),
            order_type="limit",
            client_id=order.client_order_id.value,
            price=str(order.price),
            ioc=order.time_in_force == TimeInForce.IOC,
            reduce_only=order.is_reduce_only,
            post_only=order.is_post_only,
        )

    async def _submit_stop_market_order(
        self,
        order: StopMarketOrder,
        position: Optional[Position],
    ) -> None:
        order_type = "stop"
        if position is not None:
            if order.is_buy and order.trigger_price < position.avg_px_open:
                order_type = "takeProfit"
            elif order.is_sell and order.trigger_price > position.avg_px_open:
                order_type = "takeProfit"
        response = await self._http_client.place_trigger_order(
            market=order.instrument_id.symbol.value,
            side=OrderSideParser.to_str_py(order.side).lower(),
            size=str(order.quantity),
            order_type=order_type,
            client_id=order.client_order_id.value,
            trigger_price=str(order.trigger_price),
            reduce_only=order.is_reduce_only,
        )

        # Hot cache identifiers
        trigger_id = str(response["id"])
        venue_order_id = VenueOrderId(trigger_id)
        self._open_triggers[trigger_id] = order.client_order_id
        self._order_ids[venue_order_id] = order.client_order_id

        self.generate_order_accepted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            ts_event=pd.to_datetime(response["createdAt"], utc=True).value,
        )

    async def _submit_stop_limit_order(
        self,
        order: StopLimitOrder,
        position: Optional[Position],
    ) -> None:
        order_type = "stop"
        if position is not None:
            if order.is_buy and order.trigger_price < position.avg_px_open:
                order_type = "takeProfit"
            elif order.is_sell and order.trigger_price > position.avg_px_open:
                order_type = "takeProfit"
        response = await self._http_client.place_trigger_order(
            market=order.instrument_id.symbol.value,
            side=OrderSideParser.to_str_py(order.side).lower(),
            size=str(order.quantity),
            order_type=order_type,
            client_id=order.client_order_id.value,
            price=str(order.price),
            trigger_price=str(order.trigger_price),
            reduce_only=order.is_reduce_only,
        )

        # Hot cache identifiers
        trigger_id = str(response["id"])
        venue_order_id = VenueOrderId(trigger_id)
        self._open_triggers[trigger_id] = order.client_order_id
        self._order_ids[venue_order_id] = order.client_order_id

        self.generate_order_accepted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            ts_event=pd.to_datetime(response["createdAt"], utc=True).value,
        )

    async def _submit_trailing_stop_market(self, order: TrailingStopMarketOrder) -> None:
        if order.trigger_price is not None:
            self._log.error(
                f"Cannot submit order: {OrderTypeParser.to_str_py(order.type)}. "
                "Specifying a `trigger_price` is not supported by the FTX exchange. "
                "Please try submitting with a `trailing_offset` value."
            )
            return
        response = await self._http_client.place_trigger_order(
            market=order.instrument_id.symbol.value,
            side=OrderSideParser.to_str_py(order.side).lower(),
            size=str(order.quantity),
            order_type="trailingStop",
            client_id=order.client_order_id.value,
            trail_value=str(order.trailing_offset) if order.is_buy else str(-order.trailing_offset),
            reduce_only=order.is_reduce_only,
        )

        # Hot cache identifiers
        trigger_id = str(response["id"])
        venue_order_id = VenueOrderId(trigger_id)
        self._open_triggers[trigger_id] = order.client_order_id
        self._order_ids[venue_order_id] = order.client_order_id

        # Get instrument
        instrument_id: InstrumentId = self._get_cached_instrument_id(response["market"])
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot handle ws message: no instrument found for {instrument_id}.",
            )
            return

        ts_event: int = pd.to_datetime(response["createdAt"], utc=True).value

        self.generate_order_accepted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            ts_event=ts_event,
        )
        self.generate_order_updated(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            quantity=order.quantity,
            price=None,
            trigger_price=Price(response["triggerPrice"], precision=instrument.price_precision),
            ts_event=ts_event,
            venue_order_id_modified=True,  # Work around since venue order ID not assigned yet
        )

    async def _modify_order(self, command: ModifyOrder) -> None:
        self._log.debug(f"Modifying order {command.client_order_id.value}.")

        self.generate_order_pending_update(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            ts_event=self._clock.timestamp_ns(),
        )
        try:
            await self._http_client.modify_order(
                client_order_id=command.client_order_id.value,
                price=str(command.price) if command.price else None,
                size=str(command.quantity) if command.quantity else None,
            )
        except FTXError as e:
            self._log.error(f"Cannot modify order {command.venue_order_id}: {e.message}")

    async def _cancel_order(self, command: CancelOrder) -> None:
        self._log.debug(f"Canceling order {command.client_order_id.value}.")

        self.generate_order_pending_cancel(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            ts_event=self._clock.timestamp_ns(),
        )
        try:
            if command.venue_order_id.value in self._open_triggers:
                response: str = await self._http_client.cancel_open_trigger_order(
                    command.venue_order_id.value
                )
                if response == "Order cancelled":
                    self.generate_order_canceled(
                        strategy_id=command.strategy_id,
                        instrument_id=command.instrument_id,
                        client_order_id=command.client_order_id,
                        venue_order_id=command.venue_order_id,
                        ts_event=self._clock.timestamp_ns(),
                    )
                    self._open_triggers.pop(command.venue_order_id.value, None)
                else:
                    self._log.error(
                        f"Error canceling open trigger order "
                        f"{command.venue_order_id.value}, {response}",
                    )
            elif command.venue_order_id is not None:
                await self._http_client.cancel_order(command.venue_order_id.value)
            else:
                await self._http_client.cancel_order_by_client_id(command.client_order_id.value)
        except FTXError as e:
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

        open_triggers = await self._http_client.get_open_trigger_orders(command.instrument_id.value)

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
            await self._http_client.cancel_all_orders(command.instrument_id.symbol.value)
        except FTXError as e:
            self._log.error(f"Cannot cancel all orders: {e.message}")

        for trigger_info in open_triggers:
            trigger_id = str(trigger_info["id"])
            client_order_id = self._open_triggers.pop(trigger_id, None)
            if client_order_id is None:
                self._log.warning(
                    f"No client order ID found to generate order canceled "
                    f"for trigger ID {trigger_id}."
                )
                continue
            self.generate_order_canceled(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=client_order_id,
                venue_order_id=VenueOrderId(trigger_id),
                ts_event=self._clock.timestamp_ns(),
            )

    def _handle_ws_reconnect(self) -> None:
        self._loop.create_task(self._ws_reconnect_async())

    async def _ws_reconnect_async(self) -> None:
        report: ExecutionMassStatus = await self.generate_mass_status(lookback_mins=1)
        self._send_mass_status_report(report)

        await self._update_account_state()

    async def _buffer_ws_msgs(self) -> None:
        self._log.debug("Monitoring reconciliation...")
        while self.reconciliation_active:
            await self.sleep0()  # noqa (is a coroutine)

        if self._ws_buffer:
            self._log.debug(
                f"Draining {len(self._ws_buffer)} msgs from ws buffer...",
            )

        # Drain buffered websocket messages
        while self._ws_buffer:
            # Pop in received order
            raw: bytes = self._ws_buffer.pop(0)
            self._log.debug(f"Drained {str(raw)}.")
            self._handle_ws_message(raw)

        self._task_buffer_ws_msgs = None

    async def _poll_account_state(self) -> None:
        while True:
            await asyncio.sleep(self._account_polling_interval)
            await self._update_account_state()

    async def _update_account_state(self) -> None:
        self._log.debug("Updating account state...")

        response: Dict[str, Any] = await self._http_client.get_account_info()
        if self.account_id is None:
            self._set_account_id(AccountId(f"{FTX_VENUE.value}-{response['accountIdentifier']}"))

        self._handle_account_info(response)

        if not self._initial_leverage_set:
            account: Optional[MarginAccount] = self._cache.account(self.account_id)
            while account is None:
                self._log.debug(f"Waiting for account {self.account_id}...")
                await self.sleep0()
            leverage = Decimal(response["leverage"])
            account.set_default_leverage(leverage)
            self._log.info(
                f"Setting {self.account_id} default leverage to {leverage}X.",
                LogColor.BLUE,
            )
            instruments: List[Instrument] = self._instrument_provider.list_all()
            for instrument in instruments:
                if isinstance(instrument, CurrencyPair):
                    self._log.debug(
                        f"Setting {self.account_id} leverage for {instrument.id} to 1X.",
                    )
                    account.set_leverage(instrument.id, Decimal(1))  # No leverage

            self._initial_leverage_set = True

    def _handle_account_info(self, info: Dict[str, Any]) -> None:
        total = Money(info["totalAccountValue"], USD)
        free = Money(info["freeCollateral"], USD)
        locked = Money(total - free, USD)

        balance = AccountBalance(
            total=total,
            locked=locked,
            free=free,
        )

        # TODO(cs): Uncomment for development
        # self._log.info(str(json.dumps(info, indent=4)), color=LogColor.GREEN)

        margins: List[MarginBalance] = []

        # TODO(cs): Margins on FTX are fractions - determine solution
        # for position in info["positions"]:
        #     margin = MarginBalance(
        #         initial=Money(position["initialMarginRequirement"], USD),
        #         maintenance=Money(position["maintenanceMarginRequirement"], USD),
        #         instrument_id=InstrumentId(Symbol(position["future"]), FTX_VENUE),
        #     )
        #     margins.append(margin)

        self.generate_account_state(
            balances=[balance],
            margins=margins,
            reported=True,
            ts_event=self._clock.timestamp_ns(),
            info=info,
        )

        self._log.info(
            f"initialMarginRequirement={info['initialMarginRequirement']}, "
            f"maintenanceMarginRequirement={info['maintenanceMarginRequirement']}, "
            f"marginFraction={info['marginFraction']}, "
            f"openMarginFraction={info['openMarginFraction']}, "
            f"totalAccountValue={info['totalAccountValue']}, "
            f"totalPositionSize={info['totalPositionSize']}",
            LogColor.BLUE,
        )

    async def _poll_trigger_orders(self) -> None:
        while True:
            await asyncio.sleep(self._trigger_polling_interval)
            await self._update_trigger_order_states()

    def _is_trigger_order(self, order_type: OrderType) -> bool:
        return order_type in (
            OrderType.STOP_MARKET,
            OrderType.STOP_LIMIT,
            OrderType.MARKET_IF_TOUCHED,
            OrderType.LIMIT_IF_TOUCHED,
            OrderType.TRAILING_STOP_MARKET,
        )

    async def _update_trigger_order_states(self):
        open_trigger_orders = [
            o for o in self._cache.orders_open(venue=self.venue) if self._is_trigger_order(o.type)
        ]
        open_markets = {o.instrument_id for o in open_trigger_orders}
        for instrument_id in open_markets:
            triggers = await self._http_client.get_open_trigger_orders(
                market=instrument_id.symbol.value
            )
            open_venue_order_ids = set()
            # Check manual for price and trigger_price updates
            for trigger in triggers:
                venue_order_id = VenueOrderId(str(trigger["id"]))
                open_venue_order_ids.add(venue_order_id)
                client_order_id = self._cache.client_order_id(venue_order_id)
                order = self._cache.order(client_order_id)
                if order is None:
                    self._log.error(
                        f"Cannot find trigger order to update for "
                        f"{repr(client_order_id), repr(venue_order_id)}"
                    )
                    continue

                instrument = self._instrument_provider.find(instrument_id)
                if instrument is None:
                    self._log.error(
                        f"Cannot update trigger order: "
                        f"no instrument found for {instrument_id}.",
                    )
                    continue
                price_value = trigger.get("orderPrice")
                trigger_price_value = trigger.get("triggerPrice")
                price = (
                    Price(price_value, instrument.price_precision)
                    if price_value is not None
                    else None
                )
                trigger_price = (
                    Price(trigger_price_value, instrument.price_precision)
                    if trigger_price_value is not None
                    else None
                )

                if (price and price != order.price) or (
                    trigger_price and trigger_price != order.trigger_price
                ):
                    self.generate_order_updated(
                        strategy_id=self._cache.strategy_id_for_order(client_order_id),
                        instrument_id=instrument.id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        quantity=order.quantity,
                        price=price,
                        trigger_price=trigger_price,
                        ts_event=self._clock.timestamp_ns(),
                    )

            # Check for manual trigger cancels for instrument
            for order in self._cache.orders_open(venue=self.venue, instrument_id=instrument_id):
                if order.venue_order_id not in open_venue_order_ids:
                    # Fetch strategy ID
                    self.generate_order_canceled(
                        strategy_id=self._cache.strategy_id_for_order(order.client_order_id),
                        instrument_id=instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=order.venue_order_id,
                        ts_event=self._clock.timestamp_ns(),
                    )

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        # Parse instrument ID
        instrument_id: Optional[InstrumentId] = self._instrument_ids.get(symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(symbol), FTX_VENUE)
            self._instrument_ids[symbol] = instrument_id
        return instrument_id

    def _handle_ws_message(self, raw: bytes) -> None:
        if self.reconciliation_active:
            self._log.debug(f"Buffered ws msg {str(raw)}")
            self._ws_buffer.append(raw)
            if self._task_buffer_ws_msgs is None:
                task = self._loop.create_task(self._buffer_ws_msgs())
                self._task_buffer_ws_msgs = task
            return

        msg: Dict[str, Any] = msgspec.json.decode(raw)
        channel: str = msg.get("channel")
        if channel is None:
            self._log.error(str(msg))
            return

        data: Optional[Dict[str, Any]] = msg.get("data")
        if data is None:
            self._log.debug(str(data))  # Normally subscription status
            return

        # TODO(cs): Uncomment for development
        # self._log.info(str(json.dumps(msg, indent=2)), color=LogColor.CYAN)

        # Get instrument
        instrument_id: InstrumentId = self._get_cached_instrument_id(data["market"])
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot handle ws message: no instrument found for {instrument_id}.",
            )
            return

        if channel == "fills":
            self._handle_fill_msg(instrument, data)
        elif channel == "orders":
            self._handle_order_msg(instrument, data)
        else:
            self._log.error(f"Unrecognized websocket message type, was {channel}")

    def _handle_fill_msg(self, instrument: Instrument, data: Dict[str, Any]) -> None:
        if data["type"] != "order":
            self._log.error(f"Fill not for order, {data}")
            return

        # Determine identifiers
        venue_order_id = VenueOrderId(str(data["orderId"]))
        client_order_id_str = data.get("clientOrderId")
        if client_order_id_str:
            client_order_id = ClientOrderId(client_order_id_str)
        else:
            client_order_id = self._order_ids.get(venue_order_id)

        if client_order_id:
            # Handle known order
            self._order_ids[venue_order_id] = client_order_id
            self._handle_fill(
                instrument=instrument,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                data=data,
            )
        else:
            # Handle external or trigger order
            coro = self._delayed_fill(
                instrument=instrument,
                venue_order_id=venue_order_id,
                data=data,
            )
            self._loop.create_task(coro)

    def _handle_fill(
        self,
        instrument: Instrument,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        data: Dict[str, Any],
    ) -> None:
        # Fetch strategy ID
        strategy_id: StrategyId = self._cache.strategy_id_for_order(client_order_id)
        if strategy_id is None:
            self._generate_external_trade_report(instrument, data)
            return

        self.generate_order_filled(
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            venue_position_id=None,  # NETTING accounts
            trade_id=TradeId(str(data["tradeId"])),  # Trade ID
            order_side=OrderSideParser.from_str_py(data["side"].upper()),
            order_type=self._order_types[client_order_id],
            last_qty=Quantity(data["size"], instrument.size_precision),
            last_px=Price(data["price"], instrument.price_precision),
            quote_currency=instrument.quote_currency,
            commission=Money(data["fee"], Currency.from_str(data["feeCurrency"])),
            liquidity_side=LiquiditySide.MAKER
            if data["liquidity"] == "maker"
            else LiquiditySide.TAKER,
            ts_event=pd.to_datetime(data["time"], utc=True).value,
        )
        if not self._calculated_account:
            self._loop.create_task(self._update_account_state())

    async def _delayed_fill(
        self,
        instrument: Instrument,
        venue_order_id: VenueOrderId,
        data: Dict[str, Any],
    ) -> None:
        # The order is likely either a trigger order, or an external order
        client_order_id: Optional[ClientOrderId] = None
        for trigger_id, c_order_id in self._open_triggers.copy().items():
            triggers = await self._http_client.get_trigger_order_triggers(trigger_id)
            for event in triggers:
                if str(event["orderId"]) == venue_order_id.value:  # type: ignore
                    client_order_id = c_order_id
                    order = self._cache.order(client_order_id)
                    if order is None:
                        self._log.error(f"Cannot find order for {repr(client_order_id)}")
                    self.generate_order_updated(
                        strategy_id=self._cache.strategy_id_for_order(client_order_id),
                        instrument_id=instrument.id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        quantity=order.quantity,
                        price=None,
                        trigger_price=None,
                        ts_event=pd.to_datetime(data["time"], utc=True).value,
                        venue_order_id_modified=True,
                    )
                    break

        if client_order_id is None:
            client_order_id = ClientOrderId(str(uuid.uuid4()))

        self._handle_fill(
            instrument=instrument,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            data=data,
        )

    def _handle_order_msg(self, instrument: Instrument, data: Dict[str, Any]) -> None:
        # Determine identifiers
        venue_order_id = VenueOrderId(str(data["id"]))
        client_order_id_str = data.get("clientId")
        if client_order_id_str:
            client_order_id = ClientOrderId(client_order_id_str)
        else:
            client_order_id = self._order_ids.get(venue_order_id)

        if client_order_id:
            # Handle known order
            self._order_ids[venue_order_id] = client_order_id
            self._handle_order_update(
                instrument=instrument,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                data=data,
            )
        else:
            # Handle external or trigger order
            coro = self._delayed_order_update(
                instrument=instrument,
                venue_order_id=venue_order_id,
                data=data,
            )
            self._loop.create_task(coro)

    def _handle_order_update(
        self,
        instrument: Instrument,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        data: Dict[str, Any],
    ) -> None:
        # Fetch strategy ID
        strategy_id: StrategyId = self._cache.strategy_id_for_order(client_order_id)
        if strategy_id is None:
            self._generate_external_order_report(instrument, data)
            return

        ts_event: int = pd.to_datetime(data["createdAt"], utc=True).value

        order_status = data["status"]
        if order_status == "new":
            self.generate_order_accepted(
                strategy_id=strategy_id,
                instrument_id=instrument.id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
        elif order_status == "closed":
            order = self._cache.order(client_order_id)
            if order and order.status != OrderStatus.SUBMITTED:
                self.generate_order_canceled(
                    strategy_id=strategy_id,
                    instrument_id=instrument.id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=ts_event,
                )
            # Clear from hot cache
            self._order_ids.pop(venue_order_id, None)

    async def _delayed_order_update(
        self,
        instrument: Instrument,
        venue_order_id: VenueOrderId,
        data: Dict[str, Any],
    ) -> None:
        # The order is likely either a trigger order, or an external order
        client_order_id: Optional[ClientOrderId] = None
        for trigger_id, c_order_id in self._open_triggers.copy().items():
            triggers = await self._http_client.get_trigger_order_triggers(trigger_id)
            for event in triggers:
                if str(event["orderId"]) == venue_order_id.value:  # type: ignore
                    client_order_id = c_order_id
                    order = self._cache.order(client_order_id)
                    if order is None:
                        self._log.error(f"Cannot find order for {repr(client_order_id)}")
                    self.generate_order_updated(
                        strategy_id=self._cache.strategy_id_for_order(client_order_id),
                        instrument_id=instrument.id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        quantity=order.quantity,
                        price=None,
                        trigger_price=None,
                        ts_event=pd.to_datetime(data["createdAt"], utc=True).value,
                        venue_order_id_modified=True,
                    )
                    break

        if client_order_id is None:
            client_order_id = ClientOrderId(str(uuid.uuid4()))

        self._handle_order_update(
            instrument=instrument,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            data=data,
        )

    def _generate_external_order_report(self, instrument: Instrument, data: Dict[str, Any]) -> None:
        client_id_str = data.get("clientId")
        price = data.get("price")
        created_at = pd.to_datetime(data["createdAt"], utc=True).value
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=InstrumentId(Symbol(data["market"]), FTX_VENUE),
            client_order_id=ClientOrderId(client_id_str) if client_id_str is not None else None,
            venue_order_id=VenueOrderId(str(data["id"])),
            order_side=OrderSide.BUY if data["side"] == "buy" else OrderSide.SELL,
            order_type=parse_order_type(data, price_str="price"),
            time_in_force=TimeInForce.IOC if data["ioc"] else TimeInForce.GTC,
            order_status=OrderStatus.ACCEPTED,
            price=instrument.make_price(price) if price is not None else None,
            quantity=instrument.make_qty(data["size"]),
            filled_qty=instrument.make_qty(0),
            avg_px=None,
            post_only=data["postOnly"],
            reduce_only=data["reduceOnly"],
            report_id=UUID4(),
            ts_accepted=created_at,
            ts_last=created_at,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_status_report(report)

    def _generate_external_trade_report(self, instrument: Instrument, data: Dict[str, Any]) -> None:
        report = parse_trade_report(
            account_id=self.account_id,
            instrument=instrument,
            data=data,
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_trade_report(report)
