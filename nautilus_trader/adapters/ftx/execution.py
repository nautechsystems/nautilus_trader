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

import orjson
import pandas as pd

from nautilus_trader.accounting.accounts.margin import MarginAccount
from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.adapters.ftx.common import FTX_VENUE
from nautilus_trader.adapters.ftx.http.client import FTXHttpClient
from nautilus_trader.adapters.ftx.http.error import FTXError
from nautilus_trader.adapters.ftx.parsing import parse_order_status
from nautilus_trader.adapters.ftx.parsing import parse_order_type
from nautilus_trader.adapters.ftx.parsing import parse_trigger_order_status
from nautilus_trader.adapters.ftx.providers import FTXInstrumentProvider
from nautilus_trader.adapters.ftx.websocket.client import FTXWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.c_enums.account_type import AccountType
from nautilus_trader.model.c_enums.order_side import OrderSideParser
from nautilus_trader.model.commands.trading import CancelAllOrders
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import ModifyOrder
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.commands.trading import SubmitOrderList
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
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
from nautilus_trader.model.orders.trailing_stop_limit import TrailingStopLimitOrder
from nautilus_trader.model.orders.trailing_stop_market import TrailingStopMarketOrder
from nautilus_trader.model.position import Position
from nautilus_trader.msgbus.bus import MessageBus


class FTXExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Binance SPOT markets.

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
        account_polling_interval: int = 60,
        calculated_account: bool = False,
    ):
        super().__init__(
            loop=loop,
            client_id=ClientId(FTX_VENUE.value),
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
            handler=self._handle_ws_message,
            key=client.api_key,
            secret=client.api_secret,
            us=us,
        )

        # Tasks
        self._task_poll_account: Optional[asyncio.Task] = None

        # Hot caches
        self._instrument_ids: Dict[str, InstrumentId] = {}
        self._order_ids: Dict[VenueOrderId, ClientOrderId] = {}
        self._order_types: Dict[VenueOrderId, OrderType] = {}

        # Settings
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

    def connect(self):
        """
        Connect the client to FTX.
        """
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    def disconnect(self):
        """
        Disconnect the client from FTX.
        """
        self._log.info("Disconnecting...")
        self._loop.create_task(self._disconnect())

    async def _connect(self):
        # Connect HTTP client
        if not self._http_client.connected:
            await self._http_client.connect()
        try:
            await self._instrument_provider.load_all_or_wait_async()
        except FTXError as ex:
            self._log.exception(ex)
            return

        self._log.info("FTX API key authenticated.", LogColor.GREEN)
        self._log.info(f"API key {self._http_client.api_key}.")

        # Update account state
        await self._update_account_state()
        self._task_poll_account = self._loop.create_task(self._poll_account_state())

        # Connect WebSocket client
        await self._ws_client.connect(start=True)
        await self._ws_client.subscribe_fills()
        await self._ws_client.subscribe_orders()

        # reports = await self.generate_order_status_reports()  # TODO!
        # self._log.info(str(reports), LogColor.GREEN)

        self._set_connected(True)
        self._log.info("Connected.")

    async def _disconnect(self):
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

    # -- STATUS REPORTS ----------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        client_order_id: ClientOrderId = None,
        venue_order_id: VenueOrderId = None,
    ) -> Optional[OrderStatusReport]:
        """
        Generate an order status report for the given order identifier parameter(s).

        Either one or both of the identifiers must be provided.

        If the order is not found, or an error occurs, then logs and returns
        ``None``.

        Parameters
        ----------
        client_order_id : ClientOrderId, optional
            The client order ID query filter.
        venue_order_id : VenueOrderId, optional
            The venue order ID (assigned by the venue) query filter.

        Returns
        -------
        OrderStatusReport or ``None``

        """
        if client_order_id is None and venue_order_id is None:
            self._log.error("Cannot generate order status report: no identifier given.")
            return None

        try:
            if venue_order_id is not None:
                response = await self._http_client.get_order_status(venue_order_id.value)
            else:
                response = await self._http_client.get_order_status_by_client_id(
                    client_order_id.value
                )
        except FTXError as ex:
            self._log.error(ex.message)  # type: ignore  # TODO(cs): Improve errors
            return None

        # Get instrument
        instrument_id: InstrumentId = self._get_cached_instrument_id(response)
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot generate order status report: "
                f"no instrument found for {instrument_id}.",
            )
            return None

        return parse_order_status(
            account_id=self.account_id,
            instrument=instrument,
            data=response,
            report_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
        open_only: bool = False,
    ) -> List[OrderStatusReport]:
        """
        Generate a list of order status reports with optional query filters.

        The returned list may be empty if no orders match the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        start : datetime, optional
            The start datetime query filter.
        end : datetime, optional
            The end datetime query filter.
        open_only : bool, default False
            If the query is for open orders only.

        Returns
        -------
        list[OrderStatusReport]

        """
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
                    start_time=int(start.timestamp() * 1000) if start is not None else None,
                    end_time=int(end.timestamp() * 1000) if end is not None else None,
                )
        except FTXError as ex:
            self._log.error(ex.message)  # type: ignore  # TODO(cs): Improve errors
            return []

        if response:
            for data in response:
                # Get instrument
                instrument_id = instrument_id or self._get_cached_instrument_id(data)
                instrument = self._instrument_provider.find(instrument_id)
                if instrument is None:
                    self._log.error(
                        f"Cannot generate order status report: "
                        f"no instrument found for {instrument_id}.",
                    )
                    continue

                reports.append(
                    parse_order_status(
                        account_id=self.account_id,
                        instrument=instrument,
                        data=data,
                        report_id=self._uuid_factory.generate(),
                        ts_init=self._clock.timestamp_ns(),
                    )
                )

        return reports

    async def _get_trigger_order_status_reports(
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
                    start_time=int(start.timestamp() * 1000) if start is not None else None,
                    end_time=int(end.timestamp() * 1000) if end is not None else None,
                )
        except FTXError as ex:
            self._log.error(ex.message)  # type: ignore  # TODO(cs): Improve errors
            return []

        if response:
            for data in response:
                # Get instrument
                instrument_id = instrument_id or self._get_cached_instrument_id(data)
                instrument = self._instrument_provider.find(instrument_id)
                if instrument is None:
                    self._log.error(
                        f"Cannot generate order status report: "
                        f"no instrument found for {instrument_id}.",
                    )
                    continue

                reports.append(
                    parse_trigger_order_status(
                        account_id=self.account_id,
                        instrument=instrument,
                        data=data,
                        report_id=self._uuid_factory.generate(),
                        ts_init=self._clock.timestamp_ns(),
                    )
                )

        return reports

    async def generate_trade_reports(
        self,
        instrument_id: InstrumentId = None,
        venue_order_id: VenueOrderId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> List[TradeReport]:
        """
        Generate a list of trade reports with optional query filters.

        The returned list may be empty if no trades match the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        venue_order_id : VenueOrderId, optional
            The venue order ID (assigned by the venue) query filter.
        start : datetime, optional
            The start datetime query filter.
        end : datetime, optional
            The end datetime query filter.

        Returns
        -------
        list[TradeReport]

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> List[PositionStatusReport]:
        """
        Generate a list of position status reports with optional query filters.

        The returned list may be empty if no positions match the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        start : datetime, optional
            The start datetime query filter.
        end : datetime, optional
            The end datetime query filter.

        Returns
        -------
        list[PositionStatusReport]

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- COMMAND HANDLERS --------------------------------------------------------------------------

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

        try:
            if order.type == OrderType.MARKET:
                await self._submit_market_order(order)
            elif order.type == OrderType.LIMIT:
                await self._submit_limit_order(order)
            elif order.type == OrderType.STOP_MARKET:
                await self._submit_stop_market_order(order, position)
            elif order.type == OrderType.STOP_LIMIT:
                await self._submit_stop_limit_order(order, position)
            elif order.type == OrderType.TRAILING_STOP_MARKET:
                await self._submit_trailing_stop_market(order)
            elif order.type == OrderType.TRAILING_STOP_LIMIT:
                await self._submit_trailing_stop_limit(order)
        except FTXError as ex:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=ex.message,  # type: ignore  # TODO(cs): Improve errors
                ts_event=self._clock.timestamp_ns(),  # TODO(cs): Parse from response
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
                order_type = "take_profit"
            elif order.is_sell and order.trigger_price > position.avg_px_open:
                order_type = "take_profit"
        await self._http_client.place_trigger_order(
            market=order.instrument_id.symbol.value,
            side=OrderSideParser.to_str_py(order.side).lower(),
            size=str(order.quantity),
            order_type=order_type,
            client_id=order.client_order_id.value,
            trigger_price=str(order.trigger_price),
            reduce_only=order.is_reduce_only,
        )

    async def _submit_stop_limit_order(
        self,
        order: StopLimitOrder,
        position: Optional[Position],
    ) -> None:
        order_type = "stop"
        if position is not None:
            if order.is_buy and order.trigger_price < position.avg_px_open:
                order_type = "take_profit"
            elif order.is_sell and order.trigger_price > position.avg_px_open:
                order_type = "take_profit"
        await self._http_client.place_trigger_order(
            market=order.instrument_id.symbol.value,
            side=OrderSideParser.to_str_py(order.side).lower(),
            size=str(order.quantity),
            order_type=order_type,
            client_id=order.client_order_id.value,
            price=str(order.price),
            trigger_price=str(order.trigger_price),
            reduce_only=order.is_reduce_only,
        )

    async def _submit_trailing_stop_market(self, order: TrailingStopMarketOrder) -> None:
        await self._http_client.place_trigger_order(
            market=order.instrument_id.symbol.value,
            side=OrderSideParser.to_str_py(order.side).lower(),
            size=str(order.quantity),
            order_type="trailing_stop",
            client_id=order.client_order_id.value,
            trigger_price=str(order.trigger_price),
            trail_value=str(order.trailing_offset) if order.is_buy else str(-order.trailing_offset),
            reduce_only=order.is_reduce_only,
        )

    async def _submit_trailing_stop_limit(self, order: TrailingStopLimitOrder) -> None:
        await self._http_client.place_trigger_order(
            market=order.instrument_id.symbol.value,
            side=OrderSideParser.to_str_py(order.side).lower(),
            size=str(order.quantity),
            order_type="trailing_stop",
            client_id=order.client_order_id.value,
            price=str(order.price),
            trigger_price=str(order.trigger_price),
            trail_value=str(order.trailing_offset) if order.is_buy else str(-order.trailing_offset),
            reduce_only=order.is_reduce_only,
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
        except FTXError as ex:
            self._log.error(ex.message)  # type: ignore  # TODO(cs): Improve errors

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
            await self._http_client.cancel_order(command.client_order_id.value)
        except FTXError as ex:
            self._log.error(ex.message)  # type: ignore  # TODO(cs): Improve errors

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

        # Cancel all working orders
        working_orders = self._cache.orders_working(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )
        for order in working_orders:
            self.generate_order_pending_cancel(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                ts_event=self._clock.timestamp_ns(),
            )
        try:
            await self._http_client.cancel_all_orders(command.instrument_id.symbol.value)
        except FTXError as ex:
            self._log.error(ex.message)  # type: ignore  # TODO(cs): Improve errors

    async def _poll_account_state(self) -> None:
        while True:
            await asyncio.sleep(self._account_polling_interval)
            await self._update_account_state()

    async def _update_account_state(self) -> None:
        self._log.debug("Updating account state...")

        response: Dict[str, Any] = await self._http_client.get_account_info()
        if self.account_id is None:
            self._set_account_id(AccountId(FTX_VENUE.value, str(response["accountIdentifier"])))

        self._handle_account_info(response)

        if not self._initial_leverage_set:
            account: Optional[MarginAccount] = self._cache.account(self.account_id)
            while account is None:
                self._log.debug(f"Waiting for account {self.account_id}...")
                await asyncio.sleep(0.1)
            leverage = Decimal(response["leverage"])
            account.set_default_leverage(leverage)
            self._log.info(
                f"{self.account_id} leverage {leverage}X.",
                LogColor.BLUE,
            )
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

    def _get_cached_instrument_id(self, data: Dict[str, Any]) -> InstrumentId:
        # Parse instrument ID
        symbol: str = data["market"]
        instrument_id: Optional[InstrumentId] = self._instrument_ids.get(symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(symbol), FTX_VENUE)
            self._instrument_ids[symbol] = instrument_id
        return instrument_id

    def _handle_ws_message(self, raw: bytes):
        msg: Dict[str, Any] = orjson.loads(raw)
        channel: str = msg.get("channel")
        if channel is None:
            self._log.error(str(msg))
            return

        data: Optional[Dict[str, Any]] = msg.get("data")
        if data is None:
            self._log.debug(str(data))  # Normally subscription status
            return

        # TODO(cs): Uncomment for development
        # self._log.info(str(json.dumps(msg, indent=4)), color=LogColor.GREEN)

        # Get instrument
        instrument_id: InstrumentId = self._get_cached_instrument_id(data)
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot handle ws message: no instrument found for {instrument_id}.",
            )
            return

        if channel == "fills":
            self._handle_fills(instrument, data)
        elif channel == "orders":
            self._handle_orders(instrument, data)
        else:
            self._log.error(f"Unrecognized websocket message type, was {channel}")
            return

    def _handle_fills(self, instrument: Instrument, data: Dict[str, Any]) -> None:
        if data["type"] != "order":
            self._log.error(f"Fill not for order, {data}")
            return

        # Parse identifiers
        venue_order_id = VenueOrderId(str(data["orderId"]))
        client_order_id = self._order_ids.get(venue_order_id)
        if client_order_id is None:
            client_order_id = ClientOrderId(str(uuid.uuid4()))

        # Fetch strategy ID
        strategy_id: StrategyId = self._cache.strategy_id_for_order(client_order_id)
        if strategy_id is None:
            # TODO(cs): Implement external order handling
            self._log.error(
                f"Cannot handle fill: strategy ID for {client_order_id} not found.",
            )
            return

        self.generate_order_filled(
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            venue_position_id=None,  # NETTING accounts
            trade_id=TradeId(str(data["id"])),  # Trade ID
            order_side=OrderSideParser.from_str_py(data["side"].upper()),
            order_type=self._order_types[venue_order_id],
            last_qty=Quantity(data["size"], instrument.size_precision),
            last_px=Price(data["price"], instrument.price_precision),
            quote_currency=instrument.quote_currency,
            commission=Money(data["fee"], Currency.from_str(data["feeCurrency"])),
            liquidity_side=LiquiditySide.MAKER
            if data["liquidity"] == "maker"
            else LiquiditySide.TAKER,
            ts_event=pd.to_datetime(data["time"], utc=True).to_datetime64(),
        )
        if not self._calculated_account:
            self._loop.create_task(self._update_account_state())

    def _handle_orders(self, instrument: Instrument, data: Dict[str, Any]) -> None:
        # Parse client order ID
        client_order_id_str = data.get("clientId")
        if not client_order_id_str:
            client_order_id_str = str(uuid.uuid4())
        client_order_id = ClientOrderId(client_order_id_str)
        venue_order_id = VenueOrderId(str(data["id"]))

        # Hot Cache
        self._order_ids[venue_order_id] = client_order_id
        self._order_types[venue_order_id] = parse_order_type(data)

        # Fetch strategy ID
        strategy_id: StrategyId = self._cache.strategy_id_for_order(client_order_id)
        if strategy_id is None:
            # TODO(cs): Implement external order handling
            self._log.error(
                f"Cannot handle order update: strategy ID for {client_order_id} not found.",
            )
            return

        ts_event: int = int(pd.to_datetime(data["createdAt"], utc=True).to_datetime64())

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
