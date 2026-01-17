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
import json
import time
from collections import defaultdict
from collections import deque
from collections.abc import Coroutine
from typing import Any

import msgspec
from py_clob_client.client import BalanceAllowanceParams
from py_clob_client.client import ClobClient
from py_clob_client.client import MarketOrderArgs
from py_clob_client.client import OpenOrderParams
from py_clob_client.client import OrderArgs
from py_clob_client.client import PartialCreateOrderOptions
from py_clob_client.client import TradeParams
from py_clob_client.clob_types import AssetType
from py_clob_client.clob_types import PostOrdersArgs
from py_clob_client.exceptions import PolyApiException
from py_clob_rust_adaptor import PyClobClient

from nautilus_trader.adapters.polymarket.common.cache import get_polymarket_trades_key
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_INVALID_API_KEY
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MIN_MAX_PRICES
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.constants import VALID_POLYMARKET_TIME_IN_FORCE
from nautilus_trader.adapters.polymarket.common.conversion import usdce_from_units
from nautilus_trader.adapters.polymarket.common.credentials import PolymarketWebSocketAuth
from nautilus_trader.adapters.polymarket.common.enums import PolymarketEventType
from nautilus_trader.adapters.polymarket.common.enums import PolymarketTradeStatus
from nautilus_trader.adapters.polymarket.common.parsing import validate_ethereum_address
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
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import nanos_to_secs
from nautilus_trader.core.stats import basis_points_as_percentage
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.live.retry import RetryManagerPool
from nautilus_trader.model.currencies import USDC_POS
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order


