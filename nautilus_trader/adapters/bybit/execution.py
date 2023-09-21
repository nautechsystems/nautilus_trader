import asyncio
from typing import Optional

import msgspec
import pandas as pd

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.data import BybitWsTopicCheck
from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.errors import BybitError
from nautilus_trader.adapters.bybit.schemas.common import BybitWsSubscriptionMsg
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.schemas.ws import BybitWsAccountOrderUpdateMsg, BybitWsAccountExecutionUpdateMsg
from nautilus_trader.adapters.bybit.websocket.client import BybitWebsocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.rust.common import LogColor
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import SubmitOrder, CancelOrder, CancelAllOrders
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType, OrderType
from nautilus_trader.model.identifiers import AccountId, ClientOrderId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.orders import Order, MarketOrder, LimitOrder
from nautilus_trader.model.position import Position
from nautilus_trader.msgbus.bus import MessageBus


class BybitExecutionClient(LiveExecutionClient):
    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BybitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: InstrumentProvider,
        instrument_type: BybitInstrumentType,
        base_url_ws: str,
        config: BybitExecClientConfig,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.CASH if instrument_type.is_spot else AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )
        self._log.info(f"Account type: ${self.account_type.value}", LogColor.BLUE)
        self._bybit_instrument_type = instrument_type
        self._enum_parser = BybitEnumParser()

        account_id = AccountId(f"{BYBIT_VENUE.value}-{self._bybit_instrument_type}")
        self._set_account_id(account_id)
        # Hot caches
        self._instrument_ids: dict[str, InstrumentId] = {}
        self._generate_order_status_retries: dict[ClientOrderId, int] = {}

        # Websocket API
        self._ws_client = BybitWebsocketClient(
            clock=clock,
            logger=logger,
            handler=self._handle_ws_message,
            base_url=base_url_ws,
            is_private=True,
            api_key=config.api_key,
            api_secret=config.api_secret,
        )


        # Http API
        self._http_account = BybitAccountHttpAPI(
            client=client,
            clock=clock,
            instrument_type=instrument_type,
        )

        # Order submission
        self._submit_order_methods = {
            OrderType.MARKET: self._submit_market_order,
            OrderType.LIMIT: self._submit_limit_order,
        }
        self._order_retries: dict[ClientOrderId, int] = {}

        # decoders
        self._decoder_ws_topic_check = msgspec.json.Decoder(BybitWsTopicCheck)
        self._decoder_ws_subscription = msgspec.json.Decoder(BybitWsSubscriptionMsg)
        self._decoder_ws_account_order_update = msgspec.json.Decoder(BybitWsAccountOrderUpdateMsg)
        self._decoder_ws_account_execution_update = msgspec.json.Decoder(BybitWsAccountExecutionUpdateMsg)



    async def _connect(self) -> None:
        # Initialize instrument provider
        await self._instrument_provider.initialize()
        # Update account state
        await self._update_account_state()
        # Connect to websocket
        await self._ws_client.connect()
        # subscribe account updates
        await self._ws_client.subscribe_executions_update()
        await self._ws_client.subscribe_orders_update()

    async def generate_order_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        self._log.info("Requesting OrderStatusReports...")
        reports: list[OrderStatusReport] = []
        try:
            symbol = instrument_id.symbol.value if instrument_id is not None else None
            # active_symbols = self._get_cache_active_symbols()
            # active_symbols.update(await self._get_active_position_symbols(symbol))
            open_orders = await self._http_account.query_open_orders(symbol)
            for order in open_orders:
                report = order.parse_to_order_status_report(
                    account_id=self.account_id,
                    instrument_id=self._get_cached_instrument_id(order.symbol),
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
        self._log.info(f"Received {len(reports)} OrderStatusReport{plural}.")
        return reports

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ) -> Optional[OrderStatusReport]:
        PyCondition.false(
            client_order_id is None and venue_order_id is None,
            "both `client_order_id` and `venue_order_id` were `None`",
        )
        retries = self._generate_order_status_retries.get(client_order_id, 0)
        if retries > 3:
            self._log.error(
                f"Reached maximum retries 3/3 for generating OrderStatusReport for "
                f"{repr(client_order_id) if client_order_id else ''} "
                f"{repr(venue_order_id) if venue_order_id else ''}...",
            )
            return None
        self._log.info(
            f"Generating OrderStatusReport for "
            f"{repr(client_order_id) if client_order_id else ''} "
            f"{repr(venue_order_id) if venue_order_id else ''}...",
        )
        try:
            if venue_order_id:
                bybit_order = await self._http_account.query_order(
                    symbol=instrument_id.symbol.value,
                    order_id=int(venue_order_id.value),
                )
        except BybitError as e:
            self._log.error(f"Failed to generate OrderStatusReport: {e}")
            return None



    async def generate_trade_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> list[TradeReport]:
        self._log.info("Requesting TradeReports...")
        return []

    async def generate_position_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> list[PositionStatusReport]:
        self._log.info("Requesting PositionStatusReports...")
        return []

    def _get_cache_active_symbols(self) -> set[str]:
        # check in cache for all active orders
        open_orders: list[Order] = self._cache.orders_open(venue=self.venue)
        open_positions: list[Position] = self._cache.positions_open(venue=self.venue)
        active_symbols: set[str] = set()
        for order in open_orders:
            active_symbols.add(BybitSymbol(order.instrument_id.symbol.value))
        for position in open_positions:
            active_symbols.add(BybitSymbol(position.instrument_id.symbol.value))
        return active_symbols


    def _get_cached_instrument_id(self, symbol: str) -> Optional[InstrumentId]:
        # parse instrument id
        nautilus_symbol: str = BybitSymbol(symbol).parse_as_nautilus(self._bybit_instrument_type)
        instrument_id: InstrumentId = self._instrument_ids.get(nautilus_symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(nautilus_symbol), BYBIT_VENUE)
            self._instrument_ids[nautilus_symbol] = instrument_id
        return instrument_id

    async def _get_active_position_symbols(self, symbol: Optional[str]) -> set[str]:
        active_symbols: set[str] = set()
        bybit_positions = await self._http_account.query_position_info(symbol)
        for position in bybit_positions:
            active_symbols.add(position.symbol)
        return active_symbols

    async def _update_account_state(self) -> None:
        # positions = await self._http_account.query_position_info()
        [instrument_type_balances, ts_event] = await self._http_account.query_wallet_balance()
        if instrument_type_balances:
            self._log.info("Binance API key authenticated.", LogColor.GREEN)
            self._log.info(f"API key {self._http_account.client.api_key} has trading permissions.")
        for balance in instrument_type_balances:
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
        await self._http_account.cancel_all_orders(command.instrument_id.symbol.value)

    async def _submit_order(self, command: SubmitOrder) -> None:
        await self._submit_order_inner(command.order)

    async def _submit_order_inner(self, order: Order)-> None:
        if order.is_closed:
            self._log.warning(f"Order {order} is already closed.")
            return
        # check validity
        self._check_order_validity(order)
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
                raise RuntimeError(f"unsupported order type, was {order.order_type}")
            except BybitError as e:
                print("BYBIT ERROR")

    def _check_order_validity(self, order: Order) -> None:
        # check order type valid
        if order.order_type not in self._enum_parser.valid_order_types:
            self._log.error(
                f"Cannot submit order.Order {order} has invalid order type {order.order_type}.Unsupported on bybit."
            )
            return
        # check time in force valid
        if order.time_in_force not in self._enum_parser.valid_time_in_force:
            self._log.error(
                f"Cannot submit order.Order {order} has invalid time in force {order.time_in_force}.Unsupported on bybit."
            )
            return
        # check post only
        if order.is_post_only and order.order_type != OrderType.LIMIT:
            self._log.error(
                f"Cannot submit order.Order {order} has invalid post only {order.is_post_only}.Unsupported on bybit."
            )
            return

    async def _submit_market_order(self, order: MarketOrder) -> None:
        pass

    async def _submit_limit_order(self, order: LimitOrder) -> None:
        time_in_force = self._enum_parser.parse_nautilus_time_in_force(order.time_in_force)
        order_side = self._enum_parser.parse_nautilus_order_side(order.side)
        order_type =self._enum_parser.parse_nautilus_order_type(order.order_type)
        await self._http_account.place_order(
            symbol=order.instrument_id.symbol.value,
            side=order_side,
            order_type=order_type,
            time_in_force=time_in_force,
            quantity=str(order.quantity),
            price=str(order.price),
            order_id=str(order.client_order_id)
        )

    ################################################################################
    #   WS user handlers
    ################################################################################
    def _handle_ws_message(self, raw: bytes) -> None:
        try:
            ws_message = self._decoder_ws_topic_check.decode(raw)
            self._topic_check(ws_message.topic, raw)
        except Exception as e:
            ws_message = self._decoder_ws_subscription.decode(raw)
            if ws_message.success:
                self._log.info("Success subscribing")
            else:
                self._log.error("Failed to subscribe.")
    def _topic_check(self,topic: str, raw: bytes) -> None:
        if "order" in topic:
            self._handle_account_order_update(raw)
        elif "execution" in topic:
            self._handle_account_execution_update(raw)
        elif "position" in topic:
            self._handle_account_position_update(raw)
        else:
            self._log.error(f"Unknown websocket message topic: {topic} in Bybit")

    def _handle_account_position_update(self,raw: bytes):
        pass

    def _handle_account_execution_update(self,raw: bytes):
        try:
            msg = self._decoder_ws_account_execution_update.decode(raw)
            print(msg)
        except Exception as e:
            print(e)
            print(raw)

    def _handle_account_order_update(self,raw: bytes):
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
            print(e)
            print(raw)

    async def _disconnect(self) -> None:
        await self._ws_client.disconnect()



