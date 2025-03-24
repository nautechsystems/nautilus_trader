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
from decimal import Decimal
from uuid import UUID

import msgspec

from nautilus_trader.adapters.okx.common.constants import OKX_VENUE
from nautilus_trader.adapters.okx.common.credentials import get_api_key
from nautilus_trader.adapters.okx.common.credentials import get_api_secret
from nautilus_trader.adapters.okx.common.credentials import get_passphrase
from nautilus_trader.adapters.okx.common.enums import OKXEnumParser
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXMarginMode
from nautilus_trader.adapters.okx.common.enums import OKXOrderStatus
from nautilus_trader.adapters.okx.common.enums import OKXOrderType
from nautilus_trader.adapters.okx.common.enums import OKXPositionSide
from nautilus_trader.adapters.okx.common.enums import OKXTradeMode
from nautilus_trader.adapters.okx.common.enums import OKXWsBaseUrlType
from nautilus_trader.adapters.okx.common.symbol import OKXSymbol
from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.http.account import OKXAccountHttpAPI
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.http.trade import OKXTradeHttpAPI
from nautilus_trader.adapters.okx.providers import ALLOWED_INSTRUMENT_TYPE_CONTRACT_TYPE_COMBOS
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.adapters.okx.providers import get_instrument_type_contract_type_combos
from nautilus_trader.adapters.okx.schemas.ws import OKXWsAccountPushDataMsg
from nautilus_trader.adapters.okx.schemas.ws import OKXWsEventMsg
from nautilus_trader.adapters.okx.schemas.ws import OKXWsFillsPushDataMsg
from nautilus_trader.adapters.okx.schemas.ws import OKXWsGeneralMsg
from nautilus_trader.adapters.okx.schemas.ws import OKXWsOrderMsg
from nautilus_trader.adapters.okx.schemas.ws import OKXWsOrderMsgData
from nautilus_trader.adapters.okx.schemas.ws import OKXWsOrdersPushDataMsg
from nautilus_trader.adapters.okx.schemas.ws import OKXWsPushDataMsg
from nautilus_trader.adapters.okx.schemas.ws import decoder_ws_account
from nautilus_trader.adapters.okx.schemas.ws import decoder_ws_order
from nautilus_trader.adapters.okx.schemas.ws import decoder_ws_orders
from nautilus_trader.adapters.okx.websocket.client import OKX_CHANNEL_WS_BASE_URL_TYPE_MAP
from nautilus_trader.adapters.okx.websocket.client import OKXWebsocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orders import LimitIfTouchedOrder
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketIfTouchedOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.model.orders import TrailingStopMarketOrder
from nautilus_trader.model.position import Position


class OKXExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the OKX centralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : OKXHttpClient
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
        client: OKXHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: OKXInstrumentProvider,
        config: OKXExecClientConfig,
        name: str | None,
    ) -> None:
        self._instrument_types = instrument_provider._instrument_types
        self._contact_types = instrument_provider._contract_types
        self._inst_type_contr_type_combos = get_instrument_type_contract_type_combos(
            self._instrument_types,
            self._contact_types,
        )
        if (
            OKXInstrumentType.SPOT in self._instrument_types
            or OKXInstrumentType.MARGIN in self._instrument_types
        ):
            if (
                len(
                    {
                        it
                        for it in self._instrument_types
                        if it not in [OKXInstrumentType.SPOT, OKXInstrumentType.MARGIN]
                    },
                )
                > 0
            ):
                raise ValueError(
                    "Cannot currently configure SPOT or MARGIN with other instrument types, "
                    f"instrument provider's instrument types were {self._instrument_types}",
                )

        if instrument_provider._instrument_types == (OKXInstrumentType.SPOT,):
            account_type = AccountType.CASH
        else:
            account_type = AccountType.MARGIN

        self._instrument_provider: OKXInstrumentProvider  # type hints
        super().__init__(
            loop=loop,
            client_id=ClientId(name or OKX_VENUE.value),
            venue=OKX_VENUE,
            oms_type=OmsType.NETTING,  # TODO is HEDGING 'long/short' Position Mode (one-way mode)?
            instrument_provider=instrument_provider,
            account_type=account_type,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Cache exec config object (because self._config is coerced to dictionary in ``Component``)
        self._exec_config = config

        if account_type == AccountType.CASH:
            self._log.info(
                f"Account type {account_type!r} set for OKXExecutionClient because instrument "
                "types are SPOT only",
                LogColor.BLUE,
            )
        else:
            self._log.info(
                f"Account type {account_type!r} set for OKXExecutionClient because instrument "
                f"types include {instrument_provider._instrument_types}",
                LogColor.BLUE,
            )

        # Set trading mode
        if self.account_type == AccountType.CASH:
            self._trade_mode = OKXTradeMode.CASH  # OKX Account Mode: 'Spot mode'
        elif self._exec_config.margin_mode == OKXMarginMode.CROSS:
            self._trade_mode = OKXTradeMode.CROSS  # OKX Account Mode: 'Multi-currency margin mode'
        else:
            self._trade_mode = OKXTradeMode.ISOLATED  # OKX Account Mode: 'Portfolio margin mode'
        # TODO: what about OKX Account Mode: 'Spot and futures mode'

        # Set position side. Currently only NET is supported.
        # TODO when is long/short posSide appropriate?
        self._position_side = OKXPositionSide.NET

        # TODO determine whether `_is_dual_side_position` (look at binance adapter) is needed

        # # Configuration
        # self._use_reduce_only = config.use_reduce_only
        # self._use_position_ids = config.use_position_ids
        # self._max_retries = config.max_retries or 0
        # self._retry_delay = config.retry_delay or 1.0
        # self._log.info(f"{config.use_reduce_only=}", LogColor.BLUE)
        # self._log.info(f"{config.use_position_ids=}", LogColor.BLUE)
        # self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        # self._log.info(f"{config.retry_delay=}", LogColor.BLUE)

        self._enum_parser = OKXEnumParser()

        account_id_suffix = "/".join(  # e.g., "SWAP[LINEAR,INVERSE]/SPOT"
            [
                f"""{i.name}[{
                    ','.join([
                    c.name for c in ALLOWED_INSTRUMENT_TYPE_CONTRACT_TYPE_COMBOS[i] if c.name != 'NONE'
                    and c in self._contact_types
                    ])
                }]""".replace(
                    "[]",
                    "",
                )
                for i in self._instrument_types
            ],
        )
        account_id = AccountId(f"{name or OKX_VENUE.value}-{account_id_suffix}")
        self._set_account_id(account_id)

        # WebSocket API
        self._ws_client = OKXWebsocketClient(
            clock=clock,
            handler=self._handle_ws_message,
            handler_reconnect=None,
            api_key=config.api_key or get_api_key(config.is_demo),
            api_secret=config.api_secret or get_api_secret(config.is_demo),
            passphrase=config.passphrase or get_passphrase(config.is_demo),
            base_url=config.base_url_ws,
            ws_base_url_type=OKXWsBaseUrlType.PRIVATE,
            is_demo=config.is_demo,
            loop=self._loop,
        )

        # Http API
        self._http_account = OKXAccountHttpAPI(client=client, clock=clock)
        self._http_trade = OKXTradeHttpAPI(client=client, clock=clock)

        # Order submission
        self._submit_order_methods = {
            OrderType.MARKET: self._submit_market_order,
            OrderType.LIMIT: self._submit_limit_order,
            # OrderType.STOP_MARKET: self._submit_stop_market_order,
            # OrderType.STOP_LIMIT: self._submit_stop_limit_order,
            # OrderType.MARKET_IF_TOUCHED: self._submit_market_if_touched_order,
            # OrderType.LIMIT_IF_TOUCHED: self._submit_limit_if_touched_order,
            # OrderType.TRAILING_STOP_MARKET: self._submit_trailing_stop_market,
            # OrderType.TRAILING_STOP_LIMIT: self._submit_trailing_stop_limit,
        }

        # Decoders
        self._decoder_ws_general_msg = msgspec.json.Decoder(OKXWsGeneralMsg)
        self._decoder_ws_event_msg = msgspec.json.Decoder(OKXWsEventMsg)
        self._decoder_ws_push_data_msg = msgspec.json.Decoder(OKXWsPushDataMsg)
        self._decoder_ws_order_msg = decoder_ws_order()  # for order rejections
        self._decoder_ws_account_msg = decoder_ws_account()
        # self._decoder_ws_fills_msg = decoder_ws_fills()  # "orders" channel provides fills
        self._decoder_ws_orders_msg = decoder_ws_orders()
        # self._decoder_ws_positions_msg = decoder_ws_positions()  # nautilus uses fills to updt pos

        # Hot cache
        self._unhandled_order_msgs: dict[
            str,
            tuple[OKXSymbol, VenueOrderId | None, ClientOrderId | None, StrategyId | None],
        ] = {}  # keys are msg id's created with `self._create_okx_order_msg_id()`

        # OKX client order id generator
        self._client_order_id_generator = ClientOrderIdGenerator(self._cache)

    async def _connect(self) -> None:
        # Update account state
        await self._update_account_state()

        # Connect to websocket
        await self._ws_client.connect()

        # Subscribe to account, positions
        # await self._ws_client.subscribe_account()
        # await self._ws_client.subscribe_fills()  # "orders" channel provides fills
        await self._ws_client.subscribe_orders()
        # await self._ws_client.subscribe_positions()  # nautilus uses fills to updt pos

    async def _disconnect(self) -> None:
        await self._ws_client.disconnect()

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_reports(  # noqa: C901
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        reports: list[OrderStatusReport] = []
        ordIds: list[str] = []  # for possible pagination in fetch order history

        _symbol = command.instrument_id.symbol.value if command.instrument_id is not None else None
        symbol = OKXSymbol(_symbol) if _symbol is not None else None
        try:
            if symbol:
                orders_pending_response = await self._http_trade.fetch_orders_pending(
                    instType=symbol.instrument_type,
                    instId=symbol.raw_symbol,
                )
                for pending_order in orders_pending_response.data:
                    client_order_id = self._client_order_id_generator.get_client_order_id(
                        pending_order.clOrdId,
                    )
                    report = pending_order.parse_to_order_status_report(
                        account_id=self.account_id,
                        instrument_id=symbol.to_instrument_id(),
                        report_id=UUID4(),
                        enum_parser=self._enum_parser,
                        ts_init=self._clock.timestamp_ns(),
                        client_order_id=client_order_id,
                    )
                    self._log.debug(f"Received {report}")
                    reports.append(report)

                # Gets latest 3 months of order history, up to a max of 100 records
                hist_order_response = await self._http_trade.fetch_orders_pending(
                    instType=symbol.instrument_type,
                    instId=symbol.raw_symbol,
                )
                for hist_order in hist_order_response.data:
                    client_order_id = self._client_order_id_generator.get_client_order_id(
                        hist_order.clOrdId,
                    )
                    report = hist_order.parse_to_order_status_report(
                        account_id=self.account_id,
                        instrument_id=symbol.to_instrument_id(),
                        report_id=UUID4(),
                        enum_parser=self._enum_parser,
                        ts_init=self._clock.timestamp_ns(),
                        client_order_id=client_order_id,
                    )
                    self._log.debug(f"Received {report}")
                    reports.append(report)
                    ordIds.append(hist_order.ordId)

                if not reports:
                    len_reports = len(reports)
                    plural = "" if len_reports == 1 else "s"
                    self._log.info(f"Received {len(reports)} OrderStatusReport{plural}")
                    return reports

                if command.start:
                    # Maybe paginate to get the remaining history
                    while unix_nanos_to_dt(reports[-1].ts_event) > command.start:  # -1 is oldest
                        hist_order_response = await self._http_trade.fetch_orders_pending(
                            instType=symbol.instrument_type,
                            instId=symbol.raw_symbol,
                            after=ordIds[-1],
                        )
                        for hist_order in hist_order_response.data:
                            client_order_id = self._client_order_id_generator.get_client_order_id(
                                hist_order.clOrdId,
                            )
                            report = hist_order.parse_to_order_status_report(
                                account_id=self.account_id,
                                instrument_id=symbol.to_instrument_id(),
                                report_id=UUID4(),
                                enum_parser=self._enum_parser,
                                ts_init=self._clock.timestamp_ns(),
                                client_order_id=client_order_id,
                            )
                            self._log.debug(f"Received {report}")
                            reports.append(report)
                            ordIds.append(hist_order.ordId)
            else:
                for instrument_type, contract_type in self._inst_type_contr_type_combos:
                    _reports = []
                    _ordIds = []
                    orders_pending_response = await self._http_trade.fetch_orders_pending(
                        instType=instrument_type,
                    )
                    for pending_order in orders_pending_response.data:
                        okx_symbol = OKXSymbol.from_raw_symbol(
                            pending_order.instId,
                            instrument_type,
                            contract_type,
                        )
                        client_order_id = self._client_order_id_generator.get_client_order_id(
                            pending_order.clOrdId,
                        )
                        report = pending_order.parse_to_order_status_report(
                            account_id=self.account_id,
                            instrument_id=okx_symbol.to_instrument_id(),
                            report_id=UUID4(),
                            enum_parser=self._enum_parser,
                            ts_init=self._clock.timestamp_ns(),
                            client_order_id=client_order_id,
                        )
                        self._log.debug(f"Received {report}")
                        _reports.append(report)

                    # Gets latest 3 months of order history, up to a max of 100 records
                    hist_order_response = await self._http_trade.fetch_orders_pending(
                        instType=instrument_type,
                    )
                    for hist_order in hist_order_response.data:
                        okx_symbol = OKXSymbol.from_raw_symbol(
                            hist_order.instId,
                            instrument_type,
                            contract_type,
                        )
                        client_order_id = self._client_order_id_generator.get_client_order_id(
                            hist_order.clOrdId,
                        )
                        report = hist_order.parse_to_order_status_report(
                            account_id=self.account_id,
                            instrument_id=okx_symbol.to_instrument_id(),
                            report_id=UUID4(),
                            enum_parser=self._enum_parser,
                            ts_init=self._clock.timestamp_ns(),
                            client_order_id=client_order_id,
                        )
                        self._log.debug(f"Received {report}")
                        _reports.append(report)
                        _ordIds.append(hist_order.ordId)

                    if not _reports:
                        continue

                    if command.start:
                        # Maybe paginate to get the remaining history
                        while (
                            unix_nanos_to_dt(_reports[-1].ts_event) > command.start
                        ):  # -1 is oldest
                            hist_order_response = await self._http_trade.fetch_orders_pending(
                                instType=instrument_type,
                                after=_ordIds[-1],
                            )
                            for hist_order in hist_order_response.data:
                                okx_symbol = OKXSymbol.from_raw_symbol(
                                    hist_order.instId,
                                    instrument_type,
                                    contract_type,
                                )
                                client_order_id = (
                                    self._client_order_id_generator.get_client_order_id(
                                        hist_order.clOrdId,
                                    )
                                )
                                report = hist_order.parse_to_order_status_report(
                                    account_id=self.account_id,
                                    instrument_id=okx_symbol.to_instrument_id(),
                                    report_id=UUID4(),
                                    enum_parser=self._enum_parser,
                                    ts_init=self._clock.timestamp_ns(),
                                    client_order_id=client_order_id,
                                )
                                self._log.debug(f"Received {report}")
                                _reports.append(report)
                                _ordIds.append(hist_order.ordId)

                    # Add instrument_type/contract_type `_reports` to main `reports` list
                    reports.extend(_reports)

        except Exception as e:
            self._log.exception("Failed to generate OrderStatusReports", e)

        # Deduplicate reports by venue_order_id, taking the latest by (ts_accepted, ts_init)
        dedup_report_dict: dict[VenueOrderId, OrderStatusReport] = {}
        for r in reports:
            if r.venue_order_id not in dedup_report_dict:
                dedup_report_dict[r.venue_order_id] = r
            elif r.ts_accepted > dedup_report_dict[r.venue_order_id].ts_accepted:
                dedup_report_dict[r.venue_order_id] = r
            elif r.ts_init > dedup_report_dict[r.venue_order_id].ts_init:
                dedup_report_dict[r.venue_order_id] = r

        # Sort and filter by `ts_accepted` because `cTime` (order creation/acceptance time) is the
        # basis of OKX's chronological sorting of orders
        reports = sorted(dedup_report_dict.values(), key=lambda r: r.ts_accepted)
        if command.start:
            reports = list(
                filter(lambda r: command.start <= unix_nanos_to_dt(r.ts_accepted), reports),
            )
        if command.end:
            reports = list(
                filter(lambda r: unix_nanos_to_dt(r.ts_accepted) <= command.end, reports),
            )

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

        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)
        okx_client_order_id = self._client_order_id_generator.get_okx_client_order_id(
            command.client_order_id,
        )

        id_str = f"{command.instrument_id!r}"
        id_str += f", {command.client_order_id!r}" if command.client_order_id else ""
        id_str += f", {command.venue_order_id!r}" if command.venue_order_id else ""
        id_str += f", {okx_client_order_id=!r}" if okx_client_order_id else ""
        self._log.info(f"Generating OrderStatusReport for {id_str}")

        try:
            order_details = await self._http_trade.fetch_order_details(
                instId=okx_symbol.raw_symbol,
                ordId=command.venue_order_id.value if command.venue_order_id else None,
                clOrdId=okx_client_order_id,
            )
            if len(order_details.data) == 0:
                self._log.error(f"Received no order for {id_str}")
                return None
            if len(order_details.data) > 1:
                self._log.warning(
                    f"Received more than one order for {id_str}, using the first for the report...",
                )

            target_order = order_details.data[0]
            client_order_id = self._client_order_id_generator.get_client_order_id(
                target_order.clOrdId,
            )
            venue_order_id = VenueOrderId(target_order.ordId)
            if client_order_id is None:
                client_order_id = self._cache.client_order_id(venue_order_id)

            order_report = target_order.parse_to_order_status_report(
                account_id=self.account_id,
                instrument_id=command.instrument_id,
                report_id=UUID4(),
                enum_parser=self._enum_parser,
                ts_init=self._clock.timestamp_ns(),
                client_order_id=client_order_id,
            )
            self._log.debug(f"Received {order_report}", LogColor.MAGENTA)
            return order_report
        except Exception as e:
            self._log.exception("Failed to generate OrderStatusReport", e)

        return None

    async def generate_fill_reports(  # noqa: C901
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        self._log.debug("Requesting FillReports...")
        reports: list[FillReport] = []
        billIds: list[str] = []  # for possible pagination

        _symbol = command.instrument_id.symbol.value if command.instrument_id is not None else None
        symbol = OKXSymbol(_symbol) if _symbol is not None else None
        try:
            if symbol:
                # Gets latest 3 months of fills, up to a max of 100 records per request
                fills_response = await self._http_trade.fetch_fills_history(
                    instType=symbol.instrument_type,
                    instId=symbol.raw_symbol,
                    ordId=command.venue_order_id.value if command.venue_order_id else None,
                )
                for fill_data in fills_response.data:
                    client_order_id = self._client_order_id_generator.get_client_order_id(
                        fill_data.clOrdId,
                    )
                    report = fill_data.parse_to_fill_report(
                        account_id=self.account_id,
                        instrument_id=symbol.to_instrument_id(),
                        report_id=UUID4(),
                        client_order_id=client_order_id,
                        enum_parser=self._enum_parser,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    self._log.debug(f"Received {report}")
                    reports.append(report)
                    billIds.append(fill_data.billId)

                if not reports:
                    len_reports = len(reports)
                    plural = "" if len_reports == 1 else "s"
                    self._log.info(f"Received {len(reports)} FillReport{plural}")
                    return reports

                if command.start:
                    # Maybe paginate to get the remaining history
                    while unix_nanos_to_dt(reports[-1].ts_event) > command.start:  # -1 is oldest
                        fills_response = await self._http_trade.fetch_fills_history(
                            instType=symbol.instrument_type,
                            instId=symbol.raw_symbol,
                            ordId=command.venue_order_id.value if command.venue_order_id else None,
                            after=billIds[-1],
                        )
                        for fill_data in fills_response.data:
                            client_order_id = self._client_order_id_generator.get_client_order_id(
                                fill_data.clOrdId,
                            )
                            report = fill_data.parse_to_fill_report(
                                account_id=self.account_id,
                                instrument_id=symbol.to_instrument_id(),
                                report_id=UUID4(),
                                client_order_id=client_order_id,
                                enum_parser=self._enum_parser,
                                ts_init=self._clock.timestamp_ns(),
                            )
                            self._log.debug(f"Received {report}")
                            reports.append(report)
                            billIds.append(fill_data.billId)
            else:
                for instrument_type, contract_type in self._inst_type_contr_type_combos:
                    _reports = []
                    _billIds = []
                    # Gets latest 3 months of fills, up to a max of 100 records per request
                    fills_response = await self._http_trade.fetch_fills_history(
                        instType=instrument_type,
                        ordId=command.venue_order_id.value if command.venue_order_id else None,
                    )
                    for fill_data in fills_response.data:
                        okx_symbol = OKXSymbol.from_raw_symbol(
                            fill_data.instId,
                            instrument_type,
                            contract_type,
                        )
                        client_order_id = self._client_order_id_generator.get_client_order_id(
                            fill_data.clOrdId,
                        )
                        report = fill_data.parse_to_fill_report(
                            account_id=self.account_id,
                            instrument_id=okx_symbol.to_instrument_id(),
                            report_id=UUID4(),
                            client_order_id=client_order_id,
                            enum_parser=self._enum_parser,
                            ts_init=self._clock.timestamp_ns(),
                        )
                        self._log.debug(f"Received {report}")
                        _reports.append(report)
                        _billIds.append(fill_data.billId)

                    if not _reports:
                        continue

                    if command.start:
                        # Maybe paginate to get the remaining history
                        while (
                            unix_nanos_to_dt(_reports[-1].ts_event) > command.start
                        ):  # -1 is oldest
                            fills_response = await self._http_trade.fetch_fills_history(
                                instType=instrument_type,
                                ordId=(
                                    command.venue_order_id.value if command.venue_order_id else None
                                ),
                                after=_billIds[-1],
                            )
                            for fill_data in fills_response.data:
                                client_order_id = (
                                    self._client_order_id_generator.get_client_order_id(
                                        fill_data.clOrdId,
                                    )
                                )
                                report = fill_data.parse_to_fill_report(
                                    account_id=self.account_id,
                                    instrument_id=okx_symbol.to_instrument_id(),
                                    report_id=UUID4(),
                                    client_order_id=client_order_id,
                                    enum_parser=self._enum_parser,
                                    ts_init=self._clock.timestamp_ns(),
                                )
                                self._log.debug(f"Received {report}")
                                _reports.append(report)
                                _billIds.append(fill_data.billId)

                    # Add instrument_type/contract_type `_reports` to main `reports` list
                    reports.extend(_reports)

        except Exception as e:
            self._log.exception("Failed to generate FillReports", e)

        reports = sorted(reports, key=lambda report: report.ts_event)
        if command.start:
            reports = list(filter(lambda r: command.start <= unix_nanos_to_dt(r.ts_event), reports))
        if command.end:
            reports = list(filter(lambda r: unix_nanos_to_dt(r.ts_event) <= command.end, reports))

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} FillReport{plural}")

        return reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        reports: list[PositionStatusReport] = []

        try:
            if command.instrument_id:
                self._log.debug(f"Requesting PositionStatusReport for {command.instrument_id}")
                okx_symbol = OKXSymbol(command.instrument_id.symbol.value)
                positions_response = await self._http_account.fetch_positions(
                    instType=okx_symbol.instrument_type,
                    instId=okx_symbol.raw_symbol,
                )
                for position in positions_response.data:
                    position_report = position.parse_to_position_status_report(
                        account_id=self.account_id,
                        instrument_id=command.instrument_id,
                        report_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                    )
                    self._log.debug(f"Received {position_report}")
                    reports.append(position_report)
            else:
                self._log.debug("Requesting PositionStatusReports...")
                for instrument_type, contract_type in self._inst_type_contr_type_combos:
                    positions_response = await self._http_account.fetch_positions(
                        instType=instrument_type,
                    )
                    for position in positions_response.data:
                        okx_symbol = OKXSymbol.from_raw_symbol(
                            position.instId,
                            instrument_type,
                            contract_type,
                        )
                        position_report = position.parse_to_position_status_report(
                            account_id=self.account_id,
                            instrument_id=okx_symbol.to_instrument_id(),
                            report_id=UUID4(),
                            ts_init=self._clock.timestamp_ns(),
                        )
                        self._log.debug(f"Received {position_report}")
                        reports.append(position_report)
        except Exception as e:
            self._log.exception("Failed to generate PositionReports", e)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} PositionReport{plural}")

        return reports

    def _create_okx_order_msg_id(self) -> str:
        # OKX requires order message id's to be <= 32 characters with numbers & letters only
        # UUID.hex meets this definition
        return UUID(str(UUID4())).hex

    def _get_cache_active_symbols(self) -> set[str]:
        # Check cache for all active orders
        open_orders: list[Order] = self._cache.orders_open(venue=self.venue)
        open_positions: list[Position] = self._cache.positions_open(venue=self.venue)
        active_symbols: set[str] = set()
        for order in open_orders:
            active_symbols.add(OKXSymbol(order.instrument_id.symbol.value))
        for position in open_positions:
            active_symbols.add(OKXSymbol(position.instrument_id.symbol.value))
        return active_symbols

    def _determine_order_type(self, order: Order) -> OKXOrderType:
        if isinstance(order, MarketOrder):
            return OKXOrderType.MARKET
        if order.is_post_only:
            return OKXOrderType.POST_ONLY
        time_in_force: TimeInForce = order.time_in_force
        if time_in_force not in self._enum_parser.valid_time_in_force:
            raise RuntimeError(
                f"Invalid time in force {time_in_force}, unsupported by OKX. Supported times in "
                f"force: {self._enum_parser.valid_time_in_force}",
            )
        match time_in_force:
            case TimeInForce.GTC:
                return OKXOrderType.LIMIT  # OKX limit orders are GTC by default
            case TimeInForce.FOK:
                return OKXOrderType.FOK
            case TimeInForce.IOC:
                return OKXOrderType.IOC
            case _:
                raise RuntimeError(
                    f"Could not determine OKX order type from order {order}, valid OKX order types "
                    f"are: {list(OKXOrderType)}",
                )

    async def _get_active_position_symbols(self, symbol: str | None) -> set[str]:
        active_symbols: set[str] = set()
        for instrument_type in self._instrument_types:
            positions_response = await self._http_account.fetch_positions(
                instType=instrument_type,
                instId=symbol,
            )
            for position in positions_response.data:
                active_symbols.add(position.instId)
        return active_symbols

    async def _update_account_state(self) -> None:
        balances_data = await self._http_account.fetch_balance()
        balances = []
        margins = []

        try:
            balances.append(balances_data.parse_to_account_balance())
            margins.append(balances_data.parse_to_margin_balance())
        except Exception as e:
            self._log.error(
                f"Failed to generate AccountState for balance data {balances_data}: {e}",
            )
            raise e

        for asset_details in balances_data.details:
            try:
                _balances = asset_details.parse_to_account_balance()
                _margins = asset_details.parse_to_margin_balance()

                if _balances:
                    balances.append(_balances)
                if _margins:
                    margins.append(_margins)

            except Exception as e:
                self._log.error(
                    f"Failed to generate AccountState for asset details {asset_details}: {e}",
                )
                continue

        self.generate_account_state(
            balances=balances,
            margins=margins,
            reported=True,
            ts_event=millis_to_nanos(int(balances_data.uTime)),
        )

        # TODO: need to update instrument leverages?
        # while self.get_account() is None:
        #     await asyncio.sleep(0.1)

        # account: MarginAccount = self.get_account()
        # position_risks = await self._futures_http_account.query_futures_position_risk()
        # for position in position_risks:
        #     instrument_id: InstrumentId = self._get_cached_instrument_id(position.symbol)
        #     leverage = Decimal(position.leverage)
        #     account.set_leverage(instrument_id, leverage)
        #     self._log.debug(f"Set leverage {position.symbol} {leverage}X")

    async def _modify_order(self, command: ModifyOrder) -> None:
        if command.trigger_price:
            self._log.error(
                f"ModifyOrder command has {command.trigger_price=} but stop-type orders are not "
                "yet supported for OKX",
            )
            return

        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)

        order: Order | None = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id=} not found in cache to enable modifying")
            return
        if not self._check_order_validity(order, okx_symbol.instrument_type):
            return

        venue_order_id = str(command.venue_order_id) if command.venue_order_id else None
        price = str(command.price) if command.price else None
        # trigger_price = str(command.trigger_price) if command.trigger_price else None
        quantity = str(command.quantity) if command.quantity else None

        okx_client_order_id = self._client_order_id_generator.get_okx_client_order_id(
            command.client_order_id,
        )

        msg_id = self._create_okx_order_msg_id()
        self._unhandled_order_msgs[msg_id] = (
            okx_symbol,
            command.venue_order_id,
            command.client_order_id,
            command.strategy_id,
        )

        # NOTE: when amending a partially filled order, newSz should include partially filled amount
        # This assumes `quantity` does so as we have no

        await self._ws_client.amend_order(
            msg_id=msg_id,
            instId=okx_symbol.raw_symbol,
            cxlOnFail=False,  # TODO: add to exec config?
            ordId=venue_order_id,
            clOrdId=okx_client_order_id,
            reqId=None,
            newSz=quantity,
            newPx=price,
            expTime=None,
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)

        order: Order | None = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id=} not found in cache to enable canceling")
            return
        if not self._check_order_validity(order, okx_symbol.instrument_type):
            return

        venue_order_id = str(command.venue_order_id) if command.venue_order_id else None

        okx_client_order_id = self._client_order_id_generator.get_okx_client_order_id(
            command.client_order_id,
        )

        msg_id = self._create_okx_order_msg_id()
        self._unhandled_order_msgs[msg_id] = (
            okx_symbol,
            command.venue_order_id,
            command.client_order_id,
            command.strategy_id,
        )

        await self._ws_client.cancel_order(
            msg_id=msg_id,
            instId=okx_symbol.raw_symbol,
            ordId=venue_order_id,
            clOrdId=okx_client_order_id,
        )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)

        open_orders_strategy: list[Order] = self._cache.orders_open(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )

        for order in open_orders_strategy:
            if not self._check_order_validity(order, okx_symbol.instrument_type):
                continue
            venue_order_id = str(order.venue_order_id) if order.venue_order_id else None

            okx_client_order_id = self._client_order_id_generator.get_okx_client_order_id(
                order.client_order_id,
            )
            msg_id = self._create_okx_order_msg_id()
            self._unhandled_order_msgs[msg_id] = (
                okx_symbol,
                order.venue_order_id,
                order.client_order_id,
                order.strategy_id,
            )

            await self._ws_client.cancel_order(
                msg_id=msg_id,
                instId=okx_symbol.raw_symbol,
                ordId=venue_order_id,
                clOrdId=okx_client_order_id,
            )

    def _check_order_validity(self, order: Order, instrument_type: OKXInstrumentType) -> bool:
        if order.order_type not in self._submit_order_methods:
            self._log.error(f"Cannot submit order, {order.order_type=} not yet implemented for OKX")
            return False

        if order.is_closed:
            self._log.error(
                f"Cannot submit order {order}, already closed: {order.status_string()=}",
            )
            return False

        if order.is_post_only and order.order_type != OrderType.LIMIT:
            self._log.error(
                f"Cannot submit order {order}, is post-only with non-limit type {order.order_type=}",
            )
            return False

        # Check reduce only
        if order.is_reduce_only:
            if not (
                self._trade_mode == OKXTradeMode.CROSS
                or (
                    instrument_type in [OKXInstrumentType.SWAP, OKXInstrumentType.FUTURES]
                    and self._position_side == OKXPositionSide.NET
                )
            ):
                self._log.error(
                    f"Cannot submit reduce-only {order}, OKX reduce-only orders are only "
                    f"applicable to cross-margin trading modes ({OKXTradeMode.CROSS}) or "
                    f"SWAP/FUTURES instrument types with an OKX position side of net "
                    f"({OKXPositionSide.NET}). Instrument type is {instrument_type}, trading mode "
                    f"is {self._trade_mode}, and position side is {self._position_side}",
                )
                return False

        return True

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        self._log.debug(f"Submit list of {len(command.order_list.orders)} orders", LogColor.CYAN)

        for order in command.order_list.orders:
            order = command.order
            okx_symbol = OKXSymbol(command.instrument_id.symbol.value)
            if not self._check_order_validity(order, okx_symbol.instrument_type):  # logs reason
                continue

            # Generate order submitted event, to ensure correct ordering of event
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=self._clock.timestamp_ns(),
            )
            await self._submit_order_methods[order.order_type](order)

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order
        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)
        if not self._check_order_validity(order, okx_symbol.instrument_type):
            return

        self._log.debug(f"Submitting order {order}")

        # Generate order submitted event, to ensure correct ordering of event
        self._log.debug(
            f"Order submission info: {order.venue_order_id=}, {order.client_order_id=}, {order.strategy_id=}",
        )
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )
        await self._submit_order_methods[order.order_type](order)

    async def _submit_market_order(self, order: MarketOrder) -> None:
        okx_symbol = OKXSymbol(order.instrument_id.symbol.value)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)

        okx_client_order_id = self._client_order_id_generator.generate_okx_client_order_id(
            order.client_order_id,
        )

        msg_id = self._create_okx_order_msg_id()
        self._unhandled_order_msgs[msg_id] = (
            okx_symbol,
            None,
            order.client_order_id,
            order.strategy_id,
        )

        await self._ws_client.place_order(
            msg_id=msg_id,
            instId=okx_symbol.raw_symbol,
            tdMode=self._trade_mode,
            side=order_side,
            ordType=OKXOrderType.MARKET,
            sz=str(order.quantity),
            ccy=None,  # margin currency, only applicable to SPOT/FUTURES mode in CROSS tdMode
            px=None,
            reduceOnly=order.is_reduce_only,
            posSide=self._position_side,
            expTime=None,
            clOrdId=okx_client_order_id,
            tag=", ".join(order.tags) if order.tags else None,
        )

    async def _submit_limit_order(self, order: LimitOrder) -> None:
        okx_symbol = OKXSymbol(order.instrument_id.symbol.value)
        order_type = self._determine_order_type(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)

        # Http place order - websocket should be faster
        # await self._http_trade.place_order(
        #     instId=okx_symbol.raw_symbol,
        #     tdMode=self._trade_mode,
        #     side=order_side,
        #     ordType=order_type,
        #     sz=str(order.quantity),
        #     ccy=None,  # margin currency, only applicable to SPOT/FUTURES mode in CROSS tdMode
        #     clOrdId=str(order.client_order_id),
        #     tag=", ".join(order.tags) if order.tags else None,
        #     posSide=self._position_side,
        #     px=str(order.price),
        #     reduceOnly=order.is_reduce_only,
        # )

        okx_client_order_id = self._client_order_id_generator.generate_okx_client_order_id(
            order.client_order_id,
        )

        msg_id = self._create_okx_order_msg_id()
        self._unhandled_order_msgs[msg_id] = (
            okx_symbol,
            None,
            order.client_order_id,
            order.strategy_id,
        )

        # Websocket place order
        await self._ws_client.place_order(
            msg_id=msg_id,
            instId=okx_symbol.raw_symbol,
            tdMode=self._trade_mode,
            side=order_side,
            ordType=order_type,
            sz=str(order.quantity),
            ccy=None,  # margin currency, only applicable to SPOT/FUTURES mode in CROSS tdMode
            px=str(order.price),
            reduceOnly=order.is_reduce_only,
            posSide=self._position_side,
            clOrdId=okx_client_order_id,
            tag=", ".join(order.tags) if order.tags else None,
            expTime=None,
        )

    async def _submit_stop_market_order(self, order: StopMarketOrder) -> None:
        raise NotImplementedError("Stop-market orders are not yet implemented for OKX")

    async def _submit_stop_limit_order(self, order: StopLimitOrder) -> None:
        raise NotImplementedError("Stop-limit orders are not yet implemented for OKX")

    async def _submit_market_if_touched_order(self, order: MarketIfTouchedOrder) -> None:
        raise NotImplementedError("Market-if-touched orders are not yet implemented for OKX")

    async def _submit_limit_if_touched_order(self, order: LimitIfTouchedOrder) -> None:
        raise NotImplementedError("Limit-if-touched orders are not yet implemented for OKX")

    async def _submit_trailing_stop_market(self, order: TrailingStopMarketOrder) -> None:
        raise NotImplementedError("Trailing-stop orders are not yet implemented for OKX")

    def _handle_ws_message(self, raw: bytes) -> None:  # noqa: C901
        # Uncomment for development
        # self._log.info(str(json.dumps(msgspec.json.decode(raw), indent=4)), color=LogColor.MAGENTA)

        if raw == b"pong":
            self._ws_client._last_pong = self._clock.utc_now()
            return

        try:
            msg = self._decoder_ws_general_msg.decode(raw)
        except Exception as e:
            self._log.error(
                f"Failed to decode websocket general message: {raw.decode()} with error {e}",
            )
            return

        channel: str | None
        try:
            if msg.is_event_msg:
                try:
                    event_msg = self._decoder_ws_event_msg.decode(raw)
                except Exception as e:
                    self._log.exception(
                        f"Failed to decode websocket event data message: {raw.decode()}",
                        e,
                    )
                    return
                if event_msg.is_login:
                    self._log.info("Login succeeded", LogColor.GREEN)
                    return

                if event_msg.is_subscribe_unsubscribe:
                    self._log.info(
                        f"Got subscribe/unsubscribe event msg: {event_msg}",
                        LogColor.GREEN,
                    )
                    return

                if event_msg.is_channel_conn_count_error:
                    error_str = event_msg.format_channel_conn_count_error()
                    self._log.warning(
                        f"Received websocket channel connection count error: {error_str}. The last "
                        "connection was likely rejected and OKX may in rare cases unsubscribe "
                        "existing connections.",
                    )
                    return

                if event_msg.is_error:
                    error_str = event_msg.format_error()
                    self._log.error(f"Received websocket error: {error_str}")
                    return

                if event_msg.connCount is not None:
                    channel = event_msg.channel  # channel won't be None here
                    if channel:
                        ws_base_url_type = OKX_CHANNEL_WS_BASE_URL_TYPE_MAP[channel]
                        assert self._ws_client.ws_base_url_type == ws_base_url_type, (
                            "The websocket client's base url type does not match the expected base url "
                            f"type for this channel ({channel}), got client type: {ws_base_url_type=} "
                            f"vs. channel inferred type: {ws_base_url_type}."
                        )
                        self._ws_client.update_channel_count(channel, int(event_msg.connCount))

            elif msg.is_push_data_msg:
                try:
                    push_data = self._decoder_ws_push_data_msg.decode(raw)
                except Exception as e:
                    self._log.exception(
                        f"Failed to decode websocket push data message: {raw.decode()}",
                        e,
                    )
                    return

                channel = push_data.arg.channel

                EXEC_CLIENT_SUPPORTED_PUSH_DATA_CHANNELS = [
                    "account",
                    "fills",
                    "orders",
                    "positions",
                ]
                if channel not in EXEC_CLIENT_SUPPORTED_PUSH_DATA_CHANNELS:
                    self._log.error(
                        f"Received message from channel {channel}. Is this intended for the "
                        f"execution client? Current supported exec client push data channels: "
                        f"{EXEC_CLIENT_SUPPORTED_PUSH_DATA_CHANNELS}. Raw message: {raw.decode()}",
                    )
                    return

                if channel == "account":
                    self._handle_account(raw)
                elif channel == "fills":
                    self._handle_fills(raw)
                elif channel == "orders":
                    self._handle_orders(raw)
                elif channel == "positions":
                    self._handle_positions(raw)
                else:
                    self._log.error(
                        "Unknown or unsupported websocket push data message with channel: "
                        f"{channel}",
                    )
                    return
            elif msg.is_order_msg:
                self._handle_order_msg(raw)

            elif msg.is_algo_order_msg:
                self._log.error(
                    "Handling algo order websocket messages is not yet implemented. Raw message: "
                    f"{raw.decode()}",
                )
            else:
                self._log.error(
                    f"Cannot handle unknown or unsupported websocket message: {raw.decode()}",
                )
        except Exception as e:
            self._log.exception(f"Got error for raw message: {raw.decode()}", e)

    def _handle_account(self, raw: bytes) -> None:
        try:
            account_push_data: OKXWsAccountPushDataMsg = self._decoder_ws_account_msg.decode(raw)
        except Exception as e:
            self._log.error(
                f"Failed to decode websocket account push data message: {raw.decode()} with error "
                f"{e}",
            )
            return

        for balances_data in account_push_data.data:
            balances = []
            margins = []

            try:
                balances.append(balances_data.parse_to_account_balance())
                margins.append(balances_data.parse_to_margin_balance())
            except Exception as e:
                self._log.exception(
                    f"Failed to generate AccountState for balance data {balances_data}",
                    e,
                )
                continue

            for asset_details in balances_data.details:
                try:
                    _balances = asset_details.parse_to_account_balance()
                    _margins = asset_details.parse_to_margin_balance()

                    if _balances:
                        balances.append(_balances)
                    if _margins:
                        margins.append(_margins)

                except Exception as e:
                    self._log.exception(
                        f"Failed to generate AccountState for asset details {asset_details}",
                        e,
                    )
                    continue

            self.generate_account_state(
                balances=balances,
                margins=margins,
                reported=True,
                ts_event=millis_to_nanos(int(balances_data.uTime)),
            )

    def _handle_fills(self, raw: bytes) -> None:
        try:
            fills_push_data: OKXWsFillsPushDataMsg = self._decoder_ws_fills_msg.decode(raw)
        except Exception as e:
            self._log.error(
                f"Failed to decode websocket fills push data message: {raw.decode()} with error "
                f"{e}",
            )
            return

        for fill in fills_push_data.data:
            self._log.info(f"Got fill data: {raw.decode()}", LogColor.MAGENTA)

            # Find instrument
            instrument = self._instrument_provider.find_conditional(fill.instId)
            if instrument is None:
                self._log.error(
                    f"Could not find instrument for raw symbol {fill.instId!r}, which is needed to "
                    f"correctly parse fill push data message: {raw.decode()}",
                )
                continue

            client_order_id = self._cache.client_order_id(VenueOrderId(fill.ordId))
            order: Order | None = self._cache.order(client_order_id)
            if order is None:
                self._log.error(
                    f"Cannot generate fill event for {client_order_id!r}, order not found to in "
                    "cache for locating its order type",
                )
                return
            strategy_id = self._cache.strategy_id_for_order(client_order_id)

            # NOTE no fee in fills data to make non-None commission for order filled event
            # fee_currency = Currency.from_str(self.feeCcy or "USDT")
            # fee = format(float(self.fee or 0), f".{fee_currency.precision}f")
            # commission = Money(Decimal(fee), fee_currency)

            report = fill.parse_to_fill_report(
                account_id=self.account_id,
                instrument_id=instrument.id,
                report_id=UUID4(),
                enum_parser=self._enum_parser,
                ts_init=self._clock.timestamp_ns(),
                client_order_id=client_order_id,
                commission=Money(0, instrument.quote_currency),
            )
            self.generate_order_filled(
                strategy_id=strategy_id,
                instrument_id=instrument.id,
                client_order_id=client_order_id,
                venue_order_id=report.venue_order_id,
                venue_position_id=None,
                trade_id=report.trade_id,
                order_side=report.order_side,
                order_type=order.order_type,
                last_qty=report.last_qty,
                last_px=report.last_px,
                quote_currency=instrument.quote_currency,
                commission=report.commission,
                liquidity_side=report.liquidity_side,
                ts_event=report.ts_event,
            )

    def _handle_orders(self, raw: bytes) -> None:  # noqa: C901
        try:
            orders_push_data: OKXWsOrdersPushDataMsg = self._decoder_ws_orders_msg.decode(raw)
        except Exception as e:
            self._log.error(
                f"Failed to decode websocket orders push data message: {raw.decode()} with error "
                f"{e}",
            )
            return

        for order_data in orders_push_data.data:
            # Find instrument
            instrument = self._instrument_provider.find_conditional(order_data.instId)
            if instrument is None:
                self._log.error(
                    f"Could not find instrument for raw symbol {order_data.instId!r}, which is "
                    f"needed to correctly parse orders push data message: {raw.decode()}",
                )
                continue

            client_order_id = self._client_order_id_generator.get_client_order_id(
                order_data.clOrdId,
            )

            venue_order_id = VenueOrderId(order_data.ordId)

            position_id = None
            if client_order_id:
                position_id = self._cache.position_id(client_order_id)

            strategy_id = None
            if client_order_id:
                strategy_id = self._cache.strategy_id_for_order(client_order_id)

            if position_id and not strategy_id:
                strategy_id = self._cache.strategy_id_for_position(position_id)

            ts_event = millis_to_nanos(Decimal(order_data.uTime))

            report = order_data.parse_to_order_status_report(
                account_id=self.account_id,
                instrument_id=instrument.id,
                report_id=UUID4(),
                enum_parser=self._enum_parser,
                ts_init=self._clock.timestamp_ns(),
                client_order_id=client_order_id,
            )

            if not strategy_id:
                self._log.debug(
                    "Cannot generate order event because cache does not contain strategy id for "
                    "orders data. This is likely an EXTERNAL order or is associated with an "
                    "EXTERNAL order that was recovered from reconciliation. Sending order status "
                    f"report. Orders data received: {order_data}",
                    LogColor.MAGENTA,
                )
                # strategy_id here will be inferred as EXTERNAL
                self._send_order_status_report(report)
                return

            if order_data.state is OKXOrderStatus.LIVE:
                order = self._cache.order(report.client_order_id)
                if order is None:
                    self._log.error(
                        "Cannot find order in cache for OrderStatusReport client order id: "
                        f"{report.client_order_id!r}",
                    )
                    return

                venue_order_id_modified = (
                    False if venue_order_id is None else order_data.ordId != str(venue_order_id)
                )
                if order.status == OrderStatus.PENDING_UPDATE:
                    self._log.debug("Generating order updated event", LogColor.MAGENTA)
                    self.generate_order_updated(
                        strategy_id=strategy_id,
                        instrument_id=report.instrument_id,
                        client_order_id=report.client_order_id,
                        venue_order_id=report.venue_order_id,
                        quantity=report.quantity,  # current order quantity
                        price=report.price,  # current order price (or None)
                        trigger_price=report.trigger_price,
                        ts_event=ts_event,
                        venue_order_id_modified=venue_order_id_modified,
                    )
                else:
                    self._log.debug("Generating order accepted event", LogColor.MAGENTA)
                    self.generate_order_accepted(
                        strategy_id=strategy_id,
                        instrument_id=report.instrument_id,
                        client_order_id=report.client_order_id,
                        venue_order_id=report.venue_order_id,
                        ts_event=ts_event,
                    )

            if order_data.state is OKXOrderStatus.CANCELED:
                self._log.debug(
                    f"Generating order canceled event with {order_data.cancel_reason=!r}",
                    LogColor.MAGENTA,
                )
                self.generate_order_canceled(
                    strategy_id=strategy_id,
                    instrument_id=instrument.id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=ts_event,
                )
                return

            if order_data.is_amended:
                channel = orders_push_data.arg.channel
                self._log.info(
                    f"Got order with amendation info from channel: {channel!r}: "
                    f"({order_data.amend_source_reason=!r}, {order_data.amend_result_reason=!r}) "
                    f"and order status {order_data.state!r}. Raw msg: {raw.decode()}",
                    LogColor.MAGENTA,
                )

                if order_data.amend_result_reason and "success" in order_data.amend_result_reason:
                    self.generate_order_updated(
                        strategy_id=strategy_id,
                        instrument_id=report.instrument_id,
                        client_order_id=report.client_order_id,
                        venue_order_id=report.venue_order_id,
                        quantity=report.quantity,
                        price=report.price,
                        trigger_price=report.trigger_price,
                        ts_event=ts_event,
                        venue_order_id_modified=venue_order_id_modified,
                    )
                elif order_data.amend_result_reason and "failure" in order_data.amend_result_reason:
                    self.generate_order_modify_rejected(
                        strategy_id=strategy_id,
                        instrument_id=instrument.id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        reason=f"{order_data.amend_result_reason}/{order_data.amend_source_reason}",
                        ts_event=ts_event,
                    )

                return

            if order_data.state in [OKXOrderStatus.FILLED, OKXOrderStatus.PARTIALLY_FILLED]:
                self._log.debug("Generating order filled event", LogColor.MAGENTA)
                self.generate_order_filled(
                    strategy_id=strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    venue_position_id=position_id,
                    trade_id=TradeId(order_data.tradeId),
                    order_side=self._enum_parser.parse_okx_order_side(order_data.side),
                    order_type=self._enum_parser.parse_okx_order_type(order_data.ordType),
                    last_qty=order_data.get_fill_sz(instrument.size_precision),
                    last_px=order_data.get_fill_px(instrument.price_precision),
                    quote_currency=instrument.quote_currency,
                    commission=Money(
                        Decimal(order_data.fillFee or 0),
                        Currency.from_str(order_data.fillFeeCcy or "USDT"),
                    ),
                    liquidity_side=order_data.execType.parse_to_liquidity_side(),
                    ts_event=ts_event,
                )

    def _handle_positions(self, raw: bytes) -> None:
        self._log.debug(
            "Got positions message. Nothing to do because nautilus updates positions from fills. "
            f"Raw message: {raw.decode()}",
        )
        # try:
        #     positions_push_data: OKXWsPositionsPushDataMsg = self._decoder_ws_positions_msg.decode(
        #         raw
        #     )
        # except Exception as e:
        #     self._log.error(
        #         f"Failed to decode websocket positions push data message: {raw.decode()} with "
        #         f"error {e}"
        #     )
        #     return
        # for position_data in positions_push_data.data:
        #     # Find instrument
        #     instrument = self._instrument_provider.find_conditional(position_data.instId)
        #     if instrument is None:
        #         self._log.error(
        #             f"Could not find instrument for raw symbol {position_data.instId!r}, which is "
        #             f"needed to correctly parse positions push data message: {raw.decode()}"
        #         )
        #         continue

        #     position_report = position_data.parse_to_position_status_report(
        #         account_id=self.account_id,
        #         instrument_id=instrument.id,
        #         report_id=UUID4(),
        #         ts_init=self._clock.timestamp_ns(),
        #     )

    def _handle_order_msg(self, raw: bytes) -> None:
        try:
            order_msg: OKXWsOrderMsg = self._decoder_ws_order_msg.decode(raw)
        except Exception as e:
            self._log.exception(f"Failed to decode websocket order message: {raw.decode()}", e)
            return

        self._log.debug(f"Got order msg: {order_msg}", LogColor.MAGENTA)
        cached_order_msg = self._unhandled_order_msgs.pop(order_msg.id, (None, None, None, None))
        okx_symbol, venue_order_id, client_order_id, strategy_id = cached_order_msg

        if not strategy_id:
            self._log.debug(
                "Cannot process order message due to missing strategy id in the client's hot cache "
                f"for unhandled order messages. Order msg received: {order_msg}",
            )
            return

        if okx_symbol is None:
            self._log.debug(
                "Cannot process order message due to missing okx symbol in the client's hot cache "
                f"for unhandled order messages. Order msg received: {order_msg}",
            )
            return

        instrument_id = okx_symbol.to_instrument_id()

        if order_msg.code != "0":  # failed attempt to place-order/amend-order/cancel-order
            op_str = "place-order" if order_msg.op == "order" else order_msg.op
            rejection_type = (
                "cancel"
                if order_msg.op == "cancel-order "
                else "modify" if order_msg.op == "amend-order " else ""
            )
            self._log.error(
                f"Received error response for order op {op_str!r} with cached submission "
                f"info: ({instrument_id=}, {venue_order_id=}, {client_order_id=}). "
                f"Generating order {rejection_type}rejected event. Raw message: "
                f"{raw.decode()}",
            )

            ts_event = millis_to_nanos(int(order_msg.outTime))

            order_msg_data: OKXWsOrderMsgData = next(iter(order_msg.data))

            if order_msg.op == "cancel-order":
                self.generate_order_cancel_rejected(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    reason=order_msg_data.rejection_reason,
                    ts_event=ts_event,
                )
            if order_msg.op == "amend-order":
                self.generate_order_modify_rejected(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    reason=order_msg_data.rejection_reason,
                    ts_event=ts_event,
                )
            else:
                self.generate_order_rejected(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    reason=order_msg_data.rejection_reason,
                    ts_event=ts_event,
                )
            return


class ClientOrderIdGenerator:
    """
    Generate OKX client order IDs (alphanumeric only up to 32 characters).
    """

    def __init__(self, cache: Cache) -> None:
        self._cache = cache

    def generate_okx_client_order_id(self, client_order_id: ClientOrderId) -> str:
        okx_client_order_id = UUID(str(UUID4())).hex

        self._cache.add(
            client_order_id.value,
            okx_client_order_id.encode(),
        )
        self._cache.add(okx_client_order_id, client_order_id.value.encode())

        return okx_client_order_id

    def get_okx_client_order_id(self, client_order_id: ClientOrderId | None) -> str | None:
        if not client_order_id:
            return None

        value: bytes = self._cache.get(client_order_id.value)
        if value is not None:
            return value.decode()

    def get_client_order_id(self, okx_client_order_id: str | None) -> ClientOrderId | None:
        if not okx_client_order_id:
            return None

        value: bytes = self._cache.get(okx_client_order_id)
        if value is not None:
            return ClientOrderId(value.decode())