class SignedOrderWrapper:
    """Wrapper for signed order data that provides a dict() method for compatibility."""

    def __init__(self, data):
        self._data = data

    def dict(self):
        return self._data


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
    rust_client : RustClobClient, optional
        The optional Rust CLOB client for high-performance order signing and placement.

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
        rust_client: Any | None = None,
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
        self._log.info(f"{config.signature_type=}", LogColor.BLUE)
        self._log.info(f"{config.funder=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)
        self._log.info(f"{config.generate_order_history_from_trades=}", LogColor.BLUE)
        self._log.info(f"{config.log_raw_ws_messages=}", LogColor.BLUE)
        self._log.info(f"{config.ack_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.use_rust=}", LogColor.BLUE)

        # Rust client for order signing/placement (optional)
        self._rust_client = rust_client
        if self._rust_client is not None:
            self._log.info("Using Rust client for order signing and placement", LogColor.GREEN)

        account_id = AccountId(f"{name or POLYMARKET_VENUE.value}-001")
        self._set_account_id(account_id)
        self._log.info(f"account_id={account_id.value}", LogColor.BLUE)

        wallet_address = http_client.get_address()
        if wallet_address is None:
            raise RuntimeError("Auth error: could not determine `wallet_address`")

        validate_ethereum_address(wallet_address)
        self._wallet_address = wallet_address
        self._api_key = http_client.creds.api_key
        self._log.info(f"{wallet_address=}", LogColor.BLUE)

        # HTTP API
        self._http_client = http_client
        self._retry_manager_pool = RetryManagerPool[None](
            pool_size=100,
            max_retries=config.max_retries or 0,
            delay_initial_ms=config.retry_delay_initial_ms or 1_000,
            delay_max_ms=config.retry_delay_max_ms or 10_000,
            backoff_factor=2,
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
        self._processed_trades: deque[TradeId] = deque(maxlen=10_000)
        self._ack_events_order: dict[VenueOrderId, asyncio.Event] = {}
        self._ack_events_trade: dict[VenueOrderId, asyncio.Event] = {}

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()

        # Set up initial active markets
        instruments = self._cache.instruments(venue=POLYMARKET_VENUE)
        for instrument in instruments:
            await self._maintain_active_market(instrument.id)

        try:
            if self._ws_client.is_disconnected():
                await self._ws_client.connect()

            await self._update_account_state()
            await self._await_account_registered()
        except PolyApiException as e:
            self._log.error(repr(e))
            if e.error_msg["error"] == POLYMARKET_INVALID_API_KEY:
                await self._ws_client.disconnect()
            raise e

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

        if self._ws_client.is_connected():
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

    async def _update_account_state(self) -> None:
        self._log.info("Checking account balance")

        params = BalanceAllowanceParams(
            asset_type=AssetType.COLLATERAL,
            signature_type=self._config.signature_type,
        )
        response: dict[str, Any] = await asyncio.to_thread(
            self._http_client.get_balance_allowance,
            params,
        )
        total = usdce_from_units(int(response["balance"]))
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

    async def generate_order_status_reports(  # noqa: C901
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        self._log.debug("Requesting OrderStatusReports...")
        reports: list[OrderStatusReport] = []

        if command.instrument_id is not None:
            condition_id = get_polymarket_condition_id(command.instrument_id)
            asset_id = get_polymarket_token_id(command.instrument_id)
            params = OpenOrderParams(market=condition_id, asset_id=asset_id)
        else:
            params = None

        # Check active orders with venue
        # Note: py_clob_client.get_orders() handles pagination internally
        retry_manager = await self._retry_manager_pool.acquire()
        try:
            response: list[JSON] | None = await retry_manager.run(
                "generate_order_status_reports",
                [command.instrument_id],
                asyncio.to_thread,
                self._http_client.get_orders,
                params=params,
            )
            if response:
                # Uncomment for development
                # self._log.info(f"Processing {len(response)} orders", LogColor.MAGENTA)
                for json_obj in response:
                    raw = msgspec.json.encode(json_obj)
                    polymarket_order = self._decoder_order_report.decode(raw)

                    instrument_id = get_polymarket_instrument_id(
                        polymarket_order.market,
                        polymarket_order.asset_id,
                    )
                    instrument = self._cache.instrument(instrument_id)
                    if instrument is None:
                        self._log.warning(
                            f"Cannot handle order report: instrument {instrument_id} not found "
                            f"(market={polymarket_order.market}, asset_id={polymarket_order.asset_id})",
                        )
                        continue

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
        finally:
            await self._retry_manager_pool.release(retry_manager)

        if self._config.generate_order_history_from_trades:
            self._log.warning(
                "Experimental feature not currently recommended: generating order history from trades",
            )
            reported_client_order_ids: set[ClientOrderId] = {r.client_order_id for r in reports}
            for order in self._cache.orders_open(venue=POLYMARKET_VENUE):
                if order.client_order_id in reported_client_order_ids:
                    continue  # Already reported

                order_status_command = GenerateOrderStatusReport(
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                )
                maybe_report = await self.generate_order_status_report(order_status_command)
                if maybe_report:
                    reports.append(maybe_report)

            known_venue_order_ids: set[VenueOrderId] = {
                o.venue_order_id for o in self._cache.orders()
            }
            known_venue_order_ids.update({r.venue_order_id for r in reports})

            # Check fills to generate order reports
            fill_command = GenerateFillReports(
                instrument_id=command.instrument_id,
                venue_order_id=None,
                start=None,
                end=None,
                command_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )
            fill_reports = await self.generate_fill_reports(fill_command)
            if fill_reports and not known_venue_order_ids:
                self._log.warning(
                    "No previously known venue order IDs found in cache or from active orders",
                )

            venue_order_id_fill_reports: dict[VenueOrderId, list[FillReport]] = defaultdict(list)
            for fill in fill_reports:
                if fill.venue_order_id in known_venue_order_ids:
                    continue  # Already reported
                venue_order_id_fill_reports[fill.venue_order_id].append(fill)

            for venue_order_id, fill_reports in venue_order_id_fill_reports.items():
                first_fill = fill_reports[0]
                instrument = self._cache.instrument(first_fill.instrument_id)
                if instrument is None:
                    self._log.warning(
                        f"Cannot handle order report: instrument {first_fill.instrument_id} not found "
                        f"(venue_order_id={venue_order_id})",
                    )
                    continue

                order_type = (
                    OrderType.MARKET
                    if first_fill.liquidity_side == LiquiditySide.TAKER
                    else OrderType.LIMIT
                )

                if order_type == OrderType.LIMIT:
                    price = first_fill.last_px
                else:
                    price = None

                order_side = first_fill.order_side

                avg_px: float = 0.0
                filled_qty: float = 0.0
                ts_last: int = first_fill.ts_event

                for fill_report in fill_reports:
                    avg_px += float(fill_report.last_px) * float(fill_report.last_qty)
                    filled_qty += float(fill_report.last_qty)
                    ts_last = fill_report.ts_event

                if filled_qty > 0:
                    avg_px /= filled_qty
                else:
                    avg_px = 0.0

                self._log.warning(f"{venue_order_id=}")
                self._log.warning(f"{avg_px=}")
                self._log.warning(f"{filled_qty=}")

                report = OrderStatusReport(
                    account_id=first_fill.account_id,
                    instrument_id=first_fill.instrument_id,
                    client_order_id=ClientOrderId(str(UUID4())),
                    order_list_id=None,
                    venue_order_id=venue_order_id,
                    order_side=order_side,
                    order_type=order_type,
                    contingency_type=ContingencyType.NO_CONTINGENCY,
                    time_in_force=TimeInForce.GTC,
                    order_status=OrderStatus.FILLED,
                    price=price,
                    avg_px=instrument.make_price(avg_px),
                    quantity=instrument.make_qty(filled_qty),
                    filled_qty=instrument.make_qty(filled_qty),
                    ts_accepted=ts_last,
                    ts_last=ts_last,
                    report_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                )
                self._log.warning(f"Generated from fill report: {report}")
                reports.append(report)

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
        await self._maintain_active_market(command.instrument_id)

        venue_order_id = command.venue_order_id
        if venue_order_id is None:
            venue_order_id = self._cache.venue_order_id(command.client_order_id)
            if venue_order_id is None:
                self._log.error(
                    "Cannot generate an order status report for Polymarket without the venue order ID",
                )
                return None  # Failed

        self._log.info(
            f"Generating OrderStatusReport for "
            f"{repr(command.client_order_id) if command.client_order_id else ''} "
            f"{repr(command.venue_order_id) if command.venue_order_id else ''}",
        )

        retry_manager = await self._retry_manager_pool.acquire()
        try:
            response: JSON | None = await retry_manager.run(
                "generate_order_status_report",
                [command.client_order_id, venue_order_id],
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
                self._log.warning(
                    f"Cannot handle order report: instrument {instrument_id} not found "
                    f"(market={polymarket_order.market}, asset_id={polymarket_order.asset_id})",
                )
                return None

            return polymarket_order.parse_to_order_status_report(
                account_id=self.account_id,
                instrument=instrument,
                client_order_id=command.client_order_id,
                ts_init=self._clock.timestamp_ns(),
            )
        finally:
            await self._retry_manager_pool.release(retry_manager)

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        self._log.debug("Requesting FillReports...")
        reports: list[FillReport] = []

        params = TradeParams()
        if command.instrument_id:
            condition_id = get_polymarket_condition_id(command.instrument_id)
            asset_id = get_polymarket_token_id(command.instrument_id)
            params.market = condition_id
            params.asset_id = asset_id

        if command.start is not None:
            params.after = int(nanos_to_secs(command.start))
        if command.end is not None:
            params.before = int(nanos_to_secs(command.end))

        details = []
        if command.instrument_id:
            details.append(command.instrument_id)

        # Note: py_clob_client.get_trades() handles pagination internally
        retry_manager = await self._retry_manager_pool.acquire()
        try:
            response: JSON | None = await retry_manager.run(
                "generate_fill_reports",
                details,
                asyncio.to_thread,
                self._http_client.get_trades,
                params=params,
            )
            if response:
                # Uncomment for development
                # self._log.info(f"Processing {len(response)} trades", LogColor.MAGENTA)
                trade_ids: set[TradeId] = set()
                for json_obj in response:
                    raw = msgspec.json.encode(json_obj)
                    polymarket_trade = self._decoder_trade_report.decode(raw)

                    instrument_id = get_polymarket_instrument_id(
                        polymarket_trade.market,
                        polymarket_trade.asset_id,
                    )
                    instrument = self._cache.instrument(instrument_id)
                    if instrument is None:
                        self._log.warning(
                            f"Cannot handle trade report: instrument {instrument_id} not found "
                            f"(market={polymarket_trade.market}, asset_id={polymarket_trade.asset_id})",
                        )
                        continue

                    venue_order_id = polymarket_trade.venue_order_id(self._wallet_address)

                    if (
                        command.venue_order_id is not None
                        and venue_order_id != command.venue_order_id
                    ):
                        continue

                    client_order_id = self._cache.client_order_id(venue_order_id)
                    if client_order_id is None:
                        client_order_id = ClientOrderId(str(UUID4()))

                    report = polymarket_trade.parse_to_fill_report(
                        account_id=self.account_id,
                        instrument=instrument,
                        client_order_id=client_order_id,
                        maker_address=self._wallet_address,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    assert report.trade_id not in trade_ids, "trade IDs should be unique"
                    trade_ids.add(report.trade_id)
                    reports.append(report)
        finally:
            await self._retry_manager_pool.release(retry_manager)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} FillReport{plural}")

        return reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        reports: list[PositionStatusReport] = []

        if command.instrument_id is not None:
            instrument_ids = [command.instrument_id]
        else:
            instrument_ids = [inst.id for inst in self._cache.instruments(venue=POLYMARKET_VENUE)]

        for instrument_id in instrument_ids:
            self._log.debug(f"Requesting PositionStatusReport for {instrument_id}")
            token_id = get_polymarket_token_id(instrument_id)
            params = BalanceAllowanceParams(
                asset_type=AssetType.CONDITIONAL,
                token_id=token_id,
                signature_type=self._config.signature_type,
            )
            response: dict[str, Any] = await asyncio.to_thread(
                self._http_client.get_balance_allowance,
                params,
            )
            usdce_balance = usdce_from_units(int(response["balance"]))
            position_side = PositionSide.LONG if usdce_balance.raw > 0 else PositionSide.FLAT
            now = self._clock.timestamp_ns()

            report = PositionStatusReport(
                account_id=self.account_id,
                instrument_id=instrument_id,
                position_side=position_side,
                quantity=Quantity.from_raw(usdce_balance.raw, precision=USDC_POS.precision),
                report_id=UUID4(),
                ts_last=now,
                ts_init=now,
            )
            reports.append(report)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} PositionReport{plural}")

        return reports

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    def _get_neg_risk_for_instrument(self, instrument) -> bool:
        if instrument is None or instrument.info is None:
            return False
        return instrument.info.get("neg_risk", False)

    async def _query_account(self, _command: QueryAccount) -> None:
        # Specific account ID (sub account) not yet supported
        await self._update_account_state()

    async def _cancel_order(self, command: CancelOrder) -> None:
        # https://docs.polymarket.com/#cancel-an-order
        await self._maintain_active_market(command.instrument_id)

        order: Order | None = self._cache.order(command.client_order_id)

        if order is None:
            self._log.error(f"Cannot cancel order: {command.client_order_id!r} not found in cache")
            return

        if order.is_closed:
            self._log.warning(
                f"`CancelOrder` command for {command.client_order_id!r} when order already {order.status_string()} "
                "(will not send to exchange)",
            )
            return

        if order.venue_order_id is None:
            self._log.warning("Cannot cancel on Polymarket: no VenueOrderId")
            return

        retry_manager = await self._retry_manager_pool.acquire()
        try:
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
                    reason=str(reason),
                    ts_event=self._clock.timestamp_ns(),
                )
        finally:
            await self._retry_manager_pool.release(retry_manager)

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

        retry_manager = await self._retry_manager_pool.acquire()
        try:
            order_ids = []
            for cancel in valid_cancels:
                order = self._cache.order(cancel.client_order_id)
                if order and order.venue_order_id:
                    order_ids.append(order.venue_order_id.value)
            response: JSON | None = await retry_manager.run(
                "batch_cancel_orders",
                [command.instrument_id],
                asyncio.to_thread,
                self._http_client.cancel_orders,
                order_ids=order_ids,
            )
            if not response or not retry_manager.result:
                reason_map = dict.fromkeys(order_ids, retry_manager.message)
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
                        reason=str(reason),
                        ts_event=self._clock.timestamp_ns(),
                    )
        finally:
            await self._retry_manager_pool.release(retry_manager)

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        # https://docs.polymarket.com/#cancel-orders
        await self._maintain_active_market(command.instrument_id)

        # Polymarket API does not support side-specific cancellation
        if command.order_side != OrderSide.NO_ORDER_SIDE:
            self._log.warning(
                f"Polymarket does not support order_side filtering for cancel all orders; "
                f"ignoring order_side={command.order_side.name} and canceling all orders",
            )

        open_orders_strategy: list[Order] = self._cache.orders_open(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )
        if not open_orders_strategy:
            self._log.warning(f"No open orders to cancel for strategy {command.strategy_id}")
            return

        retry_manager = await self._retry_manager_pool.acquire()
        try:
            order_ids = [o.venue_order_id.value for o in open_orders_strategy]
            response: JSON | None = await retry_manager.run(
                "cancel_all_orders",
                [command.instrument_id],
                asyncio.to_thread,
                self._http_client.cancel_orders,
                order_ids=order_ids,
            )
            if not response or not retry_manager.result:
                reason_map = dict.fromkeys(order_ids, retry_manager.message)
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
                        reason=str(reason),
                        ts_event=self._clock.timestamp_ns(),
                    )
        finally:
            await self._retry_manager_pool.release(retry_manager)

    async def _submit_order(self, command: SubmitOrder) -> None:
        await self._maintain_active_market(command.instrument_id)
        print("Gleb submitting order:", command)
        order = command.order
        if order.is_closed:
            self._log.warning(f"Order {order} is already closed")
            return

        if order.is_reduce_only:
            self._log.error(
                f"Cannot submit order {order.client_order_id}: "
                "Reduce-only orders not supported on Polymarket",
                LogColor.RED,
            )
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason="REDUCE_ONLY_NOT_SUPPORTED",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        if order.is_post_only:
            self._log.error(
                f"Cannot submit order {order.client_order_id}: "
                "Post-only orders not supported on Polymarket",
                LogColor.RED,
            )
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason="POST_ONLY_NOT_SUPPORTED",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        if order.time_in_force not in VALID_POLYMARKET_TIME_IN_FORCE:
            self._log.error(
                f"Cannot submit order {order.client_order_id}: "
                f"Order time in force {order.tif_string()} not supported on Polymarket; "
                "use any of FOK, GTC, GTD, IOC",
                LogColor.RED,
            )
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason="UNSUPPORTED_TIME_IN_FORCE",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        instrument = self._cache.instrument(order.instrument_id)

        if order.order_type == OrderType.MARKET:
            await self._submit_market_order(command, instrument)
        elif order.order_type == OrderType.LIMIT:
            await self._submit_limit_order(command, instrument)
        else:
            self._log.error(
                f"Order type {order.type_string()} not supported on Polymarket, "
                "use either MARKET, LIMIT",
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

    async def _submit_market_order(self, command: SubmitOrder, instrument) -> None:
        self._log.debug("Creating Polymarket order", LogColor.MAGENTA)

        order = command.order
        tick_size = order.tags[0] if order.tags and len(order.tags) > 0 else None
        neg_risk = self._get_neg_risk_for_instrument(instrument)
        order_type = convert_tif_to_polymarket_order_type(order.time_in_force)

        # Use Rust client if available, otherwise use Python client
        if self._rust_client is not None:
            await self._submit_market_order_rust_as_limit(command, order, tick_size, neg_risk, order_type)
        else:
            await self._submit_market_order_python(command, order, tick_size, neg_risk, order_type)

    async def _submit_market_order_python(
        self,
        command: SubmitOrder,
        order: Order,
        tick_size: str | None,
        neg_risk: bool,
        order_type: str,
    ) -> None:
        """Submit market order using Python client."""
        # if order.side == OrderSide.BUY:
        #     if not order.is_quote_quantity:
        #         self._deny_market_order_quantity(
        #             order,
        #             "Polymarket market BUY orders require quote-denominated quantities; "
        #             "resubmit with `quote_quantity=True`",
        #         )
        #         return
        # else:
        #     if order.is_quote_quantity:
        #         self._deny_market_order_quantity(
        #             order,
        #             "Polymarket market SELL orders require base-denominated quantities; "
        #             "resubmit with `quote_quantity=False`",
        #         )
        #         return

        amount = float(order.quantity)

        market_order_args = MarketOrderArgs(
            token_id=get_polymarket_token_id(order.instrument_id),
            amount=amount,
            side=order_side_to_str(order.side),
            order_type=order_type,
        )

        options = PartialCreateOrderOptions(tick_size=tick_size, neg_risk=neg_risk)
        signing_start = self._clock.timestamp()
        signed_order = await asyncio.to_thread(
            self._http_client.create_market_order,
            market_order_args,
            options=options,
        )
        interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed Polymarket market order in {interval:.3f}s", LogColor.BLUE)

        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )
        print(f"GLEB order: {order}, signed_order: {signed_order}")
        await self._post_signed_order(order, signed_order)
        interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed and posted Polymarket market order in {interval:.3f}s", LogColor.BLUE)

    async def _submit_market_order_rust_as_limit(
        self,
        command: SubmitOrder,
        order: Order,
        tick_size: str | None,
        neg_risk: bool,
        order_type: str,
    ) -> None:
        """Submit market order as limit order using Rust client for signing, Python client for posting."""
        signing_start = self._clock.timestamp()
        self._log.info("Using Rust to sign market order as limit (Python to post)", LogColor.GREEN)

        # Determine price based on order side and tick_size
        min_price, max_price = POLYMARKET_MIN_MAX_PRICES.get(tick_size, (0.001, 0.999))

        # For BUY orders, use max price to ensure fill; for SELL orders, use min price
        if order.side == OrderSide.BUY:
            price = max_price
        else:
            price = min_price

        self._log.info(
            f"Market order converted to limit: side={order_side_to_str(order.side)}, "
            f"price={price}, tick_size={tick_size}",
            LogColor.CYAN,
        )

        # Market orders don't have expire_time_ns, so use 0 as expiration
        expiration_secs = 0

        # Call Rust client's create_order method (sign only)
        # Using positional arguments as required by PyO3 signature
        signed_order_json = await self._rust_client.create_order(
            get_polymarket_token_id(order.instrument_id),  # market_id
            order_side_to_str(order.side),                  # side
            float(order.quantity),                          # size
            price,                                          # price (determined from POLYMARKET_MIN_MAX_PRICES)
            expiration_secs,                                # expiration (calculated for market orders)
            float(tick_size) if tick_size else 0.01,       # tick_size
            neg_risk,                                       # neg_risk
        )

        interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed (only) Polymarket market order via Rust in {interval:.3f}s", LogColor.GREEN)

        # Debug: Log the signed order
        self._log.info(f"Rust signed order (JSON): {signed_order_json[:200]}...", LogColor.CYAN)

        # Parse the JSON string to get the signed order dict
        signed_order_dict = json.loads(signed_order_json)

        signed_order = SignedOrderWrapper(signed_order_dict)

        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        # Post the signed order using Python client (bypassing retry manager for better concurrency)
        response: JSON | None = await asyncio.to_thread(
            self._http_client.post_order,
            signed_order,
            convert_tif_to_polymarket_order_type(order.time_in_force),
        )

        if not response or not response.get("success"):
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Order submission failed: {response}",
                ts_event=self._clock.timestamp_ns(),
            )
        else:
            # Extract venue order ID and add to cache
            order_id_raw = response["orderID"]
            order_id_clean = order_id_raw.strip('"')  # Remove quotes if present
            venue_order_id = VenueOrderId(order_id_clean)
            self._cache.add_venue_order_id(order.client_order_id, venue_order_id)

            # Signal order event
            event = self._ack_events_order.get(venue_order_id)
            if event:
                event.set()

            # Signal trade event
            trade_event = self._ack_events_trade.get(venue_order_id)
            if trade_event:
                trade_event.set()

        total_interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed and posted market order (as limit, signed using Rust) in {total_interval:.3f}s", LogColor.GREEN)


    async def _submit_limit_order(self, command: SubmitOrder, instrument) -> None:
        self._log.debug("Creating Polymarket order", LogColor.MAGENTA)

        order = command.order
        tick_size = order.tags[0] if order.tags and len(order.tags) > 0 else None

        if order.is_quote_quantity:
            self._log.error(
                f"Cannot submit order {order.client_order_id}: UNSUPPORTED_QUOTE_QUANTITY",
                LogColor.RED,
            )
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason="UNSUPPORTED_QUOTE_QUANTITY",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        neg_risk = self._get_neg_risk_for_instrument(instrument)
        order_type = convert_tif_to_polymarket_order_type(order.time_in_force)

        # Use Rust client if available, otherwise use Python client
        if self._rust_client is not None:
            # await self._submit_limit_order_rust_create_and_post(command, order, tick_size, neg_risk, order_type)
            await self._submit_limit_order_rust_sign_only(command, order, tick_size, neg_risk, order_type)
            # await self._submit_limit_order_rust_sign_post_separately(command, order, tick_size, neg_risk, order_type)

        else:
            await self._submit_limit_order_python(command, order, tick_size, neg_risk, order_type)

    async def _submit_limit_order_python(
        self,
        command: SubmitOrder,
        order: Order,
        tick_size: str | None,
        neg_risk: bool,
        order_type: str,
    ) -> None:
        """Submit limit order using Python client."""
        # Create signed Polymarket limit order
        order_args = OrderArgs(
            price=float(order.price),
            token_id=get_polymarket_token_id(order.instrument_id),
            size=float(order.quantity),
            side=order_side_to_str(order.side),
            expiration=int(nanos_to_secs(order.expire_time_ns)),
        )

        options = PartialCreateOrderOptions(tick_size=tick_size, neg_risk=neg_risk)
        signing_start = self._clock.timestamp()
        signed_order = await asyncio.to_thread(
            self._http_client.create_order,
            order_args,
            options=options,
        )
        interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed Polymarket order in {interval:.3f}s", LogColor.BLUE)

        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        await self._post_signed_order(order, signed_order)
        interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed and posted Polymarket order in {interval:.3f}s", LogColor.BLUE)


    async def _submit_limit_order_rust_create_and_post(
        self,
        command: SubmitOrder,
        order: Order,
        tick_size: str | None,
        neg_risk: bool,
        order_type: str,
    ) -> None:
        """Submit limit order using Rust client (sign + post in one call)."""
        signing_start = self._clock.timestamp()
        self._log.info("Using Rust to sign and post order", LogColor.GREEN)

        # Call Rust client's create_and_post_order method
        # Using positional arguments as required by PyO3 signature
        response = await self._rust_client.create_and_post_order(
            get_polymarket_token_id(order.instrument_id),  # market_id
            order_side_to_str(order.side),                  # side
            float(order.quantity),                          # size
            float(order.price),                             # price
            int(nanos_to_secs(order.expire_time_ns)),      # expiration
            float(tick_size) if tick_size else 0.01,       # tick_size
            neg_risk,                                       # neg_risk
            order_type,                                     # order_type
        )

        interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed and posted Polymarket order via Rust in {interval:.3f}s", LogColor.GREEN)

        # Debug: Log the response structure
        self._log.info(f"Rust response: {response}", LogColor.CYAN)

        # Process response and extract venue order ID FIRST (before generating events)
        if not response or not response.get("success"):
            self._log.error(
                f"Rust order submission failed: {response}",
                LogColor.RED,
            )
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Order rejected by exchange: {response}",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Extract and store venue order ID
        # Rust returns orderID as a quoted string, so we need to strip quotes
        order_id_raw = response["orderID"]
        order_id_clean = order_id_raw.strip('"')  # Remove surrounding quotes
        venue_order_id = VenueOrderId(order_id_clean)
        self._log.info(
            f"Order accepted: {order.client_order_id} -> {venue_order_id} (raw: {order_id_raw})",
            LogColor.GREEN,
        )

        # Add to cache BEFORE generating submitted event
        self._cache.add_venue_order_id(order.client_order_id, venue_order_id)

        # Signal order event
        event = self._ack_events_order.get(venue_order_id)
        if event:
            event.set()

        # Signal trade event
        trade_event = self._ack_events_trade.get(venue_order_id)
        if trade_event:
            trade_event.set()

        # Now generate the submitted event (after cache is updated)
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

    async def _submit_limit_order_rust_sign_only(
        self,
        command: SubmitOrder,
        order: Order,
        tick_size: str | None,
        neg_risk: bool,
        order_type: str,
    ) -> None:
        """Submit limit order using Rust client for signing only, then Python client for posting."""
        signing_start = self._clock.timestamp()
        self._log.info("Using Rust to sign order (Python to post)", LogColor.GREEN)

        # Call Rust client's create_order method (sign only)
        # Using positional arguments as required by PyO3 signature
        signed_order_json = await self._rust_client.create_order(
            get_polymarket_token_id(order.instrument_id),  # market_id
            order_side_to_str(order.side),                  # side
            float(order.quantity),                          # size
            float(order.price),                             # price
            int(nanos_to_secs(order.expire_time_ns)),      # expiration
            float(tick_size) if tick_size else 0.01,       # tick_size
            neg_risk,                                       # neg_risk
        )

        interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed (only) Polymarket order via Rust in {interval:.3f}s", LogColor.GREEN)

        # Debug: Log the signed order
        self._log.info(f"Rust signed order (JSON): {signed_order_json[:200]}...", LogColor.CYAN)

        # Parse the JSON string to get the signed order dict
        signed_order_dict = json.loads(signed_order_json)

        signed_order = SignedOrderWrapper(signed_order_dict)

        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        # Post the signed order using Python client (bypassing retry manager for better concurrency)
        response: JSON | None = await asyncio.to_thread(
            self._http_client.post_order,
            signed_order,
            convert_tif_to_polymarket_order_type(order.time_in_force),
        )

        if not response or not response.get("success"):
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Order submission failed: {response}",
                ts_event=self._clock.timestamp_ns(),
            )
        else:
            # Extract venue order ID and add to cache
            order_id_raw = response["orderID"]
            order_id_clean = order_id_raw.strip('"')  # Remove quotes if present
            venue_order_id = VenueOrderId(order_id_clean)
            self._cache.add_venue_order_id(order.client_order_id, venue_order_id)

            # Signal order event
            event = self._ack_events_order.get(venue_order_id)
            if event:
                event.set()

            # Signal trade event
            trade_event = self._ack_events_trade.get(venue_order_id)
            if trade_event:
                trade_event.set()

        total_interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed and posted (signed using Rust) in {total_interval:.3f}s", LogColor.GREEN)

    async def _submit_limit_order_rust_sign_post_separately(
        self,
        command: SubmitOrder,
        order: Order,
        tick_size: str | None,
        neg_risk: bool,
        order_type: str,
    ) -> None:
        """Submit limit order using Rust client for signing AND posting, but as separate calls."""
        signing_start = self._clock.timestamp()
        self._log.info("Using Rust to sign order (separate sign + post)", LogColor.GREEN)

        # Step 1: Sign the order with Rust
        signed_order_json = await self._rust_client.create_order(
            get_polymarket_token_id(order.instrument_id),  # market_id
            order_side_to_str(order.side),                  # side
            float(order.quantity),                          # size
            float(order.price),                             # price
            int(nanos_to_secs(order.expire_time_ns)),      # expiration
            float(tick_size) if tick_size else 0.01,       # tick_size
            neg_risk,                                       # neg_risk
        )

        sign_interval = self._clock.timestamp() - signing_start
        self._log.info(f"Signed order via Rust in {sign_interval:.3f}s", LogColor.GREEN)

        # Debug: Log the signed order
        self._log.info(f"Rust signed order (JSON): {signed_order_json[:200]}...", LogColor.CYAN)

        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        # Step 2: Post the signed order with Rust
        post_start = self._clock.timestamp()
        response = await self._rust_client.post_order(
            signed_order_json,  # Already a JSON string
            order_type,
        )

        post_interval = self._clock.timestamp() - post_start
        self._log.info(f"Posted order via Rust in {post_interval:.3f}s", LogColor.GREEN)

        # Debug: Log the response
        self._log.info(f"Rust post response: {response}", LogColor.CYAN)

        # Process response and extract venue order ID
        if not response or not response.get("success"):
            self._log.error(
                f"Rust order posting failed: {response}",
                LogColor.RED,
            )
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Order rejected by exchange: {response}",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Extract venue order ID and add to cache
        order_id_raw = response["orderID"]
        order_id_clean = order_id_raw.strip('"')  # Remove quotes if present
        venue_order_id = VenueOrderId(order_id_clean)
        self._log.info(
            f"Order accepted: {order.client_order_id} -> {venue_order_id} (raw: {order_id_raw})",
            LogColor.GREEN,
        )

        # Add to cache BEFORE generating submitted event
        self._cache.add_venue_order_id(order.client_order_id, venue_order_id)

        # Signal order event
        event = self._ack_events_order.get(venue_order_id)
        if event:
            event.set()

        # Signal trade event
        trade_event = self._ack_events_trade.get(venue_order_id)
        if trade_event:
            trade_event.set()

        total_interval = self._clock.timestamp() - signing_start
        self._log.info(
            f"Signed and posted via Rust (separate calls) in {total_interval:.3f}s "
            f"(sign: {sign_interval:.3f}s, post: {post_interval:.3f}s)",
            LogColor.GREEN,
        )

    def submit_order_list(self, command: SubmitOrderList) -> None:
        """
        Submit an order list for execution.

        For Polymarket, if the order list contains orders for the same instrument,
        they are submitted sequentially (normal behavior). However, you can use
        `submit_order_batch_concurrent` for concurrent submission across different
        instruments.

        Parameters
        ----------
        command : SubmitOrderList
            The command to execute.

        """
        # Extract orders from the order list
        orders = list(command.order_list.orders)

        # Submit concurrently (all orders for same instrument in an OrderList)
        self.create_task(
            self.submit_order_batch_concurrent(orders),
            log_msg=f"submit_order_list-{command.order_list.id}",
        )

    async def submit_order_batch_concurrent(self, orders: list[Order]) -> None:
        """
        Submit multiple orders concurrently across different instruments.

        This method parallelizes both signing and HTTP posting to minimize
        total submission time. Designed for batch market-making strategies
        where orders need to hit the exchange simultaneously.

        Parameters
        ----------
        orders : list[Order]
            The orders to submit concurrently (can have different instrument IDs).

        Notes
        -----
        - Each order is validated and signed independently
        - All orders are POSTed to the exchange concurrently
        - Errors are handled per-order (one failure doesn't affect others)
        - Appropriate events (submitted, accepted, rejected) are generated for each order

        """
        if not orders:
            self._log.warning("submit_order_batch_concurrent called with empty order list")
            return

        batch_start = self._clock.timestamp()
        self._log.info(
            f"Starting concurrent batch submission for {len(orders)} orders",
            LogColor.BLUE,
        )

        # Step 1: Validate all orders and prepare instruments
        validated_orders: list[tuple[Order, Any]] = []
        for order in orders:
            # Perform same validations as _submit_order
            if order.is_closed:
                self._log.warning(f"Order {order} is already closed - skipping")
                continue

            if order.is_reduce_only:
                self.generate_order_denied(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason="REDUCE_ONLY_NOT_SUPPORTED",
                    ts_event=self._clock.timestamp_ns(),
                )
                continue

            if order.is_post_only:
                self.generate_order_denied(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason="POST_ONLY_NOT_SUPPORTED",
                    ts_event=self._clock.timestamp_ns(),
                )
                continue

            if order.time_in_force not in VALID_POLYMARKET_TIME_IN_FORCE:
                self.generate_order_denied(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason="UNSUPPORTED_TIME_IN_FORCE",
                    ts_event=self._clock.timestamp_ns(),
                )
                continue

            if order.order_type not in (OrderType.MARKET, OrderType.LIMIT):
                self._log.error(
                    f"Order type {order.type_string()} not supported - skipping",
                )
                continue

            instrument = self._cache.instrument(order.instrument_id)
            if not instrument:
                self._log.error(f"Instrument {order.instrument_id} not found - skipping")
                continue

            validated_orders.append((order, instrument))

        if not validated_orders:
            self._log.warning("No valid orders to submit after validation")
            return

        # Step 2: Sign all orders concurrently
        sign_start = self._clock.timestamp()

        async def sign_order(order_tuple: tuple[Order, Any]) -> tuple[Order, Any, Any]:
            order, instrument = order_tuple

            # Generate OrderSubmitted event
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

            try:
                # Extract tick_size and neg_risk from order tags (same as regular submission)
                tick_size = None
                neg_risk = False
                if order.tags:
                    tick_size = order.tags[0] if len(order.tags) > 0 else None
                    neg_risk = order.tags[1] if len(order.tags) > 1 else False

                # Determine order parameters
                if order.order_type == OrderType.MARKET:
                    # For market orders, convert to limit with extreme price
                    min_price, max_price = POLYMARKET_MIN_MAX_PRICES.get(tick_size, (0.001, 0.999))
                    if order.side == OrderSide.BUY:
                        price_value = max_price
                    else:
                        price_value = min_price
                    amount = float(order.quantity)
                    expiration_secs = 0  # Market orders don't have expiration
                else:  # LIMIT
                    price_value = float(order.price)
                    amount = float(order.quantity)
                    # Get expiration for limit orders
                    if order.expire_time_ns:
                        expiration_secs = int(nanos_to_secs(order.expire_time_ns))
                    else:
                        expiration_secs = 0

                # Get token_id from instrument_id (same way as regular submission)
                token_id = get_polymarket_token_id(order.instrument_id)

                # Sign order (using Rust client if available, otherwise Python)
                if self._rust_client:
                    # Rust client's create_order method (sign only)
                    signed_order_json = await self._rust_client.create_order(
                        token_id,                                    # market_id
                        order_side_to_str(order.side),              # side
                        amount,                                      # size
                        price_value,                                 # price
                        expiration_secs,                             # expiration
                        float(tick_size) if tick_size else 0.01,   # tick_size
                        neg_risk,                                    # neg_risk
                    )
                    # Parse the JSON string to get the signed order dict
                    signed_order_dict = json.loads(signed_order_json)
                    signed_order = SignedOrderWrapper(signed_order_dict)
                else:
                    # For Python client, use create_market_order or create_order
                    if order.order_type == OrderType.MARKET:
                        # Market order
                        market_order_args = MarketOrderArgs(
                            token_id=token_id,
                            amount=amount,
                            side=order_side_to_str(order.side),
                            order_type=convert_tif_to_polymarket_order_type(order.time_in_force),
                        )
                        options = PartialCreateOrderOptions(
                            tick_size=float(tick_size) if tick_size else 0.01,
                            neg_risk=neg_risk,
                        )
                        signed_order = await asyncio.to_thread(
                            self._http_client.create_market_order,
                            market_order_args,
                            options=options,
                        )
                    else:
                        # Limit order
                        order_args = OrderArgs(
                            price=price_value,
                            token_id=token_id,
                            size=amount,
                            side=order_side_to_str(order.side),
                            expiration=expiration_secs,
                        )
                        options = PartialCreateOrderOptions(
                            tick_size=float(tick_size) if tick_size else 0.01,
                            neg_risk=neg_risk,
                        )
                        signed_order = await asyncio.to_thread(
                            self._http_client.create_order,
                            order_args,
                            options=options,
                        )

                return (order, instrument, signed_order)
            except Exception as e:
                self._log.error(f"Error signing order {order.client_order_id}: {e}")
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=f"SIGNING_ERROR: {e}",
                    ts_event=self._clock.timestamp_ns(),
                )
                return (order, instrument, None)

        # Sign all orders in parallel
        signed_results = await asyncio.gather(*[sign_order(o) for o in validated_orders])

        sign_interval = self._clock.timestamp() - sign_start
        self._log.info(
            f"Signed {len(validated_orders)} orders in {sign_interval:.3f}s",
            LogColor.CYAN,
        )

        # Filter out failed signings
        successfully_signed = [(o, i, s) for o, i, s in signed_results if s is not None]

        if not successfully_signed:
            self._log.warning("No orders successfully signed")
            return

        # Step 3: POST all signed orders in a SINGLE HTTP call using post_orders
        # Always use Python client's post_orders for maximum performance
        post_start = self._clock.timestamp()

        try:
            # Build list of PostOrdersArgs for batch POST
            post_orders_args = []
            for order, instrument, signed_order in successfully_signed:
                post_orders_args.append(
                    PostOrdersArgs(
                        order=signed_order,
                        orderType=convert_tif_to_polymarket_order_type(order.time_in_force),
                    )
                )

            # POST all orders in a SINGLE HTTP call (regardless of signing method)
            self._log.info(
                f"Posting {len(post_orders_args)} orders in single HTTP call",
                LogColor.CYAN,
            )

            retry_manager = await self._retry_manager_pool.acquire()
            try:
                response = await retry_manager.run(
                    "submit_orders_batch",
                    [o.client_order_id for o, _, _ in successfully_signed],
                    asyncio.to_thread,
                    self._http_client.post_orders,
                    post_orders_args,
                )
            finally:
                await self._retry_manager_pool.release(retry_manager)

            # Parse response and generate events
            if response and isinstance(response, list):
                for idx, (order, instrument, signed_order) in enumerate(successfully_signed):
                    order_response = response[idx] if idx < len(response) else None

                    if order_response and order_response.get("success"):
                        venue_order_id = VenueOrderId(order_response["orderID"])
                        self._cache.add_venue_order_id(order.client_order_id, venue_order_id)

                        # Generate OrderAccepted event
                        self.generate_order_accepted(
                            strategy_id=order.strategy_id,
                            instrument_id=order.instrument_id,
                            client_order_id=order.client_order_id,
                            venue_order_id=venue_order_id,
                            ts_event=self._clock.timestamp_ns(),
                        )

                        # Signal order event
                        event = self._ack_events_order.get(venue_order_id)
                        if event:
                            event.set()

                        # Signal trade event
                        trade_event = self._ack_events_trade.get(venue_order_id)
                        if trade_event:
                            trade_event.set()
                    else:
                        error_msg = order_response.get("error", "Unknown error") if order_response else "No response"
                        self.generate_order_rejected(
                            strategy_id=order.strategy_id,
                            instrument_id=order.instrument_id,
                            client_order_id=order.client_order_id,
                            reason=f"POST_ERROR: {error_msg}",
                            ts_event=self._clock.timestamp_ns(),
                        )
            else:
                # Response format unexpected
                self._log.error(f"Unexpected response format from post_orders: {response}")
                for order, instrument, signed_order in successfully_signed:
                    self.generate_order_rejected(
                        strategy_id=order.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        reason="UNEXPECTED_RESPONSE_FORMAT",
                        ts_event=self._clock.timestamp_ns(),
                    )

        except Exception as e:
            self._log.error(f"Error in batch POST: {e}")
            # Reject all orders
            for order, instrument, signed_order in successfully_signed:
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=f"BATCH_POST_ERROR: {e}",
                    ts_event=self._clock.timestamp_ns(),
                )

        post_interval = self._clock.timestamp() - post_start
        batch_total = self._clock.timestamp() - batch_start

        self._log.info(
            f"Batch submission complete: {len(successfully_signed)} orders in {batch_total:.3f}s "
            f"(sign: {sign_interval:.3f}s, post: {post_interval:.3f}s)",
            LogColor.GREEN,
        )

    async def _post_signed_order(self, order: Order, signed_order) -> None:
        retry_manager = await self._retry_manager_pool.acquire()
        try:
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
                    reason=str(retry_manager.message),
                    ts_event=self._clock.timestamp_ns(),
                )
            else:
                venue_order_id = VenueOrderId(response["orderID"])
                self._cache.add_venue_order_id(order.client_order_id, venue_order_id)

                # Signal order event
                event = self._ack_events_order.get(venue_order_id)
                if event:
                    event.set()

                # Signal trade event
                trade_event = self._ack_events_trade.get(venue_order_id)
                if trade_event:
                    trade_event.set()
        finally:
            await self._retry_manager_pool.release(retry_manager)

    def _handle_ws_message(self, raw: bytes) -> None:
        try:
            if self._config.log_raw_ws_messages:
                self._log.info(
                    str(json.dumps(msgspec.json.decode(raw), indent=4)),
                    color=LogColor.MAGENTA,
                )

            msg = self._decoder_user_msg.decode(raw)
            if isinstance(msg, PolymarketUserOrder):
                self._handle_ws_order_msg(msg, wait_for_ack=True)
            elif isinstance(msg, PolymarketUserTrade):
                self._add_trade_to_cache(msg, raw)
                self._handle_ws_trade_msg(msg, wait_for_ack=True)
            else:
                self._log.error(f"Unrecognized websocket message {msg}")
        except Exception as e:
            self._log.exception(
                f"Error handling websocket message: {e.__class__.__name__} - "
                f"raw message: {raw.decode()}",
                e,
            )

    def _add_trade_to_cache(self, msg: PolymarketUserTrade, raw: bytes) -> None:
        start_us = self._clock.timestamp_us()
        cache_key = get_polymarket_trades_key(msg.taker_order_id, msg.id)
        self._cache.add(cache_key, raw)
        interval_us = self._clock.timestamp_us() - start_us
        self._log.info(
            f"Added trade {msg.id} {msg.status.value} to {cache_key} in {interval_us}μs",
            LogColor.BLUE,
        )

    async def _wait_for_ack_order(
        self,
        msg: PolymarketUserOrder,
        venue_order_id: VenueOrderId,
    ) -> None:
        client_order_id = self._cache.client_order_id(venue_order_id)
        if client_order_id is not None:
            self._handle_ws_order_msg(msg, wait_for_ack=False)
            return

        event = asyncio.Event()
        self._ack_events_order[venue_order_id] = event

        timed_out = False
        try:
            await asyncio.wait_for(event.wait(), timeout=self._config.ack_timeout_secs)
        except TimeoutError:
            timed_out = True
            self._log.warning(f"Timed out awaiting placement ack for {venue_order_id!r}")
        finally:
            self._ack_events_order.pop(venue_order_id, None)

        # If timed out, reject the order instead of processing it
        if timed_out:
            # Try to get client_order_id again in case it was set during timeout
            client_order_id = self._cache.client_order_id(venue_order_id)
            if client_order_id is not None:
                order = self._cache.order(client_order_id)
                if order is not None:
                    strategy_id = self._cache.strategy_id_for_order(client_order_id)
                    instrument_id = order.instrument_id

                    self._log.warning(
                        f"Rejecting order {client_order_id} due to placement ack timeout"
                    )
                    self.generate_order_rejected(
                        strategy_id=strategy_id,
                        instrument_id=instrument_id,
                        client_order_id=client_order_id,
                        reason=f"Placement acknowledgment timeout after {self._config.ack_timeout_secs}s",
                        ts_event=self._clock.timestamp_ns(),
                    )
                    return
            else:
                self._log.error(
                    f"Cannot reject timed-out order {venue_order_id!r}: "
                    f"client_order_id not found in cache"
                )
                # Still process the message as fallback
                self._handle_ws_order_msg(msg, wait_for_ack=False)
                return

        self._handle_ws_order_msg(msg, wait_for_ack=False)

    async def _wait_for_ack_trade(
        self,
        msg: PolymarketUserTrade,
        venue_order_id: VenueOrderId,
    ) -> None:
        self._log.debug(f"Waiting for trade ack for {venue_order_id!r}...")

        client_order_id = self._cache.client_order_id(venue_order_id)
        if client_order_id is not None:
            self._handle_ws_trade_msg(msg, wait_for_ack=False)
            return

        event = asyncio.Event()
        self._ack_events_trade[venue_order_id] = event

        try:
            await asyncio.wait_for(event.wait(), timeout=self._config.ack_timeout_secs)
        except TimeoutError:
            self._log.warning(f"Timed out awaiting TRADE ack for {venue_order_id!r}")  # GLEB Change
        finally:
            self._ack_events_trade.pop(venue_order_id, None)

        self._handle_ws_trade_msg(msg, wait_for_ack=False)

    def _handle_ws_order_msg(self, msg: PolymarketUserOrder, wait_for_ack: bool):
        self._log.debug(f"Handling order message, {wait_for_ack=}")

        venue_order_id = msg.venue_order_id()
        instrument_id = get_polymarket_instrument_id(msg.market, msg.asset_id)
        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.warning(
                f"Received order message for unknown instrument {instrument_id} "
                f"(market={msg.market}, asset_id={msg.asset_id}). "
                f"This may indicate the instrument is not subscribed or cached, skipping order processing",
            )
            return

        if wait_for_ack:
            self.create_task(self._wait_for_ack_order(msg, venue_order_id))
            return

        client_order_id = self._cache.client_order_id(venue_order_id)
        self._log.debug(f"Processing order update for {client_order_id!r}")

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

        self._log.debug(f"Order {msg.type.value}: {client_order_id!r}", LogColor.MAGENTA)

        match msg.type:
            case PolymarketEventType.PLACEMENT:
                # Check if order is already accepted to avoid duplicate accepted events
                order = self._cache.order(client_order_id) if client_order_id else None
                if order is None or not order.is_open:
                    self.generate_order_accepted(
                        strategy_id=strategy_id,
                        instrument_id=instrument_id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        ts_event=self._clock.timestamp_ns(),
                    )
                else:
                    self._log.debug(
                        f"Order {client_order_id!r} already accepted - skipping duplicate placement event",
                    )
            case PolymarketEventType.CANCELLATION:
                self.generate_order_canceled(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=millis_to_nanos(int(msg.timestamp)),
                )
            case PolymarketEventType.UPDATE | PolymarketEventType.TRADE:
                # We skip these events as they are handled by trade messages
                self._log.debug(f"Skipping order update: {msg}")
            case _:  # Branch never hit unless code changes (leave in place)
                raise RuntimeError(f"Unknown `PolymarketEventType`, was '{msg.type.value}'")

    def _handle_ws_trade_msg(self, msg: PolymarketUserTrade, wait_for_ack: bool):
        self._log.debug(f"Handling trade message, {wait_for_ack=}")

        trade_id = TradeId(msg.id)
        trade_str = f"Trade {trade_id}"
        log_msg = f"{trade_str} {msg.status.value}: {msg}"

        match msg.status:
            case PolymarketTradeStatus.RETRYING:
                self._log.warning(log_msg)
                return
            case PolymarketTradeStatus.FAILED:
                self._log.error(log_msg)
                return
            case _:
                self._log.info(log_msg, LogColor.BLUE)

        venue_order_id = msg.venue_order_id(self._wallet_address)
        instrument_id = get_polymarket_instrument_id(msg.market, msg.asset_id)
        instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            self._log.warning(
                f"Received trade message for unknown instrument {instrument_id} "
                f"(market={msg.market}, asset_id={msg.asset_id}). "
                f"This may indicate the instrument is not subscribed or cached, skipping trade processing",
            )
            return

        if wait_for_ack:
            self.create_task(self._wait_for_ack_trade(msg, venue_order_id))
            return

        client_order_id = self._cache.client_order_id(venue_order_id)
        strategy_id = None

        if client_order_id:
            strategy_id = self._cache.strategy_id_for_order(client_order_id)

        if strategy_id is None:
            self._log.warning("Strategy ID not found - parsing fill report")
            report = msg.parse_to_fill_report(
                account_id=self.account_id,
                instrument=instrument,
                client_order_id=client_order_id,
                maker_address=self._wallet_address,
                ts_init=self._clock.timestamp_ns(),
            )
            self._send_fill_report(report)
            self._processed_trades.append(trade_id)
            return

        order = self._cache.order(client_order_id)

        if order is None:
            self._log.error(f"Cannot process trade: {client_order_id!r} not found in cache")
            return

        if trade_id in order.trade_ids or trade_id in self._processed_trades:
            self._log.debug(f"{trade_str} already processed - skipping")
            return

        if order.is_closed:
            self._log.warning(f"Order already closed - skipping trade processing: {order}")
            return  # Already closed (only status update)

        last_qty = instrument.make_qty(msg.last_qty(self._wallet_address))
        last_px = instrument.make_price(msg.last_px(self._wallet_address))
        commission = float(last_qty * last_px) * basis_points_as_percentage(
            float(msg.get_fee_rate_bps(self._wallet_address)),
        )
        ts_event = millis_to_nanos(int(msg.match_time))

        self.generate_order_filled(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            venue_position_id=None,  # Not applicable on Polymarket
            trade_id=trade_id,
            order_side=msg.order_side(),
            order_type=order.order_type,
            last_qty=last_qty,
            last_px=last_px,
            quote_currency=USDC_POS,
            commission=Money(commission, USDC_POS),
            liquidity_side=msg.liquidity_side(),
            ts_event=ts_event,
            info=msg.to_dict(),
        )

        self._processed_trades.append(trade_id)

        self.create_task(self._update_account_state())
