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

import asyncio
from typing import Any

from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.core.nautilus_pyo3 import OKXTradeMode
from nautilus_trader.core.uuid import UUID4
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
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.functions import order_side_to_pyo3
from nautilus_trader.model.functions import order_type_to_pyo3
from nautilus_trader.model.functions import time_in_force_to_pyo3
from nautilus_trader.model.functions import trigger_type_to_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orders import Order


class OKXExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the OKX centralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.OKXHttpClient
        The OKX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : OKXInstrumentProvider
        The instrument provider.
    config : OKXExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.OKXHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: OKXInstrumentProvider,
        config: OKXExecClientConfig,
        name: str | None,
    ) -> None:
        PyCondition.not_empty(config.instrument_types, "config.instrument_types")

        # Determine account type based on instrument types
        if instrument_provider.instrument_types == (OKXInstrumentType.SPOT,):
            account_type = AccountType.CASH
        else:
            account_type = AccountType.MARGIN

        super().__init__(
            loop=loop,
            client_id=ClientId(name or OKX_VENUE.value),
            venue=OKX_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=account_type,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._instrument_provider: OKXInstrumentProvider = instrument_provider

        instrument_types = [i.name.upper() for i in config.instrument_types]
        contract_types = (
            [c.name.upper() for c in config.contract_types] if config.contract_types else None
        )

        # Configuration
        self._config = config
        self._log.info(f"config.instrument_types={instrument_types}", LogColor.BLUE)
        self._log.info(f"config.contract_types={contract_types}", LogColor.BLUE)
        self._log.info(f"{config.margin_mode=}", LogColor.BLUE)
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.use_fills_channel=}", LogColor.BLUE)
        self._log.info(f"{config.use_mm_mass_cancel=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)

        # Set account ID
        account_id = AccountId(f"{name or OKX_VENUE.value}-master")
        self._set_account_id(account_id)

        # Create pyo3 account ID for Rust HTTP client
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)

        # HTTP API
        self._http_client = client
        self._log.info(f"REST API key {self._http_client.api_key}", LogColor.BLUE)

        # Track algo order IDs for cancellation
        self._algo_order_ids: dict[ClientOrderId, str] = {}
        self._algo_order_instruments: dict[ClientOrderId, InstrumentId] = {}
        self._client_id_aliases: dict[ClientOrderId, ClientOrderId] = {}
        self._client_id_children: dict[ClientOrderId, ClientOrderId] = {}

        # WebSocket API
        self._ws_client = nautilus_pyo3.OKXWebSocketClient.with_credentials(
            url=config.base_url_ws or nautilus_pyo3.get_okx_ws_url_private(config.is_demo),
            account_id=self.pyo3_account_id,
        )
        self._ws_client_futures: set[asyncio.Future] = set()

        self._ws_business_client = nautilus_pyo3.OKXWebSocketClient.with_credentials(
            url=nautilus_pyo3.get_okx_ws_url_business(config.is_demo),
            account_id=self.pyo3_account_id,
        )
        self._ws_business_client_futures: set[asyncio.Future] = set()

        if account_type == AccountType.CASH:
            self._trade_mode = OKXTradeMode.CASH
        else:
            # TODO: Initially support isolated margin only
            self._trade_mode = OKXTradeMode.ISOLATED

    @property
    def okx_instrument_provider(self) -> OKXInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        await self._cache_instruments()
        await self._update_account_state()

        # Check OKX-Nautilus clock sync
        server_time: int = await self._http_client.http_get_server_time()
        self._log.info(f"OKX server time {server_time} UNIX (ms)")

        nautilus_time: int = self._clock.timestamp_ms()
        self._log.info(f"Nautilus clock time {nautilus_time} UNIX (ms)")

        await self._ws_client.connect(
            instruments=self.okx_instrument_provider.instruments_pyo3(),
            callback=self._handle_msg,
        )

        # Wait for connection to be established
        await self._ws_client.wait_until_active(timeout_secs=10.0)
        self._log.info(f"Connected to {self._ws_client.url}", LogColor.BLUE)
        self._log.info(f"Private websocket API key {self._ws_client.api_key}", LogColor.BLUE)
        self._log.info("OKX API key authenticated", LogColor.GREEN)

        await self._ws_business_client.connect(
            instruments=self.okx_instrument_provider.instruments_pyo3(),
            callback=self._handle_msg,
        )

        # Wait for connection to be established
        await self._ws_business_client.wait_until_active(timeout_secs=10.0)
        self._log.info(
            f"Connected to business websocket {self._ws_business_client.url}",
            LogColor.BLUE,
        )

        for instrument_type in self._instrument_provider._instrument_types:
            await self._ws_client.subscribe_orders(instrument_type)
            await self._ws_business_client.subscribe_orders_algo(instrument_type)

            # Only subscribe to fills channel if VIP5+ (configurable)
            if self._config.use_fills_channel:
                self._log.info("Subscribing to fills channel", LogColor.BLUE)
                await self._ws_client.subscribe_fills(instrument_type)
            else:
                self._log.info(
                    "Using order status reports for fill information (standard for all users)",
                    LogColor.BLUE,
                )

        await self._ws_client.subscribe_account()

    async def _disconnect(self) -> None:
        # Shutdown websocket
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting websocket")

            await self._ws_client.close()

        # Shutdown business websocket
        if not self._ws_business_client.is_closed():
            self._log.info("Disconnecting business websocket")

            await self._ws_business_client.close()

            self._log.info(f"Disconnected from {self._ws_client.url}", LogColor.BLUE)

        self._http_client.cancel_all_requests()

        # Cancel any pending futures
        all_futures = self._ws_client_futures | self._ws_business_client_futures
        for future in all_futures:
            if not future.done():
                future.cancel()

        if all_futures:
            try:
                await asyncio.wait_for(
                    asyncio.gather(*all_futures, return_exceptions=True),
                    timeout=2.0,
                )
            except TimeoutError:
                self._log.warning("Timeout while waiting for websockets shutdown to complete")

        self._ws_client_futures.clear()
        self._ws_business_client_futures.clear()

    async def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses.
        instruments_pyo3 = self.okx_instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    async def _update_account_state(self) -> None:
        try:
            pyo3_account_state = await self._http_client.request_account_state(self.pyo3_account_id)
            account_state = AccountState.from_dict(pyo3_account_state.to_dict())

            self.generate_account_state(
                balances=account_state.balances,
                margins=account_state.margins,
                reported=True,
                ts_event=self._clock.timestamp_ns(),
            )
        except Exception as e:
            self._log.error(f"Failed to update account state: {e}")

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        # Wait for instruments to be cached
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        self._log.debug(
            f"Requesting OrderStatusReports "
            f"{repr(command.instrument_id) if command.instrument_id else ''}"
            "...",
        )

        pyo3_reports: list[nautilus_pyo3.OrderStatusReport] = []
        reports: list[OrderStatusReport] = []

        try:
            if command.instrument_id:
                # Request for specific instrument
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )
                response = await self._http_client.request_order_status_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                    start=ensure_pydatetime_utc(command.start),
                    end=ensure_pydatetime_utc(command.end),
                    open_only=command.open_only,
                )
                pyo3_reports.extend(response)
            else:
                for instrument_type in self._config.instrument_types:
                    response = await self._http_client.request_order_status_reports(
                        account_id=self.pyo3_account_id,
                        instrument_type=instrument_type,
                        start=ensure_pydatetime_utc(command.start),
                        end=ensure_pydatetime_utc(command.end),
                        open_only=command.open_only,
                    )
                    pyo3_reports.extend(response)

            for pyo3_report in pyo3_reports:
                report = OrderStatusReport.from_pyo3(pyo3_report)
                self._apply_client_order_alias(report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except ValueError as exc:
            if "request canceled" in str(exc).lower():
                self._log.debug("OrderStatusReports request cancelled during shutdown")
            else:
                self._log.exception("Failed to generate OrderStatusReports", exc)
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
        # Check instruments are cached
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        self._log.debug(
            "Requesting OrderStatusReport "
            + ", ".join(
                repr(x)
                for x in [
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                ]
                if x
            )
            + " ...",
        )

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        canonical_requested_id: ClientOrderId | None = None

        try:
            pyo3_reports: list[nautilus_pyo3.OrderStatusReport] = (
                await self._http_client.request_order_status_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                )
            )

            if not pyo3_reports:
                return None

            # Filter for the specific order we're looking for
            canonical_requested_id = self._canonical_client_order_id(command.client_order_id)
            self._log.warning(
                f"Resolving order status lookup for requested {command.client_order_id!r} -> canonical {canonical_requested_id!r}",
            )

            for pyo3_report in pyo3_reports:
                report = OrderStatusReport.from_pyo3(pyo3_report)
                self._apply_client_order_alias(report)
                canonical_report_id = self._canonical_client_order_id(report.client_order_id)
                if (
                    canonical_requested_id
                    and canonical_report_id is not None
                    and canonical_report_id == canonical_requested_id
                ) or (command.venue_order_id and report.venue_order_id == command.venue_order_id):
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
                    return report
        except ValueError as exc:
            if "request canceled" in str(exc).lower():
                self._log.debug("OrderStatusReport request cancelled during shutdown")
            else:
                self._log.exception("Failed to generate OrderStatusReport", exc)
        except Exception as e:
            self._log.exception("Failed to generate OrderStatusReport", e)

        if canonical_requested_id is not None:
            return await self._resolve_algo_fallback(
                canonical_requested_id,
                command,
                pyo3_instrument_id,
            )

        return None

    async def _resolve_algo_fallback(
        self,
        canonical_requested_id: ClientOrderId,
        command: GenerateOrderStatusReport,
        pyo3_instrument_id: nautilus_pyo3.InstrumentId,
    ) -> OrderStatusReport | None:
        fallback_ids: list[ClientOrderId] = []
        for candidate in (
            canonical_requested_id,
            self._exchange_client_order_id(command.client_order_id),
            command.client_order_id,
        ):
            if candidate is not None and candidate not in fallback_ids:
                fallback_ids.append(candidate)

        algo_ids: set[str] = set()
        for candidate in fallback_ids:
            candidate_report = await self._fetch_algo_order_status_report(
                candidate,
                pyo3_instrument_id,
            )
            if candidate_report is not None:
                return candidate_report
            algo_id = self._algo_order_ids.get(candidate)
            if algo_id is not None:
                algo_ids.add(algo_id)

        for algo_id in algo_ids:
            candidate_report = await self._fetch_algo_order_status_report_by_algo_id(
                algo_id,
                pyo3_instrument_id,
            )
            if candidate_report is not None:
                return candidate_report

        exchange_client_order_id = self._exchange_client_order_id(command.client_order_id)
        algo_ids_repr = sorted(algo_ids) if algo_ids else None
        self._log.debug(
            f"Did not receive OrderStatusReport for client_id={command.client_order_id!r} "
            f"(exchange={exchange_client_order_id!r}, venue_order_id={command.venue_order_id!r}, "
            f"algo_ids={algo_ids_repr})",
        )

        return None

    async def _fetch_algo_order_status_report(
        self,
        query_client_order_id: ClientOrderId,
        pyo3_instrument_id: nautilus_pyo3.InstrumentId,
    ) -> OrderStatusReport | None:
        try:
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(
                query_client_order_id.value,
            )
            algo_report = await self._http_client.request_algo_order_status_report(
                account_id=self.pyo3_account_id,
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
            )
            if algo_report is None:
                return None

            report = OrderStatusReport.from_pyo3(algo_report)
            self._apply_client_order_alias(report)
            self._log.debug(
                f"Resolved OKX algo order status via fallback for {query_client_order_id!r}",
            )
            return report
        except ValueError as exc:
            if "404" in str(exc) or "Not Found" in str(exc):
                self._log.debug(
                    f"OKX algo order status not found for {query_client_order_id!r} (404)",
                )
            else:
                self._log.exception("Failed to generate OKX algo OrderStatusReport", exc)
        except Exception as exc:
            self._log.exception("Failed to generate OKX algo OrderStatusReport", exc)

        return None

    async def _fetch_algo_order_status_report_by_algo_id(
        self,
        algo_id: str,
        pyo3_instrument_id: nautilus_pyo3.InstrumentId,
    ) -> OrderStatusReport | None:
        try:
            algo_reports = await self._http_client.request_algo_order_status_reports(
                account_id=self.pyo3_account_id,
                instrument_id=pyo3_instrument_id,
                algo_id=algo_id,
            )
            for algo_report in algo_reports:
                report = OrderStatusReport.from_pyo3(algo_report)
                self._apply_client_order_alias(report)
                self._log.debug(
                    f"Resolved OKX algo order status via algo_id={algo_id}",
                )
                return report
        except ValueError as exc:
            if "404" in str(exc) or "Not Found" in str(exc):
                self._log.debug(
                    f"OKX algo order status not found for algo_id={algo_id} (404)",
                )
            else:
                self._log.exception("Failed to query OKX algo order by algo_id", exc)
        except Exception as exc:
            self._log.exception("Failed to query OKX algo order by algo_id", exc)

        return None

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        # Check instruments cache first
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        self._log.debug(
            "Requesting FillReports "
            + ", ".join(
                repr(x)
                for x in [
                    command.instrument_id,
                    command.venue_order_id,
                ]
                if x
            )
            + " ...",
        )

        pyo3_reports: list[nautilus_pyo3.FillReport] = []
        reports: list[FillReport] = []

        try:
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )
                response = await self._http_client.request_fill_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                    start=ensure_pydatetime_utc(command.start),
                    end=ensure_pydatetime_utc(command.end),
                )
                pyo3_reports.extend(response)
            else:
                for instrument_type in self._config.instrument_types:
                    response = await self._http_client.request_fill_reports(
                        account_id=self.pyo3_account_id,
                        instrument_type=instrument_type,
                        start=ensure_pydatetime_utc(command.start),
                        end=ensure_pydatetime_utc(command.end),
                    )
                    pyo3_reports.extend(response)

            for pyo3_report in pyo3_reports:
                report = FillReport.from_pyo3(pyo3_report)
                canonical_id = self._canonical_client_order_id(report.client_order_id)
                if canonical_id is not None:
                    report.client_order_id = canonical_id
                reports.append(report)
        except ValueError as exc:
            if "request canceled" in str(exc).lower():
                self._log.debug("FillReports request cancelled during shutdown")
            else:
                self._log.exception("Failed to generate FillReports", exc)
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

        self._log.debug(
            f"Requesting PositionStatusReports"
            f" {repr(command.instrument_id) if command.instrument_id else ''}"
            " ...",
        )

        pyo3_reports: list[nautilus_pyo3.PositionStatusReport] = []
        reports: list[PositionStatusReport] = []

        try:
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )
                response = await self._http_client.request_position_status_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                )

                if not response:
                    instrument = self._cache.instrument(command.instrument_id)
                    if instrument is None:
                        raise RuntimeError(
                            f"Cannot create FLAT position report - instrument {command.instrument_id} not found",
                        )

                    report = PositionStatusReport.create_flat(
                        account_id=self.account_id,
                        instrument_id=command.instrument_id,
                        size_precision=instrument.size_precision,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    reports.append(report)
                else:
                    pyo3_reports.extend(response)
            else:
                for instrument_type in self._config.instrument_types:
                    response = await self._http_client.request_position_status_reports(
                        account_id=self.pyo3_account_id,
                        instrument_type=instrument_type,
                    )
                    pyo3_reports.extend(response)

            for pyo3_report in pyo3_reports:
                report = PositionStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except ValueError as exc:
            if "request canceled" in str(exc).lower():
                self._log.debug("PositionReports request cancelled during shutdown")
            else:
                self._log.exception("Failed to generate PositionReports", exc)
        except Exception as e:
            self._log.exception("Failed to generate PositionReports", e)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} PositionReport{plural}")

        return reports

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _query_account(self, _command: QueryAccount) -> None:
        # TODO: Specific account ID (sub account) not yet supported
        await self._update_account_state()

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order: {order}")
            return

        # Generate OrderSubmitted event here to ensure correct event sequencing
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        # Check if this is a conditional order that needs to go via REST API
        is_conditional = order.order_type in (
            OrderType.STOP_MARKET,
            OrderType.STOP_LIMIT,
            OrderType.MARKET_IF_TOUCHED,
            OrderType.LIMIT_IF_TOUCHED,
        )

        if is_conditional:
            await self._submit_algo_order_http(command)
        else:
            await self._submit_order_websocket(command)

    async def _submit_order_websocket(self, command: SubmitOrder) -> None:
        order = command.order

        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_side = order_side_to_pyo3(order.side)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None
        pyo3_trigger_price = (
            nautilus_pyo3.Price.from_str(str(order.trigger_price))
            if order.has_trigger_price
            else None
        )

        pyo3_time_in_force = (
            time_in_force_to_pyo3(order.time_in_force) if order.time_in_force else None
        )

        td_mode = self._trade_mode

        if command.params:
            td_mode_str = command.params.get("td_mode")
            if td_mode_str:
                try:
                    td_mode = OKXTradeMode(td_mode_str)
                except ValueError:
                    self._log.warning(
                        f"Failed to parse OKXTradeMode: Valid modes are 'cash', 'isolated', 'cross', 'spot_isolated', "
                        f"falling back to '{str(self._trade_mode).lower()}'",
                    )
                    td_mode = self._trade_mode

        try:
            await self._ws_client.submit_order(
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
                td_mode=td_mode,
                client_order_id=pyo3_client_order_id,
                order_side=pyo3_order_side,
                order_type=pyo3_order_type,
                quantity=pyo3_quantity,
                price=pyo3_price,
                trigger_price=pyo3_trigger_price,
                time_in_force=pyo3_time_in_force,
                post_only=order.is_post_only,
                reduce_only=order.is_reduce_only,
                quote_quantity=order.is_quote_quantity,
            )
        except Exception as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _submit_algo_order_http(self, command: SubmitOrder) -> None:
        order = command.order

        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_side = order_side_to_pyo3(order.side)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_trigger_price = nautilus_pyo3.Price.from_str(str(order.trigger_price))

        pyo3_limit_price = (
            nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None
        )

        pyo3_trigger_type = (
            trigger_type_to_pyo3(order.trigger_type) if hasattr(order, "trigger_type") else None
        )

        td_mode = self._trade_mode
        if command.params and "td_mode" in command.params:
            td_mode_str = command.params["td_mode"]
            try:
                td_mode = OKXTradeMode(td_mode_str)
            except ValueError:
                self._log.warning(f"Invalid trade mode '{td_mode_str}', using default")

        try:
            response = await self._http_client.place_algo_order(
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
                td_mode=td_mode,
                client_order_id=pyo3_client_order_id,
                order_side=pyo3_order_side,
                order_type=pyo3_order_type,
                quantity=pyo3_quantity,
                trigger_price=pyo3_trigger_price,
                trigger_type=pyo3_trigger_type,
                limit_price=pyo3_limit_price,
                reduce_only=order.is_reduce_only if order.is_reduce_only else None,
            )

            if response.get("s_code") and response["s_code"] != "0":
                raise ValueError(f"OKX API error: {response.get('s_msg', 'Unknown error')}")

            algo_id = response.get("algo_id")
            if algo_id:
                self._algo_order_ids[order.client_order_id] = algo_id
                self._algo_order_instruments[order.client_order_id] = order.instrument_id
        except Exception as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
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

        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        resolved_client_order_id = self._exchange_client_order_id(command.client_order_id)
        self._log.debug(
            "Modifying OKX order using exchange id "
            f"{resolved_client_order_id!r} (canonical "
            f"{self._canonical_client_order_id(command.client_order_id)!r}, "
            f"requested {command.client_order_id!r})",
        )
        pyo3_client_order_id = (
            nautilus_pyo3.ClientOrderId(resolved_client_order_id.value)
            if resolved_client_order_id is not None
            else None
        )
        pyo3_venue_order_id = (
            nautilus_pyo3.VenueOrderId(command.venue_order_id.value)
            if command.venue_order_id
            else None
        )
        pyo3_price = nautilus_pyo3.Price.from_str(str(command.price)) if command.price else None
        pyo3_quantity = (
            nautilus_pyo3.Quantity.from_str(str(command.quantity)) if command.quantity else None
        )

        try:
            await self._ws_client.modify_order(
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
                price=pyo3_price,
                quantity=pyo3_quantity,
                client_order_id=pyo3_client_order_id,
                venue_order_id=pyo3_venue_order_id,
            )
        except Exception as e:
            self.generate_order_modify_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

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
            canonical_client_order_id = self._canonical_client_order_id(
                command.client_order_id,
            )
            alias_lookup_key = canonical_client_order_id or command.client_order_id
            algo_id = self._algo_order_ids.get(alias_lookup_key)
            if algo_id:
                self._log.debug(
                    f"Cancelling OKX algo order using algo_id {algo_id} "
                    f"for canonical {alias_lookup_key!r} (requested {command.client_order_id!r})",
                )
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )
                try:
                    await self._http_client.cancel_algo_order(
                        instrument_id=pyo3_instrument_id,
                        algo_id=algo_id,
                    )
                except ValueError as exc:
                    message = str(exc)
                    alias_text = str(alias_lookup_key) if alias_lookup_key is not None else ""
                    client_text = str(command.client_order_id) if command.client_order_id else ""
                    if (
                        "already canceled" not in message
                        and algo_id not in message
                        and alias_text not in message
                        and client_text not in message
                    ):
                        raise
                if alias_lookup_key is not None:
                    del self._algo_order_ids[alias_lookup_key]
                    self._algo_order_instruments.pop(alias_lookup_key, None)
            else:
                pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
                pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )
                resolved_client_order_id = self._exchange_client_order_id(
                    command.client_order_id,
                )
                self._log.debug(
                    "Cancelling OKX order over websocket using exchange id "
                    f"{resolved_client_order_id!r} (canonical {canonical_client_order_id!r}, "
                    f"requested {command.client_order_id!r})",
                )
                pyo3_client_order_id = (
                    nautilus_pyo3.ClientOrderId(resolved_client_order_id.value)
                    if resolved_client_order_id is not None
                    else None
                )
                pyo3_venue_order_id = (
                    nautilus_pyo3.VenueOrderId(command.venue_order_id.value)
                    if command.venue_order_id
                    else None
                )

                await self._ws_client.cancel_order(
                    trader_id=pyo3_trader_id,
                    strategy_id=pyo3_strategy_id,
                    instrument_id=pyo3_instrument_id,
                    client_order_id=pyo3_client_order_id,
                    venue_order_id=pyo3_venue_order_id,
                )
        except Exception as e:
            self.generate_order_cancel_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        if self._config.use_mm_mass_cancel:
            await self._cancel_all_orders_mass_cancel(command)
        else:
            await self._cancel_all_orders_individually(command)

    async def _cancel_algo_order_fallback(
        self,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        algo_id: str,
    ) -> None:
        self._log.debug(
            f"Fallback cancel for OKX algo order {client_order_id!r} using algo_id {algo_id}",
        )
        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument_id.value)
            await self._http_client.cancel_algo_order(
                instrument_id=pyo3_instrument_id,
                algo_id=algo_id,
            )
            self._log.debug(
                f"Successfully cancelled OKX algo order {client_order_id!r} via fallback",
            )
        except Exception as e:
            self._log.warning(
                f"Failed fallback cancel for OKX algo order {client_order_id!r} (algo_id={algo_id}): {e}",
            )
        finally:
            self._algo_order_ids.pop(client_order_id, None)
            self._algo_order_instruments.pop(client_order_id, None)

    async def _cancel_all_orders_mass_cancel(self, command: CancelAllOrders) -> None:
        # Use OKX's mass-cancel WebSocket endpoint for market makers
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        try:
            await self._ws_client.mass_cancel_orders(
                instrument_id=pyo3_instrument_id,
            )
        except Exception as e:
            # If mass cancel fails, generate cancel rejected events for all open orders
            orders_open = self._cache.orders_open(instrument_id=command.instrument_id)
            for order in orders_open:
                if not order.is_closed:
                    self.generate_order_cancel_rejected(
                        strategy_id=order.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=order.venue_order_id,
                        reason=str(e),
                        ts_event=self._clock.timestamp_ns(),
                    )

    async def _cancel_all_orders_individually(self, command: CancelAllOrders) -> None:
        orders_open: list[Order] = self._cache.orders_open(instrument_id=command.instrument_id)
        processed: set[ClientOrderId] = set()
        for order in orders_open:
            if order.is_closed:
                continue

            cancel_command = CancelOrder(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                command_id=command.id,
                ts_init=command.ts_init,
            )
            await self._cancel_order(cancel_command)
            processed.add(order.client_order_id)

        for client_order_id, algo_id in list(self._algo_order_ids.items()):
            if client_order_id in processed:
                continue
            instrument_id = self._algo_order_instruments.get(client_order_id)
            if instrument_id is None or instrument_id != command.instrument_id:
                continue

            await self._cancel_algo_order_fallback(
                client_order_id=client_order_id,
                instrument_id=instrument_id,
                algo_id=algo_id,
            )

    # -- WEBSOCKET HANDLERS -----------------------------------------------------------------------

    def _is_external_order(self, client_order_id: ClientOrderId) -> bool:
        return not client_order_id or not self._cache.strategy_id_for_order(client_order_id)

    def _handle_msg(self, msg: Any) -> None:
        if isinstance(msg, nautilus_pyo3.OKXWebSocketError):
            self._log.error(repr(msg))
            return

        try:
            if isinstance(msg, nautilus_pyo3.AccountState):
                self._handle_account_state(msg)
            elif isinstance(msg, nautilus_pyo3.OrderRejected):
                self._handle_order_rejected_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderCancelRejected):
                self._handle_order_cancel_rejected_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderModifyRejected):
                self._handle_order_modify_rejected_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderStatusReport):
                self._handle_order_status_report_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.FillReport):
                self._handle_fill_report_pyo3(msg)
            else:
                self._log.debug(f"Received unhandled message type: {type(msg)}")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)

    def _handle_account_state(self, pyo3_account_state: nautilus_pyo3.AccountState) -> None:
        account_state = AccountState.from_dict(pyo3_account_state.to_dict())
        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=account_state.is_reported,
            ts_event=account_state.ts_event,
        )

    def _handle_order_rejected_pyo3(self, pyo3_event: nautilus_pyo3.OrderRejected) -> None:
        event = OrderRejected.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)

    def _handle_order_cancel_rejected_pyo3(
        self,
        pyo3_event: nautilus_pyo3.OrderCancelRejected,
    ) -> None:
        event = OrderCancelRejected.from_dict(pyo3_event.to_dict())
        reason = event.reason or ""
        canonical = self._canonical_client_order_id(event.client_order_id)
        canonical_repr = repr(canonical) if canonical is not None else ""
        duplicate_reason = reason.endswith(repr(event.client_order_id)) or (
            canonical_repr and reason.endswith(canonical_repr)
        )
        if duplicate_reason:
            return
        order = self._cache.order(event.client_order_id)
        if order is None or order.is_closed:
            return
        self._send_order_event(event)

    def _handle_order_modify_rejected_pyo3(
        self,
        pyo3_event: nautilus_pyo3.OrderModifyRejected,
    ) -> None:
        event = OrderModifyRejected.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)

    def _handle_order_status_report_pyo3(  # noqa: C901 (too complex)
        self,
        pyo3_report: nautilus_pyo3.OrderStatusReport,
    ) -> None:
        # Discard order status reports until account is properly initialized
        # Reconciliation will handle getting the current state of open orders
        if not self.is_connected or not self.account_id or not self._cache.account(self.account_id):
            self._log.debug(
                f"Discarding order status report during connection sequence: {pyo3_report.client_order_id!r}",
            )
            return

        report = OrderStatusReport.from_pyo3(pyo3_report)
        self._apply_client_order_alias(report)

        if self._is_external_order(report.client_order_id):
            self._send_order_status_report(report)
            return

        order = self._cache.order(report.client_order_id)
        canonical_client_order_id = (
            self._canonical_client_order_id(report.client_order_id) or report.client_order_id
        )
        algo_id_for_client = self._algo_order_ids.get(canonical_client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process order status report - order for {report.client_order_id!r} not found",
            )
            return

        if order.is_closed:
            return

        # For algo orders (stop orders), store the algo_id mapping
        # The venue_order_id is actually the algo_id for algo orders
        if order.order_type in (OrderType.STOP_MARKET, OrderType.STOP_LIMIT):
            child = self._client_id_children.get(report.client_order_id)
            venue_changed = (
                order.venue_order_id is not None
                and report.venue_order_id is not None
                and order.venue_order_id != report.venue_order_id
            )
            if (
                (child is None or child == report.client_order_id)
                and report.venue_order_id
                and report.client_order_id
                and not venue_changed
            ):
                self._algo_order_ids[canonical_client_order_id] = str(report.venue_order_id)
                self._algo_order_instruments[canonical_client_order_id] = order.instrument_id

        if report.order_status == OrderStatus.REJECTED:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                reason=report.reason,
                ts_event=report.ts_last,
            )
            self._clear_client_order_aliases(report)
            self._algo_order_ids.pop(canonical_client_order_id, None)
            self._algo_order_instruments.pop(canonical_client_order_id, None)
        elif report.order_status == OrderStatus.ACCEPTED:
            if order.status in (OrderStatus.FILLED, OrderStatus.CANCELED, OrderStatus.EXPIRED):
                return
            venue_changed = (
                order.venue_order_id is not None
                and report.venue_order_id is not None
                and order.venue_order_id != report.venue_order_id
            )
            venue_is_original_algo = bool(
                venue_changed
                and algo_id_for_client
                and report.venue_order_id is not None
                and str(report.venue_order_id) == str(algo_id_for_client),
            )
            if venue_changed and not venue_is_original_algo:
                self.generate_order_updated(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    quantity=report.quantity or order.quantity,
                    price=(
                        report.price if report.price is not None else getattr(order, "price", None)
                    ),
                    trigger_price=report.trigger_price or getattr(order, "trigger_price", None),
                    ts_event=report.ts_last,
                    venue_order_id_modified=True,
                )
                self._algo_order_ids.pop(canonical_client_order_id, None)
                self._algo_order_instruments.pop(canonical_client_order_id, None)
                return
            if venue_is_original_algo:
                return
            if is_order_updated(order, report):
                self.generate_order_updated(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    quantity=report.quantity or order.quantity,
                    price=(
                        report.price if report.price is not None else getattr(order, "price", None)
                    ),
                    trigger_price=report.trigger_price or getattr(order, "trigger_price", None),
                    ts_event=report.ts_last,
                )
                return
            if order.status == OrderStatus.ACCEPTED:
                return
            self.generate_order_accepted(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
            )
        elif report.order_status == OrderStatus.CANCELED:
            self.generate_order_canceled(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
            )
            self._clear_client_order_aliases(report)
            self._algo_order_ids.pop(canonical_client_order_id, None)
            self._algo_order_instruments.pop(canonical_client_order_id, None)
            return
        elif report.order_status == OrderStatus.EXPIRED:
            self.generate_order_expired(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
            )
            self._clear_client_order_aliases(report)
            self._algo_order_ids.pop(canonical_client_order_id, None)
            self._algo_order_instruments.pop(canonical_client_order_id, None)
        elif report.order_status == OrderStatus.TRIGGERED:
            if (
                order.venue_order_id is not None
                and report.venue_order_id is not None
                and order.venue_order_id != report.venue_order_id
            ):
                self.generate_order_updated(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    quantity=report.quantity or order.quantity,
                    price=(
                        report.price if report.price is not None else getattr(order, "price", None)
                    ),
                    trigger_price=report.trigger_price or getattr(order, "trigger_price", None),
                    ts_event=report.ts_last,
                    venue_order_id_modified=True,
                )

            if order.order_type in (
                OrderType.STOP_LIMIT,
                OrderType.TRAILING_STOP_LIMIT,
                OrderType.LIMIT_IF_TOUCHED,
            ):
                self.generate_order_triggered(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    ts_event=report.ts_last,
                )
        elif report.order_status == OrderStatus.FILLED:
            self._clear_client_order_aliases(report)
        else:
            # Fills should be handled from FillReports
            self._log.warning(f"Received unhandled OrderStatusReport: {report}")

    def _handle_order_update(self, order: Any, report: OrderStatusReport) -> None:
        self.generate_order_updated(
            strategy_id=order.strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            quantity=report.quantity,
            price=report.price,
            trigger_price=report.trigger_price,
            ts_event=report.ts_last,
        )

    def _handle_fill_report_pyo3(self, pyo3_report: nautilus_pyo3.FillReport) -> None:
        report = FillReport.from_pyo3(pyo3_report)

        if self._is_external_order(report.client_order_id):
            self._send_fill_report(report)
            return

        order = self._cache.order(report.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process fill report - order for {report.client_order_id!r} not found",
            )
            return

        instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot process fill report - instrument {order.instrument_id} not found",
            )
            return

        updated_event = None
        if (
            order.venue_order_id is not None
            and report.venue_order_id is not None
            and order.venue_order_id != report.venue_order_id
        ):
            updated_event = OrderUpdated(
                trader_id=self.trader_id,
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=report.venue_order_id,
                account_id=self.account_id,
                quantity=order.quantity,
                price=getattr(order, "price", None),
                trigger_price=getattr(order, "trigger_price", None),
                event_id=UUID4(),
                ts_event=report.ts_event,
                ts_init=self._clock.timestamp_ns(),
            )
            order.apply(updated_event)
            self._cache.add_venue_order_id(
                order.client_order_id,
                report.venue_order_id,
                overwrite=True,
            )
            self._send_order_event(updated_event)

        self.generate_order_filled(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=report.venue_order_id,
            venue_position_id=report.venue_position_id,
            trade_id=report.trade_id,
            order_side=order.side,
            order_type=order.order_type,
            last_qty=report.last_qty,
            last_px=report.last_px,
            quote_currency=instrument.quote_currency,
            commission=report.commission,
            liquidity_side=report.liquidity_side,
            ts_event=report.ts_event,
        )
        canonical_client_order_id = (
            self._canonical_client_order_id(order.client_order_id) or order.client_order_id
        )
        self._algo_order_ids.pop(canonical_client_order_id, None)
        self._algo_order_instruments.pop(canonical_client_order_id, None)

    def _resolve_client_order_ids(
        self,
        client_order_id: ClientOrderId | None,
    ) -> tuple[ClientOrderId | None, ClientOrderId | None]:
        if client_order_id is None:
            return None, None

        canonical = self._client_id_aliases.get(client_order_id, client_order_id)
        exchange = self._client_id_children.get(canonical)

        if exchange and exchange != canonical:
            self._log.debug(
                f"Resolved client order alias {client_order_id!r} -> canonical {canonical!r}, exchange {exchange!r}",
            )
            return canonical, exchange

        if canonical != client_order_id:
            self._log.debug(
                f"Resolved client order alias {client_order_id!r} -> canonical {canonical!r}",
            )
            return canonical, client_order_id

        return canonical, canonical

    def _canonical_client_order_id(
        self,
        client_order_id: ClientOrderId | None,
    ) -> ClientOrderId | None:
        canonical, _ = self._resolve_client_order_ids(client_order_id)
        return canonical

    def _exchange_client_order_id(
        self,
        client_order_id: ClientOrderId | None,
    ) -> ClientOrderId | None:
        _, exchange = self._resolve_client_order_ids(client_order_id)
        return exchange

    def _register_client_order_aliases(
        self,
        parent_id: ClientOrderId | None,
        linked_order_ids: list[ClientOrderId] | None,
    ) -> None:
        if parent_id is None:
            return

        canonical_parent, _ = self._resolve_client_order_ids(parent_id)
        if canonical_parent is None:
            canonical_parent = parent_id

        self._client_id_aliases[parent_id] = canonical_parent
        self._client_id_children.setdefault(canonical_parent, canonical_parent)
        canonical_parent_ref = canonical_parent

        if not linked_order_ids:
            return

        for linked_id in linked_order_ids:
            if linked_id is None:
                continue

            self._client_id_aliases[linked_id] = canonical_parent_ref

            if linked_id != canonical_parent_ref:
                self._client_id_children[canonical_parent_ref] = linked_id

            self._log.debug(
                f"Registered OKX alias parent {canonical_parent_ref!r} <-> child {linked_id!r}",
            )

    def _apply_client_order_alias(self, report: OrderStatusReport) -> None:
        parent_id = report.client_order_id
        linked_ids = getattr(report, "linked_order_ids", None)

        if linked_ids:
            linked_ids = list(linked_ids)
            report.linked_order_ids = linked_ids

        self._register_client_order_aliases(parent_id, linked_ids)

        canonical_id = self._canonical_client_order_id(parent_id)
        if canonical_id is None or parent_id == canonical_id:
            return

        if not report.linked_order_ids:
            report.linked_order_ids = []

        if parent_id not in report.linked_order_ids:
            report.linked_order_ids.append(parent_id)

        report.client_order_id = canonical_id
        self._log.debug(
            f"Applied OKX alias: parent {parent_id!r} -> canonical {canonical_id!r} with linked {report.linked_order_ids}",
        )

    def _clear_client_order_aliases(self, report: OrderStatusReport) -> None:
        client_order_ids: list[ClientOrderId] = []

        if report.client_order_id:
            client_order_ids.append(report.client_order_id)
        if report.linked_order_ids:
            client_order_ids.extend(report.linked_order_ids)

        self._clear_client_order_aliases_from_ids(client_order_ids)

    def _clear_client_order_aliases_from_ids(
        self,
        ids: list[ClientOrderId | None],
    ) -> None:
        for identifier in ids:
            if identifier is None:
                continue
            self._client_id_aliases.pop(identifier, None)

            for key, value in list(self._client_id_aliases.items()):
                if value == identifier:
                    self._client_id_aliases.pop(key, None)

            canonical = self._client_id_children.pop(identifier, None)
            if canonical is not None and canonical != identifier:
                self._client_id_aliases.pop(canonical, None)

            self._algo_order_ids.pop(identifier, None)
            self._algo_order_instruments.pop(identifier, None)


def is_order_updated(order: Order, report: OrderStatusReport) -> bool:
    if order.has_price and report.price and order.price != report.price:
        return True

    if (
        order.has_trigger_price
        and report.trigger_price
        and order.trigger_price != report.trigger_price
    ):
        return True

    return order.quantity != report.quantity
