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
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
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

        self._portfolio_id: str = config.portfolio_id or get_env_key("COINBASE_INTX_PORTFOLIO_ID")
        account_id = AccountId(f"{name or COINBASE_INTX}-{self._portfolio_id}")
        self._set_account_id(account_id)
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)
        self._log.info(f"account_id={self.account_id.value}", LogColor.BLUE)
        self._log.info(f"portfolio_id={self._portfolio_id}", LogColor.BLUE)

        # HTTP API
        self._http_client = client
        self._log.info(f"REST API key {self._http_client.api_key}", LogColor.BLUE)

        # FIX API
        self._fix_client = nautilus_pyo3.CoinbaseIntxFixClient(
            endpoint=None,  # Uses production endpoint by default
            api_key=config.api_key,
            api_secret=config.api_secret,
            api_passphrase=config.api_passphrase,
            portfolio_id=self._portfolio_id,
        )
        self._fix_client_futures: set[asyncio.Future] = set()

    @property
    def coinbase_intx_instrument_provider(self) -> CoinbaseIntxInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._cache_instruments()
        await self._update_account_state()
        await self._await_account_registered()

        self._log.info("Coinbase INTX API key authenticated", LogColor.GREEN)

        self._log.info(
            f"Logging on to FIX Drop Copy server: endpoint={self._fix_client.endpoint}, "
            f"target_comp_id={self._fix_client.target_comp_id}, "
            f"sender_comp_id={self._fix_client.sender_comp_id}",
            LogColor.BLUE,
        )
        await self._fix_client.connect(
            handler=self._handle_msg,
        )

        try:
            # Wait for connection to be established
            await asyncio.wait_for(self._wait_for_logon(), 30.0)
        except TimeoutError:
            self._log.error("Timed out logging on to FIX Drop Copy server")
            await self._fix_client.close()
            raise

        self._log.info("Logon successful", LogColor.GREEN)

    async def _wait_for_logon(self):
        while not self._fix_client.is_logged_on():
            await asyncio.sleep(0.01)

    async def _disconnect(self) -> None:
        # Shutdown FIX client
        if not self._fix_client.is_logged_on():
            self._log.info("Disconnecting FIX client")
            await self._fix_client.close()
            self._log.info("Disconnected from FIX Drop Copy server", LogColor.BLUE)

        # Cancel any pending futures
        await cancel_tasks_with_timeout(
            self._fix_client_futures,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )
        self._fix_client_futures.clear()

    async def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses.
        await self._instrument_provider.initialize()

        instruments_pyo3 = self.coinbase_intx_instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    async def _update_account_state(self) -> None:
        pyo3_account_state = await self._http_client.request_account_state(self.pyo3_account_id)
        account_state = AccountState.from_dict(pyo3_account_state.to_dict())

        self.generate_account_state(
            balances=account_state.balances,
            margins=[],  # TBD
            reported=True,
            ts_event=account_state.ts_event,
        )

        if account_state.balances:
            self._log.info(
                f"Generated account state with {len(account_state.balances)} balance(s)",
            )

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
            self._log.exception("Failed to generate OrderStatusReports", e)

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
            self._log.exception("Failed to generate OrderStatusReport", e)
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
            self._log.exception("Failed to generate FillReports", e)

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
            self._log.exception("Failed to generate PositionReports", e)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} PositionReport{plural}")

        return reports

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _query_account(self, _command: QueryAccount) -> None:
        # Specific account ID (sub account) not yet supported
        await self._update_account_state()

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
            await self._http_client.cancel_order(
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
            await self._http_client.cancel_orders(
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
            "Cannot modify order: modifying orders on Coinbase International requires a new client order ID, "
            "this could be handled but doesn't map well to the Nautilus domain model so for now we use a "
            "cancel and replace approach",
        )

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

        if order.is_quote_quantity:
            reason = "UNSUPPORTED_QUOTE_QUANTITY"
            self._log.error(
                f"Cannot submit order {order.client_order_id}: {reason}",
            )
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=reason,
                ts_event=self._clock.timestamp_ns(),
            )
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
                return  # Do not generate accepted event
            elif order.order_type == OrderType.LIMIT:
                report = await self._submit_limit_order(order)
            elif order.order_type == OrderType.STOP_MARKET:
                report = await self._submit_stop_market_order(order)
            elif order.order_type == OrderType.STOP_LIMIT:
                report = await self._submit_stop_limit_order(order)
            else:
                self._log.error(f"Submitting {order.type_string()} orders not currently supported")
                return  # Do not generate accepted event

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
        return await self._http_client.submit_order(
            account_id=self.pyo3_account_id,
            client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
            symbol=nautilus_pyo3.Symbol(order.instrument_id.symbol.value),
            order_side=order_side_to_pyo3(order.side),
            order_type=nautilus_pyo3.OrderType.LIMIT,
            time_in_force=time_in_force_to_pyo3(order.time_in_force),
            expire_time=ensure_pydatetime_utc(order.expire_time),
            quantity=nautilus_pyo3.Quantity.from_str(str(order.quantity)),
            price=nautilus_pyo3.Price.from_str(str(order.price)),
            post_only=order.is_post_only if order.is_post_only else None,
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
        )

    async def _submit_stop_market_order(
        self,
        order: StopMarketOrder,
    ) -> nautilus_pyo3.OrderStatusReport:
        return await self._http_client.submit_order(
            account_id=self.pyo3_account_id,
            client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
            symbol=nautilus_pyo3.Symbol(order.instrument_id.symbol.value),
            order_side=order_side_to_pyo3(order.side),
            order_type=nautilus_pyo3.OrderType.STOP_MARKET,
            time_in_force=time_in_force_to_pyo3(order.time_in_force),
            expire_time=ensure_pydatetime_utc(order.expire_time),
            quantity=nautilus_pyo3.Quantity.from_str(str(order.quantity)),
            trigger_price=nautilus_pyo3.Price.from_str(str(order.trigger_price)),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
        )

    async def _submit_stop_limit_order(
        self,
        order: StopMarketOrder,
    ) -> nautilus_pyo3.OrderStatusReport:
        return await self._http_client.submit_order(
            account_id=self.pyo3_account_id,
            client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
            symbol=nautilus_pyo3.Symbol(order.instrument_id.symbol.value),
            order_side=order_side_to_pyo3(order.side),
            order_type=nautilus_pyo3.OrderType.STOP_LIMIT,
            time_in_force=time_in_force_to_pyo3(order.time_in_force),
            expire_time=ensure_pydatetime_utc(order.expire_time),
            quantity=nautilus_pyo3.Quantity.from_str(str(order.quantity)),
            price=nautilus_pyo3.Price.from_str(str(order.price)),
            trigger_price=nautilus_pyo3.Price.from_str(str(order.trigger_price)),
            reduce_only=order.is_reduce_only if order.is_reduce_only else None,
        )

    def _is_external_order(self, client_order_id: ClientOrderId) -> bool:
        return not client_order_id or not self._cache.strategy_id_for_order(client_order_id)

    def _handle_msg(self, msg: Any) -> None:  # noqa: C901 (too complex)
        # Note: These FIX execution reports are using a default precision of 8 for now,
        # this avoids the need to track a separate cache down in Rust. Ensure all price
        # and quantity values are reinitialized using the instruments `make_price` and
        # `make_qty` helper methods.

        if isinstance(msg, nautilus_pyo3.OrderStatusReport):
            report = OrderStatusReport.from_pyo3(msg)

            if self._is_external_order(report.client_order_id):
                self._send_order_status_report(report)
                return

            order = self._cache.order(report.client_order_id)
            if order is None:
                self._log.error(
                    f"Cannot process execution report - order for {report.client_order_id!r} not found",
                )
                return

            instrument = self._cache.instrument(order.instrument_id)
            if instrument is None:
                raise ValueError(
                    f"Cannot process execution report - instrument {order.instrument_id} not found",
                )

            if report.order_status == OrderStatus.CANCELED:
                self.generate_order_canceled(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    ts_event=report.ts_last,
                )
            elif report.order_status == OrderStatus.EXPIRED:
                self.generate_order_expired(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    ts_event=report.ts_last,
                )
            elif report.order_status == OrderStatus.TRIGGERED:
                self.generate_order_triggered(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    ts_event=report.ts_last,
                )
            elif order.status == OrderStatus.PENDING_UPDATE:
                self.generate_order_updated(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    quantity=instrument.make_qty(report.quantity),
                    price=instrument.make_price(report.price) if report.price else None,
                    trigger_price=(
                        instrument.make_price(report.trigger_price)
                        if report.trigger_price
                        else None
                    ),
                    ts_event=report.ts_last,
                )
            else:
                self._log.warning(f"Received unhandled execution report {report}")
        elif isinstance(msg, nautilus_pyo3.FillReport):
            report = FillReport.from_pyo3(msg)

            if self._is_external_order(report.client_order_id):
                self._send_order_status_report(report)
                return

            order = self._cache.order(report.client_order_id)
            if order is None:
                self._log.error(
                    f"Cannot process execution report - order for {report.client_order_id!r} not found",
                )
                return

            instrument = self._cache.instrument(order.instrument_id)
            if instrument is None:
                raise ValueError(
                    f"Cannot process execution report - instrument {order.instrument_id} not found",
                )

            self.generate_order_filled(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=report.venue_order_id,
                venue_position_id=report.venue_position_id,
                trade_id=report.trade_id,
                order_side=report.order_side,
                order_type=order.order_type,
                last_qty=instrument.make_qty(report.last_qty),
                last_px=instrument.make_price(report.last_px),
                quote_currency=instrument.quote_currency,
                commission=report.commission,
                liquidity_side=report.liquidity_side,
                ts_event=report.ts_event,
            )
