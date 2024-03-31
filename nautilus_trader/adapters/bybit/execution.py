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
import json

import msgspec
import pandas as pd

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.credentials import get_api_key
from nautilus_trader.adapters.bybit.common.credentials import get_api_secret
from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.errors import BybitError
from nautilus_trader.adapters.bybit.provider import BybitInstrumentProvider
from nautilus_trader.adapters.bybit.schemas.common import BybitWsSubscriptionMsg
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
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
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.rust.common import LogColor
from nautilus_trader.core.rust.model import TimeInForce
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
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
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.position import Position


class BybitExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the `Bybit` centralized crypto exchange.

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
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.CASH,
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
        self._max_retries = config.max_retries
        self._retry_delay = config.retry_delay
        self._log.info(f"Account type: {account_type_to_str(self.account_type)}", LogColor.BLUE)
        self._log.info(f"Product types: {[p.value for p in product_types]}", LogColor.BLUE)
        self._log.info(f"{config.use_gtd=}", LogColor.BLUE)
        self._log.info(f"{config.use_reduce_only=}", LogColor.BLUE)
        self._log.info(f"{config.use_position_ids=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay=}", LogColor.BLUE)

        self._enum_parser = BybitEnumParser()

        account_id = AccountId(f"{BYBIT_VENUE.value}-UNIFIED")
        self._set_account_id(account_id)

        # Hot caches
        self._instrument_ids: dict[str, InstrumentId] = {}
        self._generate_order_status_retries: dict[ClientOrderId, int] = {}

        # WebSocket API
        self._ws_client = BybitWebsocketClient(
            clock=clock,
            handler=self._handle_ws_message,
            base_url=base_url_ws,
            is_private=True,
            api_key=config.api_key or get_api_key(config.testnet),
            api_secret=config.api_secret or get_api_secret(config.testnet),
        )

        # Http API
        self._http_account = BybitAccountHttpAPI(
            client=client,
            clock=clock,
        )

        # Order submission
        self._submit_order_methods = {
            OrderType.MARKET: self._submit_market_order,
            OrderType.LIMIT: self._submit_limit_order,
        }
        self._order_retries: dict[ClientOrderId, int] = {}

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

    async def _connect(self) -> None:
        # Update account state
        await self._update_account_state()

        # Connect to websocket
        await self._ws_client.connect()

        # Subscribe account updates
        await self._ws_client.subscribe_executions_update()
        await self._ws_client.subscribe_orders_update()

    async def _disconnect(self) -> None:
        await self._ws_client.disconnect()

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
            symbol = instrument_id.symbol.value if instrument_id is not None else None
            # active_symbols = self._get_cache_active_symbols()
            # active_symbols.update(await self._get_active_position_symbols(symbol))
            # open_orders: dict[BybitProductType, list[BybitOrder]] = dict()
            for instr in self._product_types:
                open_orders = await self._http_account.query_open_orders(instr, symbol)
                for order in open_orders:
                    # Uncomment for development
                    self._log.info(f"Generating report {order}", LogColor.MAGENTA)
                    bybit_symbol = BybitSymbol(order.symbol + f"-{instr.value.upper()}")
                    assert bybit_symbol is not None  # Type checking
                    report = order.parse_to_order_status_report(
                        account_id=self.account_id,
                        instrument_id=bybit_symbol.parse_as_nautilus(),
                        report_id=UUID4(),
                        enum_parser=self._enum_parser,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    reports.append(report)
                    self._log.debug(f"Received {report}")
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
        retries = self._generate_order_status_retries.get(client_order_id, 0)
        if retries > 3:
            self._log.error(
                f"Reached maximum retries 3/3 for generating OrderStatusReport for "
                f"{repr(client_order_id) if client_order_id else ''} "
                f"{repr(venue_order_id) if venue_order_id else ''}",
            )
            return None
        self._log.info(
            f"Generating OrderStatusReport for "
            f"{repr(client_order_id) if client_order_id else ''} "
            f"{repr(venue_order_id) if venue_order_id else ''}",
        )
        try:
            if venue_order_id:
                bybit_orders = await self._http_account.query_order(
                    product_type=BybitProductType.LINEAR,
                    symbol=instrument_id.symbol.value,
                    order_id=venue_order_id.value,
                )
                if len(bybit_orders) == 0:
                    self._log.error(f"Received no order for {venue_order_id}")
                    return None
                targetOrder = bybit_orders[0]
                if len(bybit_orders) > 1:
                    self._log.warning(f"Received more than one order for {venue_order_id}")
                    targetOrder = bybit_orders[0]

                order_report = targetOrder.parse_to_order_status_report(
                    account_id=self.account_id,
                    instrument_id=self._get_cached_instrument_id(targetOrder.symbol),
                    report_id=UUID4(),
                    enum_parser=self._enum_parser,
                    ts_init=self._clock.timestamp_ns(),
                )
                self._log.debug(f"Received {order_report}")
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
        return []

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[PositionStatusReport]:
        self._log.info("Requesting PositionStatusReports...")
        reports: list[PositionStatusReport] = []
        for product_type in self._product_types:
            if product_type == BybitProductType.SPOT:
                continue  # No positions on spot
            positions = await self._http_account.query_position_info(product_type)
            for position in positions:
                # Uncomment for development
                self._log.info(f"Generating report {position}", LogColor.MAGENTA)
                instr: InstrumentId = BybitSymbol(
                    position.symbol + "-" + product_type.value.upper(),
                ).parse_as_nautilus()
                position_report = position.parse_to_position_status_report(
                    account_id=self.account_id,
                    instrument_id=instr,
                    report_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                )
                self._log.debug(f"Received {position_report}")
                reports.append(position_report)
        return reports

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        # Parse instrument ID
        bybit_symbol = BybitSymbol(symbol + "-LINEAR")  # TODO: Determine how to handle products
        assert bybit_symbol  # type checking
        nautilus_instrument_id: InstrumentId = bybit_symbol.parse_as_nautilus()
        return nautilus_instrument_id

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
        return self._enum_parser.parse_bybit_time_in_force(time_in_force)

    async def _get_active_position_symbols(self, symbol: str | None) -> set[str]:
        active_symbols: set[str] = set()
        bybit_positions = await self._http_account.query_position_info(
            BybitProductType.LINEAR,
            symbol,
        )
        for position in bybit_positions:
            active_symbols.add(position.symbol)
        return active_symbols

    async def _update_account_state(self) -> None:
        # positions = await self._http_account.query_position_info()
        [product_type_balances, ts_event] = await self._http_account.query_wallet_balance()
        if product_type_balances:
            self._log.info("Bybit API key authenticated", LogColor.GREEN)
            self._log.info(f"API key {self._http_account.client.api_key} has trading permissions")
        for balance in product_type_balances:
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

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        bybit_symbol = BybitSymbol(command.instrument_id.symbol.value)
        assert bybit_symbol is not None  # Type checking
        await self._http_account.cancel_all_orders(
            BybitProductType.LINEAR,
            bybit_symbol.raw_symbol,
        )

    async def _submit_order(self, command: SubmitOrder) -> None:
        await self._submit_order_inner(command.order)

    async def _submit_order_inner(self, order: Order) -> None:
        if order.is_closed:
            self._log.warning(f"Order {order} is already closed")
            return

        if not self._check_order_validity(order):
            return

        self._log.debug(f"Submitting order {order}")

        # Generate order submitted event, to ensure correct ordering of event
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )
        while True:
            try:
                await self._submit_order_methods[order.order_type](order)
                self._order_retries.pop(order.client_order_id, None)
                break
            except KeyError:
                self._log.error(f"Unsupported order type, was {order.order_type}")
            except BybitError as e:
                self._log.error(repr(e))

    def _check_order_validity(self, order: Order) -> bool:
        # Check order type valid
        if order.order_type not in self._enum_parser.valid_order_types:
            self._log.error(
                f"Cannot submit {order} has invalid order type {order.order_type}, unsupported on Bybit",
            )
            return False
        # Check post only
        if order.is_post_only and order.order_type != OrderType.LIMIT:
            self._log.error(
                f"Cannot submit {order} has invalid post only {order.is_post_only}, unsupported on Bybit",
            )
            return False
        return True

    async def _submit_market_order(self, order: MarketOrder) -> None:
        pass

    async def _submit_limit_order(self, order: LimitOrder) -> None:
        bybit_symbol = BybitSymbol(order.instrument_id.symbol.value)
        assert bybit_symbol  # type checking
        time_in_force = self._determine_time_in_force(order)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        order_type = self._enum_parser.parse_nautilus_order_type(order.order_type)
        order = await self._http_account.place_order(
            product_type=bybit_symbol.product_type,
            symbol=bybit_symbol.raw_symbol,
            side=order_side,
            order_type=order_type,
            time_in_force=time_in_force,
            quantity=str(order.quantity),
            price=str(order.price),
            order_id=str(order.client_order_id),
        )

    def _handle_ws_message(self, raw: bytes) -> None:
        self._log.info(str(json.dumps(msgspec.json.decode(raw), indent=4)), color=LogColor.MAGENTA)
        try:
            ws_message = self._decoder_ws_msg_general.decode(raw)
            if ws_message.op == BYBIT_PONG:
                return
            if ws_message.topic:
                self._handle_ws_message_by_topic(ws_message.topic, raw)
        except Exception as e:
            self._log.error(f"Failed to parse websocket message: {raw.decode()} with error {e}")

    def _handle_ws_message_by_topic(self, topic: str, raw: bytes) -> None:
        if "order" in topic:
            self._handle_account_order_update(raw)
        elif "execution" in topic:
            self._handle_account_execution_update(raw)
        else:
            self._log.error(f"Unknown websocket message topic: {topic} in Bybit")

    # def _handle_account_position_update(self,raw: bytes):
    #     try:
    #         msg = self._decoder_ws_account_position_update.decode(raw)
    #         for position in msg.data:
    #             print(position)
    #     except Exception as e:
    #         print(e)

    def _handle_account_execution_update(self, raw: bytes) -> None:
        try:
            msg = self._decoder_ws_account_execution_update.decode(raw)
            for trade in msg.data:
                self._process_execution(trade)
        except Exception as e:
            self._log.exception(f"Failed to handle account execution update: {e}", e)

    def _process_execution(self, execution: BybitWsAccountExecution) -> None:
        client_order_id = (
            ClientOrderId(execution.orderLinkId) if execution.orderLinkId is not None else None
        )
        ts_event = millis_to_nanos(float(execution.execTime))
        venue_order_id = VenueOrderId(execution.orderId)
        instrument_id = self._get_cached_instrument_id(execution.symbol)
        strategy_id = self._cache.strategy_id_for_order(client_order_id)

        if instrument_id is None:
            raise ValueError(f"Cannot handle ws trade event: instrument {instrument_id} not found")
        if strategy_id is None:
            # this is a trade that was not placed by Nautilus
            print("NOT OUR TRADE")
            report = OrderStatusReport(
                account_id=self.account_id,
                instrument_id=instrument_id,
                client_order_id=execution.orderLinkId,
                venue_order_id=venue_order_id,
                order_side=self._enum_parser.parse_bybit_order_side(execution.side),
                order_type=self._enum_parser.parse_bybit_order_type(execution.orderType),
                order_status=OrderStatus.FILLED,
                time_in_force=TimeInForce.GTC,
                quantity=Quantity.from_str(execution.execQty),
                price=Price.from_str(execution.execPrice),
                filled_qty=Quantity.from_str(execution.execQty),
                ts_accepted=123,
                ts_init=123,
                ts_last=123,
                report_id=UUID4(),
            )
            self._send_order_status_report(report)
            return

        instrument = self._instrument_provider.find(instrument_id=instrument_id)
        if instrument is None:
            raise ValueError(f"Cannot handle ws trade event: instrument {instrument_id} not found")

        commission_asset: str | None = instrument.quote_currency
        commission_amount = Money(execution.execFee, commission_asset)
        self.generate_order_filled(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            trade_id=TradeId(execution.execId),
            order_side=self._enum_parser.parse_bybit_order_side(execution.side),
            order_type=self._enum_parser.parse_bybit_order_type(execution.orderType),
            last_qty=Quantity(float(execution.leavesQty), instrument.size_precision),
            last_px=Price(float(execution.execPrice), instrument.price_precision),
            quote_currency=instrument.quote_currency,
            commission=commission_amount,
            ts_event=ts_event,
        )

        if strategy_id is None:
            self._log.error(f"Cannot find strategy for order {execution.orderLinkId}")
            return

        # get order
        # get commission
        # commission_asset: str | None = instrument.quote_currency or Money(execution.execFee, commission_asset)

        self.generate_order_filled(
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=execution.orderLinkId,
            venue_order_id=execution.orderId,
        )

    def _handle_account_order_update(self, raw: bytes) -> None:
        try:
            msg = self._decoder_ws_account_order_update.decode(raw)
            for order in msg.data:
                report = order.parse_to_order_status_report(
                    account_id=self.account_id,
                    instrument_id=self._get_cached_instrument_id(order.symbol),
                    enum_parser=self._enum_parser,
                )
                self._send_order_status_report(report)
        except Exception as e:
            self._log.error(repr(e))
