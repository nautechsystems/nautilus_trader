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

import asyncio
from dataclasses import dataclass
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.adapters.okx.types import OkxInstrument
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.secure import mask_api_key
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.core.nautilus_pyo3 import OKXEnvironment
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.core.nautilus_pyo3 import OKXMarginMode
from nautilus_trader.core.nautilus_pyo3 import OKXTradeMode
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
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.functions import order_side_to_pyo3
from nautilus_trader.model.functions import order_type_to_pyo3
from nautilus_trader.model.functions import time_in_force_to_pyo3
from nautilus_trader.model.functions import trigger_type_to_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import CryptoOption
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order


@dataclass(frozen=True)
class _OKXAttachedOcoBinding:
    parent_client_order_id: ClientOrderId
    attach_client_order_id: ClientOrderId
    instrument_id: InstrumentId
    sl_client_order_id: ClientOrderId | None
    tp_client_order_id: ClientOrderId | None

    def child_client_order_ids(self) -> list[ClientOrderId]:
        child_ids: list[ClientOrderId] = []

        if self.sl_client_order_id is not None:
            child_ids.append(self.sl_client_order_id)
        if self.tp_client_order_id is not None and self.tp_client_order_id not in child_ids:
            child_ids.append(self.tp_client_order_id)
        return child_ids

    def all_client_order_ids(self) -> list[ClientOrderId]:
        ids = [self.parent_client_order_id, self.attach_client_order_id]
        for child_id in self.child_client_order_ids():
            if child_id not in ids:
                ids.append(child_id)
        return ids


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

        account_type = self._derive_account_type(instrument_provider, config)

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
        self._instrument_types = config.instrument_types

        instrument_types = [i.name.upper() for i in config.instrument_types]
        contract_types = (
            [c.name.upper() for c in config.contract_types] if config.contract_types else None
        )
        margin_mode = str(config.margin_mode) if config.margin_mode else None

        # Resolve environment: explicit setting takes precedence over is_demo
        self._environment = (
            config.environment
            if config.environment is not None
            else (OKXEnvironment.DEMO if config.is_demo else OKXEnvironment.LIVE)
        )

        # Configuration
        self._config = config
        self._log.info(f"config.instrument_types={instrument_types}", LogColor.BLUE)
        self._log.info(f"{config.instrument_families=}", LogColor.BLUE)
        self._log.info(f"config.contract_types={contract_types}", LogColor.BLUE)
        self._log.info(f"environment={self._environment}", LogColor.BLUE)
        self._log.info(f"config.margin_mode={margin_mode}", LogColor.BLUE)
        self._log.info(f"{config.use_spot_margin=}", LogColor.BLUE)
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)
        self._log.info(f"{config.use_fills_channel=}", LogColor.BLUE)
        self._log.info(f"{config.use_mm_mass_cancel=}", LogColor.BLUE)
        self._log.info(f"{config.use_spot_cash_position_reports=}", LogColor.BLUE)
        self._log.info(f"{config.proxy_url=}", LogColor.BLUE)

        if config.use_spot_cash_position_reports:
            self._log.warning(
                "SPOT CASH position reports enabled - positive wallet balances (cash_bal - liab) will be treated as LONG "
                "positions and negative balances (borrowing) as SHORT positions; this may lead to unintended "
                "liquidation of wallet assets if strategies are not designed to handle SPOT positions properly",
                LogColor.YELLOW,
            )

        # Set account ID
        account_id = AccountId(f"{name or OKX_VENUE.value}-master")
        self._set_account_id(account_id)

        # Create pyo3 account ID for Rust HTTP client
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)

        # HTTP API
        self._http_client = client
        if self._http_client.api_key:
            masked_key = mask_api_key(self._http_client.api_key)
            self._log.info(f"REST API key {masked_key}", LogColor.BLUE)

        # Track algo order IDs for cancellation
        self._algo_order_ids: dict[ClientOrderId, str] = {}
        self._algo_order_instruments: dict[ClientOrderId, InstrumentId] = {}
        self._client_id_aliases: dict[ClientOrderId, ClientOrderId] = {}
        self._client_id_children: dict[ClientOrderId, ClientOrderId] = {}
        self._attached_oco_bindings: dict[ClientOrderId, _OKXAttachedOcoBinding] = {}

        # WebSocket API
        _private_url = config.base_url_ws or nautilus_pyo3.get_okx_ws_url_private(self._environment)
        self._ws_client = nautilus_pyo3.OKXWebSocketClient.with_credentials(
            url=_private_url,
            account_id=self.pyo3_account_id,
            heartbeat=20,
            auth_timeout_secs=config.ws_auth_timeout_secs,
            proxy_url=config.proxy_url,
        )
        self._ws_client_futures: set[asyncio.Future] = set()

        self._ws_business_client = nautilus_pyo3.OKXWebSocketClient.with_credentials(
            url=nautilus_pyo3.derive_okx_ws_url(_private_url, "business"),
            account_id=self.pyo3_account_id,
            heartbeat=20,
            auth_timeout_secs=config.ws_auth_timeout_secs,
            proxy_url=config.proxy_url,
        )
        self._ws_business_client_futures: set[asyncio.Future] = set()

        # Determine trade mode based on account type and configuration
        self._trade_mode = self._derive_trade_mode(account_type, config)

    @property
    def okx_instrument_provider(self) -> OKXInstrumentProvider:
        return self._instrument_provider

    def _derive_account_type(
        self,
        instrument_provider: OKXInstrumentProvider,
        config: OKXExecClientConfig,
    ) -> AccountType:
        is_spot_only = instrument_provider.instrument_types == (OKXInstrumentType.SPOT,)
        if is_spot_only and not config.use_spot_margin:
            return AccountType.CASH
        return AccountType.MARGIN

    def _derive_trade_mode(
        self,
        account_type: AccountType,
        config: OKXExecClientConfig,
    ) -> OKXTradeMode:
        is_cross_margin = config.margin_mode == OKXMarginMode.CROSS

        if account_type == AccountType.CASH:
            if not config.use_spot_margin:
                return OKXTradeMode.CASH
            # SPOT margin supports CROSS for leverage; ISOLATED is limited to copy or lead traders
            return OKXTradeMode.CROSS if is_cross_margin else OKXTradeMode.ISOLATED

        return OKXTradeMode.CROSS if is_cross_margin else OKXTradeMode.ISOLATED

    async def _check_clock_sync(self) -> None:
        try:
            server_time: int = await self._http_client.get_server_time()
            nautilus_time: int = self._clock.timestamp_ms()
            self._log.info(f"OKX server time {server_time} UNIX (ms)")
            self._log.info(f"Nautilus clock time {nautilus_time} UNIX (ms)")
        except Exception:
            self._log.warning("Failed to query OKX server time")

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        await self._cache_instruments()
        await self._update_account_state()
        await self._await_account_registered()

        self._log.info("OKX API key authenticated", LogColor.GREEN)

        self.create_task(self._check_clock_sync())

        await self._ws_client.connect(
            loop_=self._loop,
            instruments=self.okx_instrument_provider.instruments_pyo3(),
            callback=self._handle_msg,
        )
        self._ws_client.cache_inst_id_codes(self.okx_instrument_provider.inst_id_codes())

        # Wait for connection to be established
        await self._ws_client.wait_until_active(timeout_secs=30.0)
        self._log.info(f"Connected to {self._ws_client.url}", LogColor.BLUE)

        if self._ws_client.api_key:
            masked_key = mask_api_key(self._ws_client.api_key)
            self._log.info(f"WebSocket API key {masked_key}", LogColor.BLUE)

        self._log.info("OKX API key authenticated", LogColor.GREEN)

        await self._ws_business_client.connect(
            loop_=self._loop,
            instruments=self.okx_instrument_provider.instruments_pyo3(),
            callback=self._handle_msg,
        )

        # Wait for connection to be established
        await self._ws_business_client.wait_until_active(timeout_secs=30.0)
        self._log.info(
            f"Connected to business websocket {self._ws_business_client.url}",
            LogColor.BLUE,
        )

        subscribed_order_channels = set()
        subscribed_fills_channels = set()

        for instrument_type in self._instrument_types:
            if instrument_type not in subscribed_order_channels:
                self._log.info(
                    f"Subscribing to orders channel for instrument type: {instrument_type}",
                    LogColor.BLUE,
                )
                await self._ws_client.subscribe_orders(instrument_type)
                subscribed_order_channels.add(instrument_type)

            # For spot margin trading, also subscribe to MARGIN channel
            # OKX treats spot pairs with cross/isolated margin as MARGIN instrument type
            if (
                instrument_type == OKXInstrumentType.SPOT
                and self._config.use_spot_margin
                and self._config.margin_mode in (OKXMarginMode.CROSS, OKXMarginMode.ISOLATED)
                and OKXInstrumentType.MARGIN not in subscribed_order_channels
            ):
                self._log.info(
                    f"Also subscribing to MARGIN orders channel (spot margin mode: {self._config.margin_mode})",
                    LogColor.BLUE,
                )
                await self._ws_client.subscribe_orders(OKXInstrumentType.MARGIN)
                subscribed_order_channels.add(OKXInstrumentType.MARGIN)

            # OKX doesn't support algo orders channel for OPTIONS
            if instrument_type != OKXInstrumentType.OPTION:
                await self._ws_business_client.subscribe_orders_algo(instrument_type)
                await self._ws_business_client.subscribe_algo_advance(instrument_type)

            # Only subscribe to fills channel if VIP5+ (configurable)
            if self._config.use_fills_channel:
                if instrument_type not in subscribed_fills_channels:
                    self._log.info(
                        f"Subscribing to fills channel for instrument type: {instrument_type}",
                        LogColor.BLUE,
                    )
                    await self._ws_client.subscribe_fills(instrument_type)
                    subscribed_fills_channels.add(instrument_type)

                # Also subscribe to fills for MARGIN when spot margin is enabled
                if (
                    instrument_type == OKXInstrumentType.SPOT
                    and self._config.use_spot_margin
                    and self._config.margin_mode in (OKXMarginMode.CROSS, OKXMarginMode.ISOLATED)
                    and OKXInstrumentType.MARGIN not in subscribed_fills_channels
                ):
                    self._log.info(
                        f"Also subscribing to MARGIN fills channel (spot margin mode: {self._config.margin_mode})",
                        LogColor.BLUE,
                    )
                    await self._ws_client.subscribe_fills(OKXInstrumentType.MARGIN)
                    subscribed_fills_channels.add(OKXInstrumentType.MARGIN)
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
            self._http_client.cache_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    async def _update_account_state(self) -> None:
        pyo3_account_state = await self._http_client.request_account_state(self.pyo3_account_id)
        account_state = AccountState.from_dict(pyo3_account_state.to_dict())

        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

        if account_state.balances:
            self._log.info(
                f"Generated account state with {len(account_state.balances)} balance(s)",
            )

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
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "OrderStatusReports")

        self._log_report_receipt(
            len(reports),
            "OrderStatusReport",
            command.log_receipt_level,
        )

        return reports

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
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
            pyo3_reports: list[
                nautilus_pyo3.OrderStatusReport
            ] = await self._http_client.request_order_status_reports(
                account_id=self.pyo3_account_id,
                instrument_id=pyo3_instrument_id,
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
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "OrderStatusReport")

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

            algo_report = self._hydrate_zero_quantity_algo_report(algo_report)
            report = OrderStatusReport.from_pyo3(algo_report)
            self._apply_client_order_alias(report)
            self._log.debug(
                f"Resolved OKX algo order status via fallback for {query_client_order_id!r}",
            )
            return report
        except ValueError as e:
            if "404" in str(e) or "Not Found" in str(e):
                self._log.debug(
                    f"OKX algo order status not found for {query_client_order_id!r} (404)",
                )
            else:
                self._log.exception("Failed to generate OKX algo OrderStatusReport", e)
        except Exception as e:
            self._log.exception("Failed to generate OKX algo OrderStatusReport", e)

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
                algo_report = self._hydrate_zero_quantity_algo_report(algo_report)
                report = OrderStatusReport.from_pyo3(algo_report)
                self._apply_client_order_alias(report)
                self._log.debug(
                    f"Resolved OKX algo order status via algo_id={algo_id}",
                )
                return report
        except ValueError as e:
            if "404" in str(e) or "Not Found" in str(e):
                self._log.debug(
                    f"OKX algo order status not found for algo_id={algo_id} (404)",
                )
            else:
                self._log.exception("Failed to query OKX algo order by algo_id", e)
        except Exception as e:
            self._log.exception("Failed to query OKX algo order by algo_id", e)

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
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "FillReports")

        self._log_report_receipt(len(reports), "FillReport", LogLevel.INFO)

        return reports

    async def _generate_spot_position_reports_from_wallet(  # noqa: C901 (too complex)
        self,
        instrument_id: InstrumentId | None = None,
    ) -> list[PositionStatusReport]:
        reports: list[PositionStatusReport] = []

        try:
            okx_balance_details = await self._http_client.get_balance()

            if not okx_balance_details:
                self._log.warning("No OKX balance details returned from balance query")
                return reports

            # Calculate net balance: cash_bal - liab
            wallet_by_currency: dict[str, Decimal] = {}

            for detail in okx_balance_details:
                currency_code = detail.ccy
                cash_bal = Decimal(detail.cash_bal or "0")
                liab = Decimal(detail.liab or "0")
                net_balance = cash_bal - liab

                wallet_by_currency[currency_code] = (
                    wallet_by_currency.get(currency_code, Decimal(0)) + net_balance
                )

            if instrument_id:
                instrument = self._cache.instrument(instrument_id)
                if instrument is None:
                    raise ValueError(
                        f"Cannot generate SPOT position report: instrument not found for {instrument_id}",
                    )

                if not isinstance(instrument, CurrencyPair):
                    raise ValueError(
                        f"Cannot generate SPOT position report: {instrument_id} is not a CurrencyPair",
                    )

                currency_code = instrument.base_currency.code
                wallet_balance = wallet_by_currency.get(currency_code, Decimal(0))

                report = self._build_spot_position_report_from_wallet_balance(
                    instrument,
                    wallet_balance,
                )
                reports.append(report)
            else:
                for loaded in self._instrument_provider.get_all().values():
                    if not isinstance(loaded, CurrencyPair):
                        continue

                    currency_code = loaded.base_currency.code
                    wallet_balance = wallet_by_currency.get(currency_code, Decimal(0))
                    if wallet_balance == 0:
                        continue

                    report = self._build_spot_position_report_from_wallet_balance(
                        loaded,
                        wallet_balance,
                    )
                    reports.append(report)
        except Exception as e:
            self._log.exception("Failed to generate SPOT position report(s) from wallet", e)

        for report in reports:
            self._log.debug(f"Generated SPOT position report from wallet: {report}")

        return reports

    def _build_spot_position_report_from_wallet_balance(
        self,
        instrument: CurrencyPair,
        wallet_balance: Decimal,
    ) -> PositionStatusReport:
        position_side = PositionSide.LONG if wallet_balance > 0 else PositionSide.SHORT
        abs_balance = abs(wallet_balance)

        try:
            quantity = instrument.make_qty(str(abs_balance), round_down=True)
        except ValueError:
            quantity = Quantity.zero(instrument.size_precision)

        if quantity == 0:
            return PositionStatusReport.create_flat(
                account_id=self.account_id,
                instrument_id=instrument.id,
                size_precision=instrument.size_precision,
                ts_init=self._clock.timestamp_ns(),
                report_id=UUID4(),
            )

        return PositionStatusReport(
            account_id=self.account_id,
            instrument_id=instrument.id,
            position_side=position_side,
            quantity=quantity,
            avg_px_open=None,
            report_id=UUID4(),
            ts_last=self._clock.timestamp_ns(),
            ts_init=self._clock.timestamp_ns(),
        )

    async def generate_position_status_reports(  # noqa: C901 (too complex)
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
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
                instrument = self._cache.instrument(command.instrument_id)
                if instrument is None:
                    raise RuntimeError(
                        f"Cannot create position report - instrument {command.instrument_id} not found in cache",
                    )

                # TODO: Refactor the below
                if isinstance(instrument, CurrencyPair):
                    # SPOT instruments: check margin mode first
                    if self._config.use_spot_margin:
                        # SPOT MARGIN: Use positions API like SWAP/FUTURES (always)
                        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                            command.instrument_id.value,
                        )
                        response = await self._http_client.request_position_status_reports(
                            account_id=self.pyo3_account_id,
                            instrument_id=pyo3_instrument_id,
                            instrument_type=OKXInstrumentType.MARGIN,
                        )

                        if not response:
                            report = PositionStatusReport.create_flat(
                                account_id=self.account_id,
                                instrument_id=command.instrument_id,
                                size_precision=instrument.size_precision,
                                ts_init=self._clock.timestamp_ns(),
                            )
                            reports.append(report)
                        else:
                            pyo3_reports.extend(response)
                    elif self._config.use_spot_cash_position_reports:
                        # SPOT CASH: Use wallet balance calculation
                        spot_reports = await self._generate_spot_position_reports_from_wallet(
                            command.instrument_id,
                        )
                        reports.extend(spot_reports)
                    else:
                        # SPOT CASH without position reports: Return FLAT
                        report = PositionStatusReport.create_flat(
                            account_id=self.account_id,
                            instrument_id=command.instrument_id,
                            size_precision=instrument.size_precision,
                            ts_init=self._clock.timestamp_ns(),
                        )
                        reports.append(report)
                else:
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
                                f"Cannot create FLAT position report - instrument {command.instrument_id} not found in cache",
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
                    if instrument_type == OKXInstrumentType.SPOT:
                        # SPOT instruments: check margin mode first
                        if self._config.use_spot_margin:
                            # SPOT MARGIN: Use positions API like SWAP/FUTURES (always)
                            response = await self._http_client.request_position_status_reports(
                                account_id=self.pyo3_account_id,
                                instrument_type=OKXInstrumentType.MARGIN,
                            )
                            pyo3_reports.extend(response)
                        elif self._config.use_spot_cash_position_reports:
                            # SPOT CASH: Use wallet balance calculation
                            spot_reports = await self._generate_spot_position_reports_from_wallet()
                            reports.extend(spot_reports)
                        # If neither, skip SPOT entirely (no position reports)
                        continue

                    response = await self._http_client.request_position_status_reports(
                        account_id=self.pyo3_account_id,
                        instrument_type=instrument_type,
                    )
                    pyo3_reports.extend(response)

            for pyo3_report in pyo3_reports:
                report = PositionStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "PositionReports")

        self._log_report_receipt(
            len(reports),
            "PositionReport",
            command.log_receipt_level,
        )

        return reports

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    def _get_trade_mode_for_order(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None,
    ) -> OKXTradeMode:
        if params:
            td_mode = params.get("td_mode")
            if td_mode:
                try:
                    return OKXTradeMode(td_mode)
                except ValueError:
                    self._log.warning(
                        f"Invalid td_mode '{td_mode}', valid modes: 'cash', 'isolated', 'cross', 'spot_isolated'",
                    )

        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.warning(
                f"Instrument {instrument_id} not found in cache, using default trade mode",
            )
            return self._trade_mode

        if isinstance(instrument, CurrencyPair):
            # SPOT trading
            if self._config.use_spot_margin:
                # Use CROSS or ISOLATED margin mode for spot margin trading
                # Note: SPOT_ISOLATED is only available for copy traders
                return (
                    OKXTradeMode.CROSS
                    if self._config.margin_mode == OKXMarginMode.CROSS
                    else OKXTradeMode.ISOLATED
                )
            else:
                return OKXTradeMode.CASH
        else:
            # Derivatives trading
            return (
                OKXTradeMode.CROSS
                if self._config.margin_mode == OKXMarginMode.CROSS
                else OKXTradeMode.ISOLATED
            )

    def _deny_market_order_quantity(self, order: Order, reason: str) -> None:
        self._log.error(
            f"Cannot submit market order {order.client_order_id}: {reason}",
            LogColor.RED,
        )
        self.generate_order_denied(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            reason=reason,
            ts_event=self._clock.timestamp_ns(),
        )

    async def _query_account(self, _command: QueryAccount) -> None:
        await self._update_account_state()

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order: {order}")
            return

        instrument = self._cache.instrument(order.instrument_id)
        is_option = isinstance(instrument, CryptoOption)

        # OKX does not support market orders for options
        if is_option and order.order_type == OrderType.MARKET:
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason="Market orders are not supported for OKX options, use Limit orders instead",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Validate quote quantity for spot margin market orders
        if (
            order.order_type == OrderType.MARKET
            and order.side == OrderSide.BUY
            and instrument
            and isinstance(instrument, CurrencyPair)
            and self._config.use_spot_margin
            and not order.is_quote_quantity
        ):
            self._deny_market_order_quantity(
                order,
                "OKX spot margin MARKET BUY orders require quote-denominated quantities; "
                "resubmit with `quote_quantity=True`",
            )
            return

        # Check if this is a conditional order that needs to go via REST API
        is_conditional = order.order_type in (
            OrderType.STOP_MARKET,
            OrderType.STOP_LIMIT,
            OrderType.MARKET_IF_TOUCHED,
            OrderType.LIMIT_IF_TOUCHED,
            OrderType.TRAILING_STOP_MARKET,
        )

        # OKX trigger/algo orders are not supported for options
        if is_conditional and is_option:
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Trigger/conditional orders ({order.order_type}) are not supported for OKX options",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        if is_conditional:
            await self._submit_algo_order_http(command)
        else:
            await self._submit_order_websocket(command)

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        order_list = command.order_list
        if not order_list.orders:
            self._log.warning("Received SubmitOrderList with empty order list")
            return

        try:
            parent_order, sl_order, tp_order = self._extract_attached_bracket_orders(
                order_list.orders,
            )
            attach_algo_ords = self._build_attach_algo_ords(sl_order, tp_order)
            self._register_attached_oco_binding(parent_order, sl_order, tp_order)

            for order in order_list.orders:
                self.generate_order_submitted(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    ts_event=self._clock.timestamp_ns(),
                )

            await self._submit_regular_order_http(
                order=parent_order,
                params=command.params,
                attach_algo_ords=attach_algo_ords,
            )
        except Exception as e:
            self._clear_attached_oco_binding(
                parent_order.client_order_id if "parent_order" in locals() else None,
            )

            for order in order_list.orders:
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=str(e),
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _submit_regular_order_http(
        self,
        order: Order,
        params: dict[str, Any] | None,
        attach_algo_ords: list[dict[str, str]] | None = None,
    ) -> None:
        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_side = order_side_to_pyo3(order.side)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None
        pyo3_time_in_force = (
            time_in_force_to_pyo3(order.time_in_force) if order.time_in_force else None
        )

        td_mode = self._get_trade_mode_for_order(order.instrument_id, params)

        px_usd = params.get("px_usd") if params else None
        px_vol = params.get("px_vol") if params else None

        response = await self._http_client.place_order(
            trader_id=pyo3_trader_id,
            strategy_id=pyo3_strategy_id,
            instrument_id=pyo3_instrument_id,
            td_mode=td_mode,
            client_order_id=pyo3_client_order_id,
            order_side=pyo3_order_side,
            order_type=pyo3_order_type,
            quantity=pyo3_quantity,
            time_in_force=pyo3_time_in_force,
            price=pyo3_price,
            post_only=order.is_post_only,
            reduce_only=order.is_reduce_only or None,
            quote_quantity=order.is_quote_quantity,
            attach_algo_ords=attach_algo_ords,
            px_usd=str(px_usd) if px_usd is not None else None,
            px_vol=str(px_vol) if px_vol is not None else None,
        )

        if response.get("s_code") and response["s_code"] != "0":
            raise ValueError(f"OKX API error: {response.get('s_msg', 'Unknown error')}")

    async def _submit_regular_order_websocket(
        self,
        order: Order,
        params: dict[str, Any] | None,
        attach_algo_ords: list[dict[str, str]] | None = None,
    ) -> None:
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

        td_mode = self._get_trade_mode_for_order(order.instrument_id, params)

        px_usd = params.get("px_usd") if params else None
        px_vol = params.get("px_vol") if params else None

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
            attach_algo_ords=attach_algo_ords,
            px_usd=str(px_usd) if px_usd is not None else None,
            px_vol=str(px_vol) if px_vol is not None else None,
        )

    async def _submit_order_websocket(self, command: SubmitOrder) -> None:
        order = command.order

        try:
            # Generate OrderSubmitted event here to ensure correct event sequencing
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

            await self._submit_regular_order_websocket(
                order=order,
                params=command.params,
            )
        except Exception as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    def _normalize_close_fraction(self, command: SubmitOrder) -> str | None:
        close_fraction = command.params.get("close_fraction") if command.params else None

        if close_fraction is None:
            return None
        if isinstance(close_fraction, str):
            return close_fraction
        if isinstance(close_fraction, bool):
            raise ValueError("OKX close_fraction must be a str, int, or float, not bool")
        if isinstance(close_fraction, int | float):
            return str(close_fraction)

        raise ValueError(
            f"OKX close_fraction must be a str, int, or float, was {type(close_fraction).__name__}",
        )

    def _extract_attached_bracket_orders(
        self,
        orders: list[Order],
    ) -> tuple[Order, Order | None, Order | None]:
        parent_order = self._extract_attached_bracket_parent(orders)
        child_orders = [
            order for order in orders if order.parent_order_id == parent_order.client_order_id
        ]

        if not child_orders:
            raise ValueError("OKX attached TP/SL requires at least one child protective order")

        sl_order: Order | None = None
        tp_order: Order | None = None

        for child_order in child_orders:
            self._validate_attached_bracket_child(
                parent_order=parent_order,
                child_order=child_order,
            )
            sl_order, tp_order = self._assign_attached_bracket_child(
                child_order=child_order,
                sl_order=sl_order,
                tp_order=tp_order,
            )

        if len(child_orders) != sum(order is not None for order in (sl_order, tp_order)):
            raise ValueError("OKX attached TP/SL bracket contains unsupported child orders")

        return parent_order, sl_order, tp_order

    def _extract_attached_bracket_parent(self, orders: list[Order]) -> Order:
        parent_orders = [order for order in orders if order.parent_order_id is None]
        if len(parent_orders) != 1:
            raise ValueError("OKX attached TP/SL requires exactly one parent entry order")

        parent_order = parent_orders[0]
        if self._is_conditional_order(parent_order):
            raise ValueError("OKX attached TP/SL requires a non-conditional parent order")

        return parent_order

    @staticmethod
    def _validate_attached_bracket_child(
        parent_order: Order,
        child_order: Order,
    ) -> None:
        if child_order.instrument_id != parent_order.instrument_id:
            raise ValueError("OKX attached TP/SL bracket orders must use one instrument")
        if child_order.quantity != parent_order.quantity:
            raise ValueError("OKX attached TP/SL child quantity must match the parent")
        if child_order.side == parent_order.side:
            raise ValueError("OKX attached TP/SL child side must oppose the parent")

    @staticmethod
    def _assign_attached_bracket_child(
        child_order: Order,
        sl_order: Order | None,
        tp_order: Order | None,
    ) -> tuple[Order | None, Order | None]:
        if child_order.order_type in (OrderType.STOP_MARKET, OrderType.STOP_LIMIT):
            if sl_order is not None:
                raise ValueError("OKX attached TP/SL supports only one stop-loss child")
            return child_order, tp_order

        if child_order.order_type in (OrderType.MARKET_IF_TOUCHED, OrderType.LIMIT_IF_TOUCHED):
            if tp_order is not None:
                raise ValueError("OKX attached TP/SL supports only one take-profit child")
            return sl_order, child_order

        raise ValueError(
            f"OKX attached TP/SL does not support child order type {child_order.order_type.name}",
        )

    @staticmethod
    def _okx_trigger_type_str(order: Order) -> str:
        trigger_type = getattr(order, "trigger_type", TriggerType.DEFAULT)
        if trigger_type == TriggerType.MARK_PRICE:
            return "mark"
        if trigger_type == TriggerType.INDEX_PRICE:
            return "index"
        return "last"

    @staticmethod
    def _attached_oco_attach_client_order_id(
        sl_order: Order | None,
        tp_order: Order | None,
    ) -> ClientOrderId | None:
        if sl_order is not None:
            return sl_order.client_order_id
        if tp_order is not None:
            return tp_order.client_order_id
        return None

    def _register_attached_oco_binding(
        self,
        parent_order: Order,
        sl_order: Order | None,
        tp_order: Order | None,
    ) -> None:
        attach_client_order_id = self._attached_oco_attach_client_order_id(sl_order, tp_order)
        if attach_client_order_id is None:
            return

        binding = _OKXAttachedOcoBinding(
            parent_client_order_id=parent_order.client_order_id,
            attach_client_order_id=attach_client_order_id,
            instrument_id=parent_order.instrument_id,
            sl_client_order_id=sl_order.client_order_id if sl_order is not None else None,
            tp_client_order_id=tp_order.client_order_id if tp_order is not None else None,
        )

        for client_order_id in binding.all_client_order_ids():
            self._attached_oco_bindings[client_order_id] = binding

    def _attached_oco_orders_from_cache(
        self,
        order: Order,
    ) -> list[Order]:
        orders: list[Order] = []
        seen_ids: set[ClientOrderId] = set()

        def add_order(candidate_id: ClientOrderId | None) -> None:
            if candidate_id is None or candidate_id in seen_ids:
                return
            candidate = self._cache.order(candidate_id)
            if candidate is None:
                return
            seen_ids.add(candidate_id)
            orders.append(candidate)

        if order.order_list_id is not None:
            order_list = self._cache.order_list(order.order_list_id)
            if order_list is not None and order_list.orders:
                return list(order_list.orders)

        add_order(order.client_order_id)
        if order.parent_order_id is not None:
            add_order(order.parent_order_id)

        for linked_id in order.linked_order_ids or []:
            add_order(linked_id)

        parent_candidate = (
            self._cache.order(order.parent_order_id) if order.parent_order_id else None
        )

        if parent_candidate is not None:
            add_order(parent_candidate.client_order_id)
            for linked_id in parent_candidate.linked_order_ids or []:
                add_order(linked_id)

        return orders

    def _rebuild_attached_oco_binding(
        self,
        client_order_id: ClientOrderId,
    ) -> _OKXAttachedOcoBinding | None:
        order = self._cache.order(client_order_id)
        if order is None:
            return None

        orders = self._attached_oco_orders_from_cache(order)
        if not orders:
            return None

        try:
            parent_order, sl_order, tp_order = self._extract_attached_bracket_orders(orders)
        except ValueError:
            return None

        self._register_attached_oco_binding(parent_order, sl_order, tp_order)
        return self._attached_oco_bindings.get(client_order_id)

    def _attached_oco_binding(
        self,
        client_order_id: ClientOrderId | None,
    ) -> _OKXAttachedOcoBinding | None:
        if client_order_id is None:
            return None
        binding = self._attached_oco_bindings.get(client_order_id)
        if binding is not None:
            return binding
        return self._rebuild_attached_oco_binding(client_order_id)

    def _build_attach_algo_ords(
        self,
        sl_order: Order | None,
        tp_order: Order | None,
    ) -> list[dict[str, str]]:
        attach_algo_ord: dict[str, str] = {}
        attach_client_order_id = self._attached_oco_attach_client_order_id(sl_order, tp_order)
        if sl_order is not None:
            if attach_client_order_id is not None:
                attach_algo_ord["attach_algo_cl_ord_id"] = attach_client_order_id.value
            attach_algo_ord["sl_trigger_px"] = str(sl_order.trigger_price)
            attach_algo_ord["sl_ord_px"] = (
                "-1" if sl_order.order_type == OrderType.STOP_MARKET else str(sl_order.price)
            )
            attach_algo_ord["sl_trigger_px_type"] = self._okx_trigger_type_str(sl_order)

        if tp_order is not None:
            if attach_client_order_id is not None:
                attach_algo_ord.setdefault("attach_algo_cl_ord_id", attach_client_order_id.value)
            attach_algo_ord["tp_trigger_px"] = str(tp_order.trigger_price)
            attach_algo_ord["tp_ord_px"] = (
                "-1" if tp_order.order_type == OrderType.MARKET_IF_TOUCHED else str(tp_order.price)
            )
            attach_algo_ord["tp_trigger_px_type"] = self._okx_trigger_type_str(tp_order)

        return [attach_algo_ord] if attach_algo_ord else []

    async def _submit_algo_order_http(self, command: SubmitOrder) -> None:
        order = command.order

        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_side = order_side_to_pyo3(order.side)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))

        pyo3_trigger_price = (
            nautilus_pyo3.Price.from_str(str(order.trigger_price))
            if order.has_trigger_price
            else None
        )

        pyo3_limit_price = (
            nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None
        )

        pyo3_trigger_type = (
            trigger_type_to_pyo3(order.trigger_type) if hasattr(order, "trigger_type") else None
        )

        callback_ratio = None
        callback_spread = None
        pyo3_activation_price = None

        if order.order_type == OrderType.TRAILING_STOP_MARKET:
            trailing_offset = order.trailing_offset
            trailing_offset_type = order.trailing_offset_type
            if trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
                # Convert basis points to ratio (e.g., 100 bps = 0.01)
                callback_ratio = str(trailing_offset / Decimal(10000))
            elif trailing_offset_type == TrailingOffsetType.PRICE:
                callback_spread = str(trailing_offset)
            else:
                self._log.error(
                    f"Unsupported trailing_offset_type for OKX: {trailing_offset_type}",
                )
                self.generate_order_denied(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=f"unsupported trailing_offset_type: {trailing_offset_type}",
                    ts_event=self._clock.timestamp_ns(),
                )
                return

            if order.activation_price is not None:
                pyo3_activation_price = nautilus_pyo3.Price.from_str(
                    str(order.activation_price),
                )

        td_mode = self._get_trade_mode_for_order(order.instrument_id, command.params)
        close_fraction = self._normalize_close_fraction(command)
        reduce_only = True if close_fraction is not None else (order.is_reduce_only or None)

        try:
            # Generate OrderSubmitted event here to ensure correct event sequencing
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

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
                reduce_only=reduce_only,
                close_fraction=close_fraction,
                callback_ratio=callback_ratio,
                callback_spread=callback_spread,
                activation_price=pyo3_activation_price,
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

    async def _batch_cancel_orders(self, command) -> None:
        regular_orders, algo_orders = self._categorize_orders_for_batch_cancel(command.cancels)

        if regular_orders:
            await self._batch_cancel_regular_orders(regular_orders)

        if algo_orders:
            await self._batch_cancel_algo_orders(algo_orders)

        if not regular_orders and not algo_orders:
            self._log.warning("No valid orders to cancel in batch")

    def _categorize_orders_for_batch_cancel(self, cancels):
        regular_orders = []
        algo_orders: list[tuple[ClientOrderId, InstrumentId, str]] = []

        for cancel in cancels:
            order = self._cache.order(cancel.client_order_id)
            if order is None:
                self._log.warning(f"{cancel.client_order_id!r} not found in cache, skipping")
                continue

            if order.is_closed:
                self._log.debug(
                    f"Order {cancel.client_order_id!r} already {order.status_string()}, skipping",
                )
                continue

            # Pending conditional orders must use HTTP algo cancel
            is_pending_algo = self._is_conditional_order(order) and not self._is_order_triggered(
                order,
            )

            if is_pending_algo:
                algo_id = self._resolve_algo_id(order)
                if algo_id:
                    algo_orders.append((order.client_order_id, cancel.instrument_id, algo_id))
                else:
                    self._log.warning(
                        f"No algo_id for conditional order {order.client_order_id!r}, skipping",
                    )
                continue

            pyo3_inst_id = nautilus_pyo3.InstrumentId.from_str(cancel.instrument_id.value)
            resolved_client_order_id = self._exchange_client_order_id(cancel.client_order_id)
            pyo3_client_order_id = (
                nautilus_pyo3.ClientOrderId(resolved_client_order_id.value)
                if resolved_client_order_id is not None
                else None
            )
            pyo3_venue_order_id = (
                nautilus_pyo3.VenueOrderId(cancel.venue_order_id.value)
                if cancel.venue_order_id
                else None
            )
            regular_orders.append((pyo3_inst_id, pyo3_client_order_id, pyo3_venue_order_id))

        return regular_orders, algo_orders

    async def _batch_cancel_regular_orders(self, orders_to_cancel) -> None:
        try:
            await self._ws_client.batch_cancel_orders(orders_to_cancel)
            self._log.info(f"Submitted batch cancel for {len(orders_to_cancel)} regular orders")
        except Exception as e:
            self._log.error(f"Failed to batch cancel regular orders: {e}")

    async def _batch_cancel_algo_orders(
        self,
        algo_orders: list[tuple[ClientOrderId, InstrumentId, str]],
    ) -> None:
        regular_algos = []
        regular_client_order_ids = []
        advance_algos = []
        advance_client_order_ids = []

        for client_order_id, inst_id, algo_id in algo_orders:
            order = self._cache.order(client_order_id)
            is_advance = order is not None and order.order_type == OrderType.TRAILING_STOP_MARKET
            pyo3_pair = (nautilus_pyo3.InstrumentId.from_str(inst_id.value), algo_id)

            if is_advance:
                advance_algos.append(pyo3_pair)
                advance_client_order_ids.append(client_order_id)
            else:
                regular_algos.append(pyo3_pair)
                regular_client_order_ids.append(client_order_id)

        cancelled_client_order_ids = []

        try:
            if regular_algos:
                await self._http_client.cancel_algo_orders(regular_algos)
                self._log.info(f"Submitted batch cancel for {len(regular_algos)} algo orders")
                cancelled_client_order_ids.extend(regular_client_order_ids)
        except Exception as e:
            self._log.error(f"Failed to batch cancel algo orders: {e}")

        # Advance algo orders must be cancelled individually
        for i, (pyo3_inst_id, algo_id) in enumerate(advance_algos):
            try:
                resp = await self._http_client.cancel_advance_algo_order(
                    instrument_id=pyo3_inst_id,
                    algo_id=algo_id,
                )
                s_code = resp.get("s_code", "0")
                if s_code != "0":
                    s_msg = resp.get("s_msg", "unknown")
                    self._log.warning(
                        f"OKX rejected cancel for advance algo {algo_id}: "
                        f"s_code={s_code}, s_msg={s_msg}",
                    )
                else:
                    cancelled_client_order_ids.append(advance_client_order_ids[i])
            except Exception as e:
                self._log.warning(f"Failed to cancel advance algo order {algo_id}: {e}")

        for client_order_id in cancelled_client_order_ids:
            self._clear_attached_oco_binding(client_order_id)
            self._algo_order_ids.pop(client_order_id, None)
            self._algo_order_instruments.pop(client_order_id, None)

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

        # Pending conditional orders use HTTP algo amend,
        # triggered conditional orders become regular on OKX
        is_pending_algo = self._is_conditional_order(order) and not self._is_order_triggered(
            order,
        )

        if is_pending_algo:
            await self._modify_algo_order_http(command, order)
        else:
            await self._modify_order_websocket(command, order)

    async def _modify_algo_order_http(self, command: ModifyOrder, order: Order) -> None:
        algo_id = self._resolve_algo_id(order)
        if not algo_id:
            self._log.error(
                f"Cannot amend pending algo order {command.client_order_id!r}: "
                "no algo_id resolved from mapping or venue_order_id",
            )
            self.generate_order_modify_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason="no algo_id available for pending conditional order",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
            command.instrument_id.value,
        )
        new_trigger_price = (
            nautilus_pyo3.Price.from_str(str(command.trigger_price))
            if command.trigger_price
            else None
        )
        new_limit_price = (
            nautilus_pyo3.Price.from_str(str(command.price)) if command.price else None
        )
        new_quantity = (
            nautilus_pyo3.Quantity.from_str(str(command.quantity)) if command.quantity else None
        )

        self._log.debug(
            f"Amending OKX algo order using algo_id {algo_id} for {command.client_order_id!r}",
        )

        try:
            resp = await self._http_client.amend_algo_order(
                instrument_id=pyo3_instrument_id,
                algo_id=algo_id,
                new_trigger_price=new_trigger_price,
                new_limit_price=new_limit_price,
                new_quantity=new_quantity,
            )

            s_code = resp.get("s_code", "0")
            if s_code != "0":
                s_msg = resp.get("s_msg", "unknown")
                reason = f"s_code={s_code}, s_msg={s_msg}"
                self._log.error(
                    f"OKX rejected amend for algo order {algo_id}: {reason}",
                )
                self.generate_order_modify_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=reason,
                    ts_event=self._clock.timestamp_ns(),
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

    async def _modify_order_websocket(self, command: ModifyOrder, order: Order) -> None:
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

        new_px_usd = command.params.get("px_usd") if command.params else None
        new_px_vol = command.params.get("px_vol") if command.params else None

        try:
            await self._ws_client.modify_order(
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
                price=pyo3_price,
                quantity=pyo3_quantity,
                client_order_id=pyo3_client_order_id,
                venue_order_id=pyo3_venue_order_id,
                new_px_usd=str(new_px_usd) if new_px_usd is not None else None,
                new_px_vol=str(new_px_vol) if new_px_vol is not None else None,
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

            # Pending conditional orders use HTTP algo cancel,
            # triggered conditional orders become regular on OKX
            is_pending_algo = self._is_conditional_order(order) and not self._is_order_triggered(
                order,
            )
            algo_id = self._resolve_algo_id(order) if is_pending_algo else None

            if is_pending_algo and not algo_id:
                self._log.error(
                    f"Cannot cancel pending algo order {command.client_order_id!r}: "
                    "no algo_id resolved from mapping or venue_order_id",
                )
                self.generate_order_cancel_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason="no algo_id available for pending conditional order",
                    ts_event=self._clock.timestamp_ns(),
                )
                return

            if algo_id:
                self._log.debug(
                    f"Cancelling OKX algo order using algo_id {algo_id} "
                    f"for canonical {alias_lookup_key!r} (requested {command.client_order_id!r})",
                )
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

                try:
                    if order.order_type == OrderType.TRAILING_STOP_MARKET:
                        resp = await self._http_client.cancel_advance_algo_order(
                            instrument_id=pyo3_instrument_id,
                            algo_id=algo_id,
                        )
                    else:
                        resp = await self._http_client.cancel_algo_order(
                            instrument_id=pyo3_instrument_id,
                            algo_id=algo_id,
                        )

                    s_code = resp.get("s_code", "0")
                    if s_code != "0":
                        s_msg = resp.get("s_msg", "unknown")
                        reason = f"s_code={s_code}, s_msg={s_msg}"
                        self._log.error(
                            f"OKX rejected cancel for algo order {algo_id}: {reason}",
                        )
                        self.generate_order_cancel_rejected(
                            strategy_id=order.strategy_id,
                            instrument_id=order.instrument_id,
                            client_order_id=order.client_order_id,
                            venue_order_id=order.venue_order_id,
                            reason=reason,
                            ts_event=self._clock.timestamp_ns(),
                        )
                        return
                except ValueError as e:
                    message = str(e)
                    alias_text = str(alias_lookup_key) if alias_lookup_key is not None else ""
                    client_text = str(command.client_order_id) if command.client_order_id else ""

                    if (
                        "already canceled" not in message
                        and algo_id not in message
                        and alias_text not in message
                        and client_text not in message
                    ):
                        raise

                self._clear_attached_oco_binding(command.client_order_id)
                self._algo_order_ids.pop(alias_lookup_key, None)
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
        if command.order_side != OrderSide.NO_ORDER_SIDE:
            self._log.warning(
                f"OKX does not support order_side filtering for cancel all orders; "
                f"ignoring order_side={order_side_to_str(command.order_side)} and canceling all orders",
            )

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
            order = self._cache.order(client_order_id)
            is_advance = order is not None and order.order_type == OrderType.TRAILING_STOP_MARKET

            if is_advance:
                resp = await self._http_client.cancel_advance_algo_order(
                    instrument_id=pyo3_instrument_id,
                    algo_id=algo_id,
                )
            else:
                resp = await self._http_client.cancel_algo_order(
                    instrument_id=pyo3_instrument_id,
                    algo_id=algo_id,
                )

            s_code = resp.get("s_code", "0")
            if s_code != "0":
                s_msg = resp.get("s_msg", "unknown")
                self._log.warning(
                    f"OKX rejected fallback cancel for algo {algo_id}: "
                    f"s_code={s_code}, s_msg={s_msg}",
                )
                return

            self._log.debug(
                f"Successfully cancelled OKX algo order {client_order_id!r} via fallback",
            )
        except Exception as e:
            self._log.warning(
                f"Failed fallback cancel for OKX algo order {client_order_id!r} (algo_id={algo_id}): {e}",
            )
        finally:
            self._clear_attached_oco_binding(client_order_id)
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
        regular_cancels: list[CancelOrder] = []
        algo_cancels: list[tuple[ClientOrderId, InstrumentId, str]] = []

        for order in orders_open:
            if order.is_closed:
                continue

            # Pending conditional orders must use HTTP algo cancel
            is_pending_algo = self._is_conditional_order(order) and not self._is_order_triggered(
                order,
            )

            if is_pending_algo:
                algo_id = self._resolve_algo_id(order)
                if algo_id:
                    algo_cancels.append((order.client_order_id, order.instrument_id, algo_id))
                else:
                    self._log.warning(
                        f"No algo_id for conditional order {order.client_order_id!r}, skipping",
                    )
            else:
                regular_cancels.append(
                    CancelOrder(
                        trader_id=command.trader_id,
                        strategy_id=command.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=order.venue_order_id,
                        command_id=command.id,
                        ts_init=command.ts_init,
                    ),
                )

        self._log.debug(
            f"Canceling {len(regular_cancels)} regular orders and "
            f"{len(algo_cancels)} algo orders for {command.instrument_id}",
        )

        # Process regular cancels in batches of 20 (OKX API limit)
        batch_size = 20

        for i in range(0, len(regular_cancels), batch_size):
            batch = regular_cancels[i : i + batch_size]
            batch_command = BatchCancelOrders(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                cancels=batch,
                command_id=command.id,
                ts_init=command.ts_init,
            )
            await self._batch_cancel_orders(batch_command)

        for client_order_id, instrument_id, algo_id in algo_cancels:
            await self._cancel_algo_order_fallback(
                client_order_id=client_order_id,
                instrument_id=instrument_id,
                algo_id=algo_id,
            )

    # -- HELPERS ----------------------------------------------------------------------------------

    _OKX_CONDITIONAL_ORDER_TYPES = frozenset(
        {
            OrderType.STOP_MARKET,
            OrderType.STOP_LIMIT,
            OrderType.MARKET_IF_TOUCHED,
            OrderType.LIMIT_IF_TOUCHED,
            OrderType.TRAILING_STOP_MARKET,
        },
    )

    def _is_conditional_order(self, order: Order) -> bool:
        return order.order_type in self._OKX_CONDITIONAL_ORDER_TYPES

    @staticmethod
    def _is_order_triggered(order: Order) -> bool:
        # Prefer the sticky is_triggered flag (StopLimit, LimitIfTouched,
        # TrailingStopLimit), fall back to filled_qty for market-type stops
        # that don't expose the attribute
        if hasattr(order, "is_triggered"):
            return order.is_triggered
        return order.filled_qty > 0 or order.status == OrderStatus.TRIGGERED

    def _resolve_algo_id(self, order: Order) -> str | None:
        """
        Resolve the OKX algo_id for an order.

        Only uses the `_algo_order_ids` mapping. After a conditional order
        triggers, its venue_order_id becomes the child ordId (not the
        algo_id), so venue_order_id must not be used as a fallback.

        """
        binding = self._attached_oco_binding(order.client_order_id)
        if binding is not None:
            for candidate in binding.all_client_order_ids():
                algo_id = self._algo_order_ids.get(candidate)
                if algo_id is not None:
                    return algo_id

        canonical = self._canonical_client_order_id(order.client_order_id)
        key = canonical or order.client_order_id
        return self._algo_order_ids.get(key)

    @staticmethod
    def _attached_oco_fanout_statuses() -> frozenset[OrderStatus]:
        return frozenset(
            {
                OrderStatus.ACCEPTED,
                OrderStatus.REJECTED,
                OrderStatus.CANCELED,
                OrderStatus.EXPIRED,
            },
        )

    def _augment_attached_oco_parent_report(self, report: OrderStatusReport) -> None:
        binding = self._attached_oco_binding(report.client_order_id)
        if binding is None or report.client_order_id != binding.parent_client_order_id:
            return

        linked_order_ids = list(report.linked_order_ids or [])
        for child_id in binding.child_client_order_ids():
            if child_id not in linked_order_ids:
                linked_order_ids.append(child_id)

        report.linked_order_ids = linked_order_ids or None

    def _build_attached_oco_child_report(
        self,
        venue_report: OrderStatusReport,
        child_order: Order,
        linked_order_ids: list[ClientOrderId],
        *,
        is_originating_child: bool,
    ) -> OrderStatusReport:
        values = venue_report.to_dict()
        values["client_order_id"] = child_order.client_order_id.value
        values["order_list_id"] = (
            child_order.order_list_id.value if child_order.order_list_id is not None else None
        )
        values["linked_order_ids"] = [
            client_order_id.value for client_order_id in linked_order_ids
        ] or None
        values["parent_order_id"] = (
            child_order.parent_order_id.value if child_order.parent_order_id is not None else None
        )
        values["contingency_type"] = child_order.contingency_type.value
        values["order_side"] = child_order.side.value
        values["order_type"] = child_order.order_type.value
        values["time_in_force"] = child_order.time_in_force.value
        values["quantity"] = str(child_order.quantity)
        values["price"] = str(child_order.price) if child_order.has_price else None
        values["trigger_price"] = (
            str(child_order.trigger_price) if child_order.has_trigger_price else None
        )
        values["trigger_type"] = (
            child_order.trigger_type.value
            if child_order.has_trigger_price
            else TriggerType.NO_TRIGGER.value
        )
        values["reduce_only"] = child_order.is_reduce_only

        if not is_originating_child:
            values["filled_qty"] = "0"
            values["avg_px"] = None
            values["ts_triggered"] = None

        return OrderStatusReport.from_dict(values)

    def _expand_attached_oco_child_reports(
        self,
        venue_report: OrderStatusReport,
    ) -> list[OrderStatusReport]:
        binding = self._attached_oco_binding(venue_report.client_order_id)
        if binding is None or venue_report.client_order_id not in binding.child_client_order_ids():
            return [venue_report]

        child_client_order_ids = binding.child_client_order_ids()
        if len(child_client_order_ids) <= 1:
            linked_order_ids = [
                client_order_id
                for client_order_id in child_client_order_ids
                if client_order_id != venue_report.client_order_id
            ]
            order = self._cache.order(venue_report.client_order_id)
            if order is None:
                return [venue_report]
            return [
                self._build_attached_oco_child_report(
                    venue_report=venue_report,
                    child_order=order,
                    linked_order_ids=linked_order_ids,
                    is_originating_child=True,
                ),
            ]

        if venue_report.order_status not in self._attached_oco_fanout_statuses():
            return [venue_report]

        reports: list[OrderStatusReport] = []

        for child_client_order_id in child_client_order_ids:
            child_order = self._cache.order(child_client_order_id)
            if child_order is None or not self._is_conditional_order(child_order):
                continue

            linked_order_ids = [
                client_order_id
                for client_order_id in child_client_order_ids
                if client_order_id != child_client_order_id
            ]
            reports.append(
                self._build_attached_oco_child_report(
                    venue_report=venue_report,
                    child_order=child_order,
                    linked_order_ids=linked_order_ids,
                    is_originating_child=child_client_order_id == venue_report.client_order_id,
                ),
            )

        return reports or [venue_report]

    def _is_attached_oco_child_report(self, report: OrderStatusReport) -> bool:
        binding = self._attached_oco_binding(report.client_order_id)
        return binding is not None and report.client_order_id in binding.child_client_order_ids()

    def _hydrate_zero_quantity_algo_report(
        self,
        pyo3_report: nautilus_pyo3.OrderStatusReport,
    ) -> nautilus_pyo3.OrderStatusReport:
        if Decimal(str(pyo3_report.quantity)) != 0:
            return pyo3_report

        if pyo3_report.client_order_id is None:
            return pyo3_report

        report_client_order_id = ClientOrderId(pyo3_report.client_order_id.value)
        canonical_client_order_id = (
            self._canonical_client_order_id(report_client_order_id) or report_client_order_id
        )
        order = self._cache.order(canonical_client_order_id)
        if order is None or not self._is_conditional_order(order):
            return pyo3_report

        self._log.debug(
            f"Hydrating zero-quantity OKX algo report for {canonical_client_order_id!r} "
            f"from cached quantity {order.quantity}",
        )

        return nautilus_pyo3.OrderStatusReport(
            account_id=pyo3_report.account_id,
            instrument_id=pyo3_report.instrument_id,
            venue_order_id=pyo3_report.venue_order_id,
            client_order_id=pyo3_report.client_order_id,
            order_side=pyo3_report.order_side,
            order_type=pyo3_report.order_type,
            time_in_force=pyo3_report.time_in_force,
            order_status=pyo3_report.order_status,
            quantity=nautilus_pyo3.Quantity.from_str(str(order.quantity)),
            filled_qty=pyo3_report.filled_qty,
            report_id=pyo3_report.report_id,
            ts_accepted=pyo3_report.ts_accepted,
            ts_last=pyo3_report.ts_last,
            ts_init=pyo3_report.ts_init,
            order_list_id=pyo3_report.order_list_id,
            venue_position_id=pyo3_report.venue_position_id,
            linked_order_ids=pyo3_report.linked_order_ids or None,
            parent_order_id=pyo3_report.parent_order_id,
            contingency_type=pyo3_report.contingency_type,
            expire_time=pyo3_report.expire_time,
            price=pyo3_report.price,
            trigger_price=pyo3_report.trigger_price,
            trigger_type=pyo3_report.trigger_type,
            limit_offset=pyo3_report.limit_offset,
            trailing_offset=pyo3_report.trailing_offset,
            trailing_offset_type=pyo3_report.trailing_offset_type,
            avg_px=pyo3_report.avg_px,
            display_qty=pyo3_report.display_qty,
            post_only=pyo3_report.post_only,
            reduce_only=pyo3_report.reduce_only,
            cancel_reason=pyo3_report.cancel_reason,
            ts_triggered=pyo3_report.ts_triggered,
        )

    # -- WEBSOCKET HANDLERS -----------------------------------------------------------------------

    def _is_external_order(self, client_order_id: ClientOrderId) -> bool:
        return not client_order_id or not self._cache.strategy_id_for_order(client_order_id)

    def _handle_msg(self, msg: Any) -> None:  # noqa: C901 (too complex)
        if isinstance(msg, nautilus_pyo3.OKXWebSocketError):
            self._log.error(repr(msg))
            return

        try:
            if isinstance(msg, nautilus_pyo3.AccountState):
                self._handle_account_state(msg)
            elif isinstance(msg, nautilus_pyo3.OrderAccepted):
                self._handle_order_accepted_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderCanceled):
                self._handle_order_canceled_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderExpired):
                self._handle_order_expired_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderRejected):
                self._handle_order_rejected_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderCancelRejected):
                self._handle_order_cancel_rejected_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderModifyRejected):
                self._handle_order_modify_rejected_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderTriggered):
                self._handle_order_triggered_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderUpdated):
                self._handle_order_updated_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderStatusReport):
                self._handle_order_status_report_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.FillReport):
                self._handle_fill_report_pyo3(msg)
            else:
                self._log.debug(f"Received unhandled message type: {type(msg)}")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)

    def _handle_instrument_update(self, pyo3_instrument: OkxInstrument) -> None:
        self._http_client.cache_instrument(pyo3_instrument)  # type: ignore [arg-type]

        if self._ws_client is not None:
            self._ws_client.cache_instrument(pyo3_instrument)  # type: ignore [arg-type]

        if self._ws_business_client is not None:
            self._ws_business_client.cache_instrument(pyo3_instrument)  # type: ignore [arg-type]

    def _handle_account_state(self, pyo3_account_state: nautilus_pyo3.AccountState) -> None:
        account_state = AccountState.from_dict(pyo3_account_state.to_dict())
        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=account_state.is_reported,
            ts_event=account_state.ts_event,
        )

    def _handle_order_accepted_pyo3(self, pyo3_event: nautilus_pyo3.OrderAccepted) -> None:
        event = OrderAccepted.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)

    def _handle_order_canceled_pyo3(self, pyo3_event: nautilus_pyo3.OrderCanceled) -> None:
        event = OrderCanceled.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)
        self._clear_order_state(event.client_order_id)

    def _handle_order_expired_pyo3(self, pyo3_event: nautilus_pyo3.OrderExpired) -> None:
        event = OrderExpired.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)
        self._clear_order_state(event.client_order_id)

    def _handle_order_rejected_pyo3(self, pyo3_event: nautilus_pyo3.OrderRejected) -> None:
        event = OrderRejected.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)
        self._clear_order_state(event.client_order_id)

    def _handle_order_triggered_pyo3(self, pyo3_event: nautilus_pyo3.OrderTriggered) -> None:
        event = OrderTriggered.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)

    def _handle_order_updated_pyo3(self, pyo3_event: nautilus_pyo3.OrderUpdated) -> None:
        event = OrderUpdated.from_dict(pyo3_event.to_dict())
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

    def _handle_order_status_report_pyo3(
        self,
        pyo3_report: nautilus_pyo3.OrderStatusReport,
    ) -> None:
        self._log.debug(
            f"Received order status report: {pyo3_report.client_order_id!r}, "
            f"status={pyo3_report.order_status}, is_connected={self.is_connected}",
            LogColor.MAGENTA,
        )

        # Discard order status reports until account is properly initialized
        # Reconciliation will handle getting the current state of open orders
        if not self.is_connected or not self.account_id or not self._cache.account(self.account_id):
            self._log.debug(
                f"Discarding order status report during connection sequence: {pyo3_report.client_order_id!r}",
            )
            return

        pyo3_report = self._hydrate_zero_quantity_algo_report(pyo3_report)
        report = OrderStatusReport.from_pyo3(pyo3_report)

        if self._is_attached_oco_child_report(report):
            reports = self._expand_attached_oco_child_reports(report)
        else:
            self._apply_client_order_alias(report)
            self._augment_attached_oco_parent_report(report)
            reports = [report]

        for normalized_report in reports:
            if self._is_external_order(normalized_report.client_order_id):
                self._send_order_status_report(normalized_report)
                continue

            self._handle_internal_order_status_report(normalized_report)

    def _handle_internal_order_status_report(  # noqa: C901 (too complex)
        self,
        report: OrderStatusReport,
    ) -> None:
        order = self._cache.order(report.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process order status report - order for {report.client_order_id!r} not found",
            )
            return

        if order.is_closed:
            return

        binding = self._attached_oco_binding(report.client_order_id)
        is_attached_oco_child = (
            binding is not None and report.client_order_id in binding.child_client_order_ids()
        )
        canonical_client_order_id = (
            report.client_order_id
            if is_attached_oco_child
            else (self._canonical_client_order_id(report.client_order_id) or report.client_order_id)
        )
        algo_id_for_client = self._algo_order_ids.get(canonical_client_order_id)

        if self._is_conditional_order(order):
            child = (
                None
                if is_attached_oco_child
                else self._client_id_children.get(report.client_order_id)
            )
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
                if is_attached_oco_child and binding is not None:
                    for child_client_order_id in binding.child_client_order_ids():
                        self._algo_order_ids[child_client_order_id] = str(report.venue_order_id)
                        self._algo_order_instruments[child_client_order_id] = order.instrument_id
                else:
                    self._algo_order_ids[canonical_client_order_id] = str(report.venue_order_id)
                    self._algo_order_instruments[canonical_client_order_id] = order.instrument_id

        if report.order_status == OrderStatus.REJECTED:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                reason=report.cancel_reason or "",
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
                    price=report.price,
                    trigger_price=report.trigger_price,
                    ts_event=report.ts_last,
                    venue_order_id_modified=True,
                )
                self._algo_order_ids.pop(canonical_client_order_id, None)
                self._algo_order_instruments.pop(canonical_client_order_id, None)
                return

            if venue_is_original_algo:
                return

            if report.is_order_updated(order):
                self.generate_order_updated(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    quantity=report.quantity or order.quantity,
                    price=report.price,
                    trigger_price=report.trigger_price,
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
                    price=order.price if order.has_price else None,
                    trigger_price=order.trigger_price if order.has_trigger_price else None,
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

        net_last_qty = report.last_qty

        # For SPOT margin MARKET BUY orders, adjust ALL fills for commission
        # Commission is deducted from the base currency
        is_spot_margin_market_buy = (
            order.order_type == OrderType.MARKET
            and order.side == OrderSide.BUY
            and self._config.use_spot_margin
            and isinstance(instrument, CurrencyPair)
        )

        if is_spot_margin_market_buy and report.commission.currency == instrument.base_currency:
            net_qty = report.last_qty.as_decimal() - report.commission.as_decimal()
            net_last_qty = Quantity(net_qty, precision=instrument.size_precision)

        # Generate OrderUpdated only on first fill for quote quantity orders
        if order.is_quote_quantity:
            venue_id_changed = (
                order.venue_order_id is not None
                and report.venue_order_id is not None
                and order.venue_order_id != report.venue_order_id
            )

            if venue_id_changed:
                self._cache.add_venue_order_id(
                    client_order_id=order.client_order_id,
                    venue_order_id=report.venue_order_id,
                    overwrite=True,
                )
            self.generate_order_updated(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=report.venue_order_id,
                quantity=net_last_qty,
                price=order.price if order.has_price else None,
                trigger_price=order.trigger_price if order.has_trigger_price else None,
                ts_event=report.ts_event,
                venue_order_id_modified=venue_id_changed,
                is_quote_quantity=False,
            )
        elif (
            order.venue_order_id is not None
            and report.venue_order_id is not None
            and order.venue_order_id != report.venue_order_id
        ):
            self._cache.add_venue_order_id(
                client_order_id=order.client_order_id,
                venue_order_id=report.venue_order_id,
                overwrite=True,
            )
            self.generate_order_updated(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=report.venue_order_id,
                quantity=order.quantity,
                price=order.price if order.has_price else None,
                trigger_price=order.trigger_price if order.has_trigger_price else None,
                ts_event=report.ts_event,
                venue_order_id_modified=True,
            )

        self.generate_order_filled(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=report.venue_order_id,
            venue_position_id=report.venue_position_id,
            trade_id=report.trade_id,
            order_side=order.side,
            order_type=order.order_type,
            last_qty=net_last_qty,
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

    def _clear_attached_oco_binding(
        self,
        client_order_id: ClientOrderId | None,
    ) -> None:
        binding = self._attached_oco_binding(client_order_id)
        if binding is None:
            return

        for identifier in binding.all_client_order_ids():
            self._attached_oco_bindings.pop(identifier, None)
            self._algo_order_ids.pop(identifier, None)
            self._algo_order_instruments.pop(identifier, None)

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
            self._clear_attached_oco_binding(identifier)
            self._client_id_aliases.pop(identifier, None)

            for key, value in list(self._client_id_aliases.items()):
                if value == identifier:
                    self._client_id_aliases.pop(key, None)

            canonical = self._client_id_children.pop(identifier, None)
            if canonical is not None and canonical != identifier:
                self._client_id_aliases.pop(canonical, None)

            self._algo_order_ids.pop(identifier, None)
            self._algo_order_instruments.pop(identifier, None)

    def _clear_order_state(self, client_order_id: ClientOrderId) -> None:
        canonical = self._canonical_client_order_id(client_order_id) or client_order_id
        self._clear_attached_oco_binding(client_order_id)
        self._algo_order_ids.pop(canonical, None)
        self._algo_order_instruments.pop(canonical, None)

        self._client_id_aliases.pop(canonical, None)
        for key, value in list(self._client_id_aliases.items()):
            if value == canonical:
                self._client_id_aliases.pop(key, None)

        self._client_id_children.pop(canonical, None)
