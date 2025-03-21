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

from __future__ import annotations

import asyncio
import datetime as dt
from typing import Any

from nautilus_trader.adapters.coinbase_intx.config import CoinbaseIntxExecClientConfig
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX_SUPPORTED_ORDER_TYPES
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX_SUPPORTED_TIF
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX_VENUE
from nautilus_trader.adapters.coinbase_intx.providers import CoinbaseIntxInstrumentProvider
from nautilus_trader.adapters.env import get_env_key
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.functions import order_side_to_pyo3
from nautilus_trader.model.functions import time_in_force_to_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.model.position import Position


class CoinbaseIntxExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Coinbase International crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.CoinbaseIntxHttpClient
        The Coinbase International HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : CoinbaseIntxInstrumentProvider
        The instrument provider.
    config : CoinbaseIntxExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.CoinbaseIntxHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: CoinbaseIntxInstrumentProvider,
        config: CoinbaseIntxExecClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or COINBASE_INTX),
            venue=COINBASE_INTX_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        self._instrument_provider: CoinbaseIntxInstrumentProvider = instrument_provider

        # Configuration
        self._config = config
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.poll_fills_min_interval_ms=}", LogColor.BLUE)

        self._portfolio_id: str = config.portfolio_id or get_env_key("COINBASE_INTX_PORTFOLIO_ID")
        account_id = AccountId(f"{name or COINBASE_INTX}-{self._portfolio_id}")
        self._set_account_id(account_id)
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)
        self._log.info(f"account_id={self.account_id.value}", LogColor.BLUE)
        self._log.info(f"portfolio_id={self._portfolio_id}", LogColor.BLUE)

        # HTTP API
        self._http_client = client
        self._log.info(f"REST API key {self._http_client.api_key}", LogColor.BLUE)

        # WebSocket API
        self._ws_client = nautilus_pyo3.CoinbaseIntxWebSocketClient(
            url=config.base_url_ws,
            api_key=config.api_key,
            api_secret=config.api_secret,
            api_passphrase=config.api_passphrase,
        )
        self._ws_client_futures: set[asyncio.Future] = set()

        # Tasks
        self._poll_fills_task: asyncio.Task | None = None

    @property
    def coinbase_intx_instrument_provider(self) -> CoinbaseIntxInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._cache_instruments()
        await self._update_account_state()

        self._poll_fills_task = self.create_task(
            self._poll_fills(self._config.poll_fills_min_interval_ms),
        )

        future = asyncio.ensure_future(
            self._ws_client.connect(
                instruments=self.coinbase_intx_instrument_provider.instruments_pyo3(),
                callback=self._handle_msg,
            ),
        )
        self._ws_client_futures.add(future)
        self._log.info(f"Connected to {self._ws_client.url}", LogColor.BLUE)
        self._log.info(f"WebSocket API key {self._ws_client.api_key}", LogColor.BLUE)
        self._log.info("Coinbase Intx API key authenticated", LogColor.GREEN)

    async def _disconnect(self) -> None:
        if self._poll_fills_task:
            self._log.debug("Canceling task 'poll_fills'")
            self._poll_fills_task.cancel()
            self._poll_fills_task = None

        # Shutdown websockets
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting websocket")
            await self._ws_client.close()
            self._log.info(f"Disconnected from {self._ws_client.url}", LogColor.BLUE)

        # Cancel all client futures
        for future in self._ws_client_futures:
            if not future.done():
                future.cancel()

    async def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses.
        await self._instrument_provider.initialize()

        instruments_pyo3 = self.coinbase_intx_instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    async def _update_account_state(self) -> None:
        try:
            pyo3_account_state = await self._http_client.request_account_state(self.pyo3_account_id)
        except ValueError as e:
            self._log.error(str(e))
            return

        account_state = AccountState.from_dict(pyo3_account_state.to_dict())

        self.generate_account_state(
            balances=account_state.balances,
            margins=[],  # TBD
            reported=True,
            ts_event=account_state.ts_event,
        )

    # This is a simple and robust work around due no 'user' channel currently available for the
    # Coinbase International websocket API. We continually poll the REST API at the defined minimum
    # poll interval, incrementing the `start` filter param, and only process new fills.
    async def _poll_fills(self, interval_ms: int) -> None:
        try:
            self._log.debug(
                f"Started task 'poll_fills' to request fill reports "
                f"with a minimum interval of {interval_ms}ms",
            )
            last_request_ms = self._clock.timestamp_ms()

            while True:
                # Calculate time since last request
                current_time_ms = self._clock.timestamp_ms()
                elapsed_ms = current_time_ms - last_request_ms

                # Ensure minimum request interval
                if elapsed_ms < interval_ms:
                    remaining_secs = (interval_ms - elapsed_ms) / 1000.0
                    await asyncio.sleep(remaining_secs)

                # Update start time for this request
                start = dt.datetime.fromtimestamp(last_request_ms / 1000.0, tz=dt.UTC)
                last_request_ms = self._clock.timestamp_ms()

                self._log.debug(f"Requesting order fills since {start}")

                pyo3_fill_reports = await self._http_client.request_fill_reports(
                    account_id=self.pyo3_account_id,
                    start=start,
                )

                for pyo3_fill_report in pyo3_fill_reports:
                    fill_report = FillReport.from_pyo3(pyo3_fill_report)
                    if fill_report.client_order_id is None:
                        self._log.warning(f"No ClientOrderId to process fill {fill_report}")
                        continue

                    order = self._cache.order(fill_report.client_order_id)
                    if order is None:
                        self._log.error(
                            f"Cannot process fill - order for {fill_report.client_order_id!r} not found",
                        )
                        continue

                    if fill_report.trade_id in order.trade_ids:
                        self._log.debug(
                            f"Already processed fill for {fill_report}",
                            LogColor.MAGENTA,
                        )
                        continue

                    instrument = self._cache.instrument(order.instrument_id)
                    if instrument is None:
                        raise ValueError(
                            f"Cannot process fill - instrument {order.instrument_id} not found",
                        )

                    self.generate_order_filled(
                        strategy_id=order.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=fill_report.venue_order_id,
                        venue_position_id=fill_report.venue_position_id,
                        trade_id=fill_report.trade_id,
                        order_side=fill_report.order_side,
                        order_type=order.order_type,
                        last_qty=fill_report.last_qty,
                        last_px=fill_report.last_px,
                        quote_currency=instrument.quote_currency,
                        commission=fill_report.commission,
                        liquidity_side=fill_report.liquidity_side,
                        ts_event=fill_report.ts_event,
                    )

                if pyo3_fill_reports:
                    await self._update_account_state()
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'poll_fills'")

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    def _get_cache_active_symbols(self) -> set[nautilus_pyo3.Symbol]:
        # Check cache for all active orders
        open_orders: list[Order] = self._cache.orders_open(venue=self.venue)
        open_positions: list[Position] = self._cache.positions_open(venue=self.venue)
        active_symbols: set[nautilus_pyo3.Symbol] = set()
        for order in open_orders:
            active_symbols.add(nautilus_pyo3.Symbol(order.instrument_id.symbol.value))
        for position in open_positions:
            active_symbols.add(nautilus_pyo3.Symbol(position.instrument_id.symbol.value))
        return active_symbols

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        self._log.debug("Requesting OrderStatusReports...")
        reports: list[OrderStatusReport] = []

        # Check instruments are cached
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        # Fetch active symbols from cached state
        active_symbols = self._get_cache_active_symbols()

        # Fetch active symbols from exchange
        pyo3_position_reports: list[nautilus_pyo3.PositionStatusReport] = (
            await self._http_client.request_position_status_reports(
                account_id=self.pyo3_account_id,
            )
        )

        for pyo3_position_report in pyo3_position_reports:
            if not pyo3_position_report.is_flat:
                active_symbols.add(pyo3_position_report.instrument_id.symbol)

        try:
            for symbol in active_symbols:
                pyo3_order_reports = await self._http_client.request_order_status_reports(
                    account_id=self.pyo3_account_id,
                    symbol=symbol,
                )

                for pyo3_order_report in pyo3_order_reports:
                    report = OrderStatusReport.from_pyo3(pyo3_order_report)
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
                    reports.append(report)
        except Exception as e:
            self._log.error(f"Failed to generate OrderStatusReports: {e}")

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        receipt_log = f"Received {len(reports)} OrderStatusReport{plural}"

        if command.log_receipt_level == LogLevel.INFO:
            self._log.info(receipt_log)
        else:
            self._log.debug(receipt_log)

        return reports

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        PyCondition.is_false(
            command.client_order_id is None and command.venue_order_id is None,
            "both `client_order_id` and `venue_order_id` were `None`",
        )

        # Check instruments are cached
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        self._log.info(
            f"Requesting OrderStatusReport for "
            f"{repr(command.client_order_id) if command.client_order_id else ''} "
            f"{repr(command.venue_order_id) if command.venue_order_id else ''}",
        )

        if command.venue_order_id is None:
            self._log.warning(
                f"Cannot request order status report for {command.client_order_id}, "
                "order has not been assigned a venue order ID",
            )
            return None

        try:
            venue_order_id = nautilus_pyo3.VenueOrderId.from_str(command.venue_order_id.value)
            pyo3_report = await self._http_client.request_order_status_report(
                account_id=self.pyo3_account_id,
                venue_order_id=venue_order_id,
            )

            report = OrderStatusReport.from_pyo3(pyo3_report)
            self._log.debug(f"Received {report}", LogColor.MAGENTA)
            return report
        except Exception as e:
            self._log.error(f"Failed to generate OrderStatusReport: {e}")
        return None

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        # Check instruments cache first
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        self._log.debug("Requesting FillReports...")
        reports: list[FillReport] = []

        try:
            pyo3_reports = await self._http_client.request_fill_reports(
                account_id=self.pyo3_account_id,
                start=command.start,
            )

            for pyo3_report in pyo3_reports:
                report = FillReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except Exception as e:
            self._log.error(f"Failed to generate FillReports: {e}")

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} FillReport{plural}")

        return reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        # Check instruments are cached
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        reports: list[PositionStatusReport] = []

        try:
            if command.instrument_id:
                instrument_id = command.instrument_id
                self._log.debug(f"Requesting PositionStatusReport for {instrument_id}")
                pyo3_report = await self._http_client.request_position_status_report(
                    account_id=self.pyo3_account_id,
                    symbol=nautilus_pyo3.Symbol.from_str(instrument_id.symbol.value),
                )

                report = PositionStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
            else:
                self._log.debug("Requesting PositionStatusReports...")
                pyo3_reports: list[nautilus_pyo3.PositionStatusReport] = (
                    await self._http_client.request_position_status_reports(
                        account_id=self.pyo3_account_id,
                    )
                )

                for pyo3_report in pyo3_reports:
                    report = PositionStatusReport.from_pyo3(pyo3_report)
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
                    reports.append(report)

                open_symbols = {i.symbol.value for i in self._cache.positions_open(self.venue)}
                reported_symbols = {r.instrument_id.symbol.value for r in pyo3_reports}
                remaining_symbols = open_symbols.difference(reported_symbols)

                for symbol in remaining_symbols:
                    pyo3_report = await self._http_client.request_position_status_report(
                        account_id=self.pyo3_account_id,
                        symbol=nautilus_pyo3.Symbol.from_str(symbol),
                    )

                    report = PositionStatusReport.from_pyo3(pyo3_report)
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
                    reports.append(report)
        except Exception as e:
            self._log.error(f"Failed to generate PositionReports: {e}")

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} PositionReport{plural}")

        return reports

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

        try:
            report = await self._http_client.cancel_order(
                account_id=self.pyo3_account_id,
                client_order_id=nautilus_pyo3.ClientOrderId(command.client_order_id.value),
            )
        except Exception as e:
            self.generate_order_cancel_rejected(
                order.strategy_id,
                order.instrument_id,
                order.client_order_id,
                order.venue_order_id,
                str(e),
                self._clock.timestamp_ns(),
            )
            return

        self.generate_order_canceled(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_event=report.ts_last,
        )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        pyo3_order_side: nautilus_pyo3.OrderSide | None = None
        if command.order_side == OrderSide.BUY:
            pyo3_order_side = nautilus_pyo3.OrderSide.BUY
        elif command.order_side == OrderSide.SELL:
            pyo3_order_side = nautilus_pyo3.OrderSide.SELL

        instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            raise ValueError(f"Instrument {command.instrument_id} not found")

        try:
            reports = await self._http_client.cancel_orders(
                account_id=self.pyo3_account_id,
                symbol=nautilus_pyo3.Symbol(command.instrument_id.symbol.value),
                order_side=pyo3_order_side,
            )
        except Exception as e:
            orders_open: list[Order] = self._cache.orders_open(instrument_id=command.instrument_id)
            for open_order in orders_open:
                if open_order.is_closed:
                    continue
                self.generate_order_cancel_rejected(
                    open_order.strategy_id,
                    open_order.instrument_id,
                    open_order.client_order_id,
                    open_order.venue_order_id,
                    str(e),
                    self._clock.timestamp_ns(),
                )
            return

        for report in reports:
            if report.client_order_id is None:
                self._log.error("Cannot process cancel: no ClientOrderId for order")
                continue

            order: Order | None = self._cache.order(ClientOrderId(report.client_order_id.value))
            if order is None:
                self._log.error(
                    f"Cannot process cancel: {command.client_order_id!r} not found in cache",
                )
                continue

            self.generate_order_canceled(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                ts_event=report.ts_last,
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

        self._log.error(
            "Cannot modify order (not yet implemented, use cancel then replace instead)",
        )
        # TODO: Modifying orders on Coinbase International under development.
        # Requires a new client order ID (this suggests they cancel and replace on their end)

        # try:
        #     report = await self._http_client.modify_order(
        #         account_id=self.pyo3_account_id,
        #         client_order_id=nautilus_pyo3.ClientOrderId(command.client_order_id.value),
        #         new_client_order_id=nautilus_pyo3.ClientOrderId(new_client_order_id_value),
        #         price=nautilus_pyo3.Price.from_str(str(command.price)) if command.price else None,
        #         trigger_price=(
        #             nautilus_pyo3.Price.from_str(str(command.trigger_price))
        #             if command.trigger_price
        #             else None
        #         ),
        #         quantity=(
        #             nautilus_pyo3.Quantity.from_str(str(command.quantity))
        #             if command.trigger_price
        #             else None
        #         ),
        #     )
        # except Exception as e:
        #     self.generate_order_modify_rejected(
        #         order.strategy_id,
        #         order.instrument_id,
        #         order.client_order_id,
        #         order.venue_order_id,
        #         str(e),
        #         self._clock.timestamp_ns(),
        #     )
        #     return
        #
        # self.generate_order_updated(
        #     strategy_id=order.strategy_id,
        #     instrument_id=order.instrument_id,
        #     client_order_id=order.client_order_id,
        #     venue_order_id=order.venue_order_id,
        #     quantity=Quantity.from_str(str(report.quantity)),
        #     price=Price.from_str(str(report.price)),
        #     trigger_price=(
        #         Price.from_str(str(report.trigger_price)) if report.trigger_price else None
        #     ),
        #     ts_event=report.ts_last,
        # )

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.order_type not in COINBASE_INTX_SUPPORTED_ORDER_TYPES:
            self._log.error(
                f"Coinbase International does not support {order.order_type_string()} order types",
            )
            return

        if order.time_in_force not in COINBASE_INTX_SUPPORTED_TIF:
            self._log.error(
                f"Coinbase International does not support {order.tif_string()} time in force",
            )
            return

        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order, {order}")
            return

        # Generate order submitted event, to ensure correct ordering of event
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        try:
            if order.order_type == OrderType.MARKET:
                report = await self._submit_market_order(order)
                return
            elif order.order_type == OrderType.LIMIT:
                report = await self._submit_limit_order(order)
            elif order.order_type == OrderType.STOP_MARKET:
                report = await self._submit_stop_market_order(order)
            elif order.order_type == OrderType.STOP_LIMIT:
                report = await self._submit_stop_limit_order(order)
            else:
                self._log.error(f"Submitting {order.type_string()} orders not currently supported")
                return

            self.generate_order_accepted(
                instrument_id=order.instrument_id,
                strategy_id=order.strategy_id,
                client_order_id=order.client_order_id,
                venue_order_id=VenueOrderId(report.venue_order_id.value),
                ts_event=self._clock.timestamp_ns(),
            )
        except Exception as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _submit_market_order(
        self,
        order: MarketOrder,
    ) -> nautilus_pyo3.OrderStatusReport:
        if order.time_in_force not in (TimeInForce.IOC, TimeInForce.FOK):
            self._log.warning(
                f"Submitting MARKET order as IOC, was {order.tif_string()} "
                "(market orders on Coinbase International must have a time in force of IOC or FOK)",
            )
            time_in_force = TimeInForce.IOC
        else:
            time_in_force = order.time_in_force
        return await self._http_client.submit_order(
            account_id=self.pyo3_account_id,
            client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
            symbol=nautilus_pyo3.Symbol(order.instrument_id.symbol.value),
            order_side=order_side_to_pyo3(order.side),
            order_type=nautilus_pyo3.OrderType.MARKET,
            time_in_force=time_in_force_to_pyo3(time_in_force),
            quantity=nautilus_pyo3.Quantity.from_str(str(order.quantity)),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
        )

    async def _submit_limit_order(
        self,
        order: LimitOrder,
    ) -> nautilus_pyo3.OrderStatusReport:
        expire_time = (
            order.expire_time.tz_convert("UTC").to_pydatetime() if order.expire_time else None
        )

        return await self._http_client.submit_order(
            account_id=self.pyo3_account_id,
            client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
            symbol=nautilus_pyo3.Symbol(order.instrument_id.symbol.value),
            order_side=order_side_to_pyo3(order.side),
            order_type=nautilus_pyo3.OrderType.LIMIT,
            time_in_force=time_in_force_to_pyo3(order.time_in_force),
            expire_time=expire_time,
            quantity=nautilus_pyo3.Quantity.from_str(str(order.quantity)),
            price=nautilus_pyo3.Price.from_str(str(order.price)),
            post_only=order.is_post_only if order.is_post_only else None,
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
        )

    async def _submit_stop_market_order(
        self,
        order: StopMarketOrder,
    ) -> nautilus_pyo3.OrderStatusReport:
        expire_time = (
            order.expire_time.tz_convert("UTC").to_pydatetime() if order.expire_time else None
        )

        return await self._http_client.submit_order(
            account_id=self.pyo3_account_id,
            client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
            symbol=nautilus_pyo3.Symbol(order.instrument_id.symbol.value),
            order_side=order_side_to_pyo3(order.side),
            order_type=nautilus_pyo3.OrderType.STOP_MARKET,
            time_in_force=time_in_force_to_pyo3(order.time_in_force),
            expire_time=expire_time,
            quantity=nautilus_pyo3.Quantity.from_str(str(order.quantity)),
            trigger_price=nautilus_pyo3.Price.from_str(str(order.trigger_price)),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
        )

    async def _submit_stop_limit_order(
        self,
        order: StopMarketOrder,
    ) -> nautilus_pyo3.OrderStatusReport:
        expire_time = (
            order.expire_time.tz_convert("UTC").to_pydatetime() if order.expire_time else None
        )

        return await self._http_client.submit_order(
            account_id=self.pyo3_account_id,
            client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
            symbol=nautilus_pyo3.Symbol(order.instrument_id.symbol.value),
            order_side=order_side_to_pyo3(order.side),
            order_type=nautilus_pyo3.OrderType.STOP_LIMIT,
            time_in_force=time_in_force_to_pyo3(order.time_in_force),
            expire_time=expire_time,
            quantity=nautilus_pyo3.Quantity.from_str(str(order.quantity)),
            price=nautilus_pyo3.Price.from_str(str(order.price)),
            trigger_price=nautilus_pyo3.Price.from_str(str(order.trigger_price)),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
        )

    def _handle_msg(self, msg: Any) -> None:
        self._log.warning(f"Received {msg}")
