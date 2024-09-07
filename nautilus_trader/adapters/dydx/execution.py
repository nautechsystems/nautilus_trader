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
"""
Provide an execution client for the dYdX decentralized crypto exchange.
"""

import asyncio
import secrets
from collections import defaultdict
from decimal import Decimal
from typing import TYPE_CHECKING

import msgspec
import pandas as pd
from grpc.aio._call import AioRpcError
from v4_proto.dydxprotocol.clob.order_pb2 import Order as DYDXOrder
from v4_proto.dydxprotocol.clob.order_pb2 import OrderId as DYDXOrderId

from nautilus_trader.adapters.dydx.common.common import DYDXOrderTags
from nautilus_trader.adapters.dydx.common.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx.common.credentials import get_mnemonic
from nautilus_trader.adapters.dydx.common.credentials import get_wallet_address
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderStatus
from nautilus_trader.adapters.dydx.common.enums import DYDXPerpetualPositionStatus
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
from nautilus_trader.adapters.dydx.config import DYDXExecClientConfig
from nautilus_trader.adapters.dydx.grpc.account import DYDXAccountGRPCAPI
from nautilus_trader.adapters.dydx.grpc.account import Wallet
from nautilus_trader.adapters.dydx.grpc.errors import DYDXGRPCError
from nautilus_trader.adapters.dydx.grpc.order_builder import MAX_CLIENT_ID
from nautilus_trader.adapters.dydx.grpc.order_builder import DYDXGRPCOrderType
from nautilus_trader.adapters.dydx.grpc.order_builder import OrderBuilder
from nautilus_trader.adapters.dydx.grpc.order_builder import OrderFlags
from nautilus_trader.adapters.dydx.http.account import DYDXAccountHttpAPI
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.adapters.dydx.http.errors import DYDXError
from nautilus_trader.adapters.dydx.http.errors import should_retry
from nautilus_trader.adapters.dydx.providers import DYDXInstrumentProvider
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsFillSubaccountMessageContents
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsMessageGeneral
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsOrderSubaccountMessageContents
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsSubaccountsChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsSubaccountsSubscribed
from nautilus_trader.adapters.dydx.websocket.client import DYDXWebsocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import nanos_to_secs
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.live.retry import RetryManagerPool
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order


if TYPE_CHECKING:
    from nautilus_trader.model.objects import Currency


class ClientOrderIdHelper:
    """
    Generate integer client order IDs.
    """

    def __init__(self, cache: Cache) -> None:
        """
        Generate integer client order IDs.
        """
        self._cache = cache

    def generate_client_order_id_int(self, client_order_id: ClientOrderId) -> int:
        """
        Generate a unique client order ID integer and save it in the Cache.
        """
        try:
            client_order_id_int = int(client_order_id.value)
        except ValueError:
            client_order_id_int = secrets.randbelow(MAX_CLIENT_ID)

        # Store the generated client order ID integer in the cache for later lookup.
        # MAX_CLIENT_ID is 2**32 - 1 which can be represented by 32 bits, i.e. 4 bytes.
        self._cache.add(
            client_order_id.value,
            client_order_id_int.to_bytes(length=4, byteorder="big"),
        )
        self._cache.add(str(client_order_id_int), client_order_id.value.encode("utf-8"))

        return client_order_id_int

    def get_client_order_id_int(self, client_order_id: ClientOrderId) -> int | None:
        """
        Retrieve the ClientOrderId integer from the cache.
        """
        result = None

        try:
            result = int(client_order_id.value)
        except ValueError:
            value = self._cache.get(client_order_id.value)

            if value is not None:
                result = int.from_bytes(value, byteorder="big")

        return result

    def get_client_order_id(self, client_order_id_int: int) -> ClientOrderId:
        """
        Retrieve the ClientOrderId from the cache.
        """
        value = self._cache.get(str(client_order_id_int))

        if value is not None:
            return ClientOrderId(value.decode("utf-8"))

        return ClientOrderId(str(client_order_id_int))


class DYDXExecutionClient(LiveExecutionClient):
    """
    Provide an execution client for the dYdX decentralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : DYDXHttpClient
        The DYDX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : DYDXInstrumentProvider
        The instrument provider.
    base_url_ws : str
        The base URL for the WebSocket client.
    config : DYDXExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: DYDXHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: DYDXInstrumentProvider,
        grpc_account_client: DYDXAccountGRPCAPI,
        base_url_ws: str,
        config: DYDXExecClientConfig,
        name: str | None,
    ) -> None:
        """
        Provide an execution client for the dYdX decentralized crypto exchange.
        """
        account_type = AccountType.MARGIN

        super().__init__(
            loop=loop,
            client_id=ClientId(name or DYDX_VENUE.value),
            venue=DYDX_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=account_type,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Configuration
        self._wallet_address = config.wallet_address or get_wallet_address(
            is_testnet=config.is_testnet,
        )
        self._subaccount = config.subaccount

        self._enum_parser = DYDXEnumParser()
        self._client_order_id_generator = ClientOrderIdHelper(cache=cache)
        account_id = AccountId(
            f"{name or DYDX_VENUE.value}-{self._wallet_address}-{self._subaccount}",
        )
        self._set_account_id(account_id)

        # WebSocket API
        self._ws_client = DYDXWebsocketClient(
            clock=clock,
            handler=self._handle_ws_message,
            base_url=base_url_ws,
            loop=loop,
        )

        # GRPC API
        self._grpc_account = grpc_account_client
        self._mnemonic = config.mnemonic or get_mnemonic(is_testnet=config.is_testnet)

        # Initialize the wallet in the connect method
        self._wallet: Wallet | None = None

        # Http API
        self._http_account = DYDXAccountHttpAPI(
            client=client,
            clock=clock,
        )

        # Decoders
        self._decoder_ws_msg_general = msgspec.json.Decoder(DYDXWsMessageGeneral)
        self._decoder_ws_msg_subaccounts_subscribed = msgspec.json.Decoder(
            DYDXWsSubaccountsSubscribed,
        )
        self._decoder_ws_msg_subaccounts_channel = msgspec.json.Decoder(
            DYDXWsSubaccountsChannelData,
        )

        # Hot caches
        self._order_builders: dict[InstrumentId, OrderBuilder] = {}
        self._generate_order_status_retries: dict[ClientOrderId, int] = {}

        self._retry_manager_pool = RetryManagerPool(
            pool_size=100,
            max_retries=config.max_retries or 0,
            retry_delay_secs=config.retry_delay or 1.0,
            logger=self._log,
            exc_types=(DYDXError, DYDXGRPCError, AioRpcError),
            retry_check=should_retry,
        )

    async def _connect(self) -> None:
        # The instruments are used in the first account channel message.
        await self._instrument_provider.load_all_async()

        self._log.info("Initializing websocket connection")

        # Connect to websocket
        await self._ws_client.connect()

        # Subscribe account updates
        await self._ws_client.subscribe_account_update(
            wallet_address=self._wallet_address,
            subaccount_number=self._subaccount,
        )

        account = await self._grpc_account.get_account(address=self._wallet_address)
        self._wallet = Wallet(
            mnemonic=self._mnemonic,
            account_number=account.account_number,
            sequence=account.sequence,
        )

        self._log.info("Execution client connected")

    async def _disconnect(self) -> None:
        await self._ws_client.unsubscribe_account_update(
            wallet_address=self._wallet_address,
            subaccount_number=self._subaccount,
        )
        await self._ws_client.disconnect()
        await self._grpc_account.disconnect()

    def _stop(self) -> None:
        self._retry_manager_pool.shutdown()

    async def _get_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
        order_side: OrderSide | None = None,
        order_type: OrderType | None = None,
    ) -> OrderStatusReport | None:
        PyCondition.false(
            client_order_id is None and venue_order_id is None,
            "both `client_order_id` and `venue_order_id` were `None`",
        )
        result = None

        instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            self._log.error(
                f"Cannot create order status report: instrument {instrument_id} not found",
            )
            return None

        if venue_order_id is None:
            dydx_orders = await self._http_account.get_orders(
                address=self._wallet_address,
                subaccount_number=self._subaccount,
                symbol=instrument_id.symbol.value.removesuffix("-PERP"),
                order_side=self._enum_parser.parse_nautilus_order_side(order_side),
                order_type=self._enum_parser.parse_nautilus_order_type(order_type),
                return_latest_orders=True,
            )

            for dydx_order in dydx_orders:
                current_client_order_id = self._client_order_id_generator.get_client_order_id(
                    int(dydx_order.clientId),
                )

                if current_client_order_id == client_order_id:
                    result = dydx_order.parse_to_order_status_report(
                        account_id=self.account_id,
                        client_order_id=current_client_order_id,
                        price_precision=instrument.price_precision,
                        size_precision=instrument.size_precision,
                        report_id=UUID4(),
                        enum_parser=self._enum_parser,
                        ts_init=self._clock.timestamp_ns(),
                    )
        else:
            dydx_order = await self._http_account.get_order(
                address=self._wallet_address,
                subaccount_number=self._subaccount,
                order_id=venue_order_id.value,
            )

            result = dydx_order.parse_to_order_status_report(
                account_id=self.account_id,
                client_order_id=client_order_id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
                report_id=UUID4(),
                enum_parser=self._enum_parser,
                ts_init=self._clock.timestamp_ns(),
            )

        return result

    async def generate_order_status_report(  # noqa: C901
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
    ) -> OrderStatusReport | None:
        """
        Create an order status report for a specific order.
        """
        self._log.info("Requesting OrderStatusReport...")
        PyCondition.false(
            client_order_id is None and venue_order_id is None,
            "both `client_order_id` and `venue_order_id` were `None`",
        )

        max_retries = 3
        retries = self._generate_order_status_retries.get(client_order_id, 0)

        if retries > max_retries:
            self._log.error(
                f"Reached maximum retries {retries}/{max_retries} for generating OrderStatusReport for "
                f"{repr(client_order_id) if client_order_id else ''} "
                f"{repr(venue_order_id) if venue_order_id else ''}",
            )
            return None

        self._log.info(
            f"Generating OrderStatusReport for {repr(client_order_id) if client_order_id else ''} {repr(venue_order_id) if venue_order_id else ''}",
        )

        report = None
        order = None

        if client_order_id is None:
            client_order_id = self._cache.client_order_id(venue_order_id)

        if client_order_id:
            order = self._cache.order(client_order_id)

        if order is None:
            message = f"Cannot find order {client_order_id!r}"
            self._log.error(message)
            return None

        if order.is_closed:
            return None  # Nothing else to do

        if venue_order_id is None:
            venue_order_id = order.venue_order_id

        try:
            report = await self._get_order_status_report(
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                order_side=order.side,
                order_type=order.order_type,
            )

        except DYDXError as e:
            retries += 1
            self._log.error(
                f"Cannot generate order status report for {client_order_id!r}: {e.message}. Retry {retries}/{max_retries}",
            )
            self._generate_order_status_retries[client_order_id] = retries

            if not client_order_id:
                self._log.warning("Cannot retry without a client order ID")
            elif retries >= max_retries:
                # Order will no longer be considered in-flight once this event is applied.
                # We could pop the value out of the hashmap here, but better to leave it in
                # so that there are no longer subsequent retries (we don't expect many of these).
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    reason=str(e.message),
                    ts_event=self._clock.timestamp_ns(),
                )

        if not report:
            # Cannot proceed to generating report
            self._log.warning(
                f"Cannot generate `OrderStatusReport` for {client_order_id=!r}, {venue_order_id=!r}: order not found",
            )

        return report

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        """
        Create an order status report.
        """
        self._log.info("Requesting OrderStatusReports...")
        reports: list[OrderStatusReport] = []

        symbol = None
        start_dt = start.to_pydatetime() if start is not None else None
        end_dt = end.to_pydatetime() if end is not None else None

        if instrument_id is not None:
            symbol = instrument_id.symbol.value.removesuffix("-PERP")

        try:
            dydx_orders = await self._http_account.get_orders(
                address=self._wallet_address,
                subaccount_number=self._subaccount,
                symbol=symbol,
            )

            for dydx_order in dydx_orders:
                current_instrument_id = DYDXSymbol(dydx_order.ticker).to_instrument_id()
                instrument = self._cache.instrument(current_instrument_id)

                if instrument is None:
                    self._log.error(
                        f"Cannot handle fill event: instrument {current_instrument_id} not found",
                    )
                    return []

                # We use the updatedAt property to filter the orders since the
                # createdAt property does not exist. createdAtBlockHeight is
                # available, but a mapping between block height and datetime is missing.
                if (
                    start_dt is not None
                    and dydx_order.updatedAt is not None
                    and dydx_order.updatedAt < start_dt
                ):
                    continue  # Filter start on the Nautilus side

                if (
                    end_dt is not None
                    and dydx_order.updatedAt is not None
                    and dydx_order.updatedAt > end_dt
                ):
                    continue  # Filter end on the Nautilus side

                report = dydx_order.parse_to_order_status_report(
                    account_id=self.account_id,
                    client_order_id=self._client_order_id_generator.get_client_order_id(
                        int(dydx_order.clientId),
                    ),
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                    report_id=UUID4(),
                    enum_parser=self._enum_parser,
                    ts_init=self._clock.timestamp_ns(),
                )
                reports.append(report)
        except DYDXError as e:
            self._log.error(f"Failed to generate OrderStatusReports: {e}")

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} OrderStatusReport{plural}")

        return reports

    async def generate_fill_reports(
        self,
        instrument_id: InstrumentId | None = None,
        venue_order_id: VenueOrderId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[FillReport]:
        """
        Create an order fill report.
        """
        self._log.info("Requesting FillReports...")
        reports: list[FillReport] = []

        symbol = None
        start_dt = start.to_pydatetime() if start is not None else None
        end_dt = end.to_pydatetime() if end is not None else None

        if instrument_id is not None:
            symbol = instrument_id.symbol.value.removesuffix("-PERP")

        try:
            dydx_fills = await self._http_account.get_fills(
                address=self._wallet_address,
                subaccount_number=self._subaccount,
                symbol=symbol,
                created_before_or_at=end_dt,
            )

            for dydx_fill in dydx_fills.fills:
                client_order_id = None

                if dydx_fill.orderId is not None:
                    client_order_id = self._cache.client_order_id(VenueOrderId(dydx_fill.orderId))
                else:
                    self._log.warning(
                        "Venue order ID not set by venue. Unable to retrieve ClientOrderId",
                    )

                current_instrument_id = DYDXSymbol(dydx_fill.market).to_instrument_id()
                instrument = self._cache.instrument(current_instrument_id)

                if instrument is None:
                    self._log.error(
                        f"Cannot handle fill event: instrument {current_instrument_id} not found",
                    )
                    return []

                if (
                    start_dt is not None
                    and dydx_fill.createdAt is not None
                    and dydx_fill.createdAt < start_dt
                ):
                    continue  # Filter start on the Nautilus side

                report = dydx_fill.parse_to_fill_report(
                    account_id=self.account_id,
                    client_order_id=client_order_id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                    report_id=UUID4(),
                    enum_parser=self._enum_parser,
                    ts_init=self._clock.timestamp_ns(),
                )
                reports.append(report)

        except DYDXError as e:
            self._log.error(f"Failed to generate FillReports: {e}")

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} FillReport{plural}")
        return reports

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[PositionStatusReport]:
        """
        Generate position status reports.
        """
        self._log.info("Requesting PositionStatusReports...")
        reports: list[PositionStatusReport] = []

        try:
            dydx_positions = await self._http_account.get_perpetual_positions(
                address=self._wallet_address,
                subaccount_number=self._subaccount,
                status=DYDXPerpetualPositionStatus.OPEN,
            )

            if instrument_id:
                for dydx_position in dydx_positions.positions:
                    current_instrument_id = DYDXSymbol(dydx_position.market).to_instrument_id()

                    if current_instrument_id == instrument_id:
                        instrument = self._cache.instrument(current_instrument_id)

                        if instrument is None:
                            self._log.error(
                                f"Cannot generate position status reports: no instrument for {current_instrument_id}",
                            )
                            return reports

                        report = dydx_position.parse_to_position_status_report(
                            account_id=self.account_id,
                            size_precision=instrument.size_precision,
                            report_id=UUID4(),
                            enum_parser=self._enum_parser,
                            ts_init=self._clock.timestamp_ns(),
                        )
                        reports.append(report)

                if not reports:
                    now = self._clock.timestamp_ns()
                    report = PositionStatusReport(
                        account_id=self.account_id,
                        instrument_id=instrument_id,
                        position_side=PositionSide.FLAT,
                        quantity=Quantity.zero(),
                        report_id=UUID4(),
                        ts_last=now,
                        ts_init=now,
                    )
                    reports = [report]
            else:
                for dydx_position in dydx_positions.positions:
                    current_instrument_id = DYDXSymbol(dydx_position.market).to_instrument_id()

                    instrument = self._cache.instrument(current_instrument_id)

                    if instrument is None:
                        self._log.error(
                            f"Cannot generate position status reports: no instrument for {current_instrument_id}",
                        )
                        return reports

                    report = dydx_position.parse_to_position_status_report(
                        account_id=self.account_id,
                        size_precision=instrument.size_precision,
                        report_id=UUID4(),
                        enum_parser=self._enum_parser,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    reports.append(report)

        except DYDXError as e:
            self._log.error(f"Failed to generate PositionStatusReports: {e}")

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} PositionStatusReport{plural}")
        return reports

    def _handle_ws_message(self, raw: bytes) -> None:
        try:
            ws_message = self._decoder_ws_msg_general.decode(raw)

            if ws_message.channel == "v4_subaccounts" and ws_message.type == "channel_data":
                self._handle_subaccounts_channel_data(raw)
            elif ws_message.channel == "v4_subaccounts" and ws_message.type == "subscribed":
                self._handle_subaccounts_subscribed(raw)
            elif ws_message.type == "unsubscribed":
                self._log.info(
                    f"Unsubscribed from channel {ws_message.channel} for {ws_message.id}",
                )
            elif ws_message.type == "connected":
                self._log.info("Websocket connected")
            else:
                self._log.error(f"Unknown message `{ws_message.type}`: {raw.decode()}")
        except Exception as e:
            self._log.error(f"Failed to parse websocket message: {raw.decode()} with error {e}")

    def _handle_subaccounts_subscribed(self, raw: bytes) -> None:
        try:
            msg: DYDXWsSubaccountsSubscribed = self._decoder_ws_msg_subaccounts_subscribed.decode(
                raw,
            )

            if msg.contents.subaccount is None:
                self._log.error(f"Subaccount {self._wallet_address}/{self._subaccount} not found")
                return

            account_balances = msg.contents.parse_to_account_balances()
            initial_margins: defaultdict[Currency, Decimal] = defaultdict(Decimal)
            maintenance_margins: defaultdict[Currency, Decimal] = defaultdict(Decimal)
            instruments = self._instrument_provider.get_all()

            for perpetual_position in msg.contents.subaccount.openPerpetualPositions.values():
                instrument_id = DYDXSymbol(perpetual_position.market).to_instrument_id()
                instrument = self._cache.instrument(instrument_id)

                if instrument is None:
                    instrument = instruments.get(instrument_id)

                if instrument is None:
                    self._log.error(
                        f"Cannot parse margin balance: no instrument for {instrument_id}",
                    )
                    return

                margin_balance = perpetual_position.parse_margin_balance(
                    margin_init=instrument.margin_init,
                    margin_maint=instrument.margin_maint,
                )

                initial_margins[
                    margin_balance.initial.currency
                ] += margin_balance.initial.as_decimal()
                maintenance_margins[
                    margin_balance.maintenance.currency
                ] += margin_balance.maintenance.as_decimal()

            margins = []

            for currency, initial_margin in initial_margins.items():
                margins.append(
                    MarginBalance(
                        initial=Money(initial_margin, currency),
                        maintenance=Money(maintenance_margins[currency], currency),
                    ),
                )

            self.generate_account_state(
                balances=account_balances,
                margins=margins,
                reported=False,
                ts_event=self._clock.timestamp_ns(),
            )

        except Exception as e:
            self._log.error(
                f"Failed to parse subaccounts subscribed message: {raw.decode()} with error {e}",
            )

    def _handle_subaccounts_channel_data(self, raw: bytes) -> None:
        try:
            msg: DYDXWsSubaccountsChannelData = self._decoder_ws_msg_subaccounts_channel.decode(raw)

            if msg.contents.fills is not None:
                for fill_msg in msg.contents.fills:
                    self._handle_fill_message(fill_msg=fill_msg)

            if msg.contents.orders is not None:
                for order_msg in msg.contents.orders:
                    self._handle_order_message(order_msg=order_msg)

        except Exception as e:
            self._log.error(
                f"Failed to parse subaccounts channel data: {raw.decode()} with error {e}",
            )

    def _handle_order_message(  # noqa: C901
        self,
        order_msg: DYDXWsOrderSubaccountMessageContents,
    ) -> None:
        client_order_id = None

        if order_msg.clientId is not None:
            client_order_id = self._client_order_id_generator.get_client_order_id(
                client_order_id_int=int(order_msg.clientId),
            )

        instrument_id = DYDXSymbol(order_msg.ticker).to_instrument_id()
        instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            self._log.error(f"Cannot handle order event: instrument {instrument_id} not found")
            return

        report = order_msg.parse_to_order_status_report(
            account_id=self.account_id,
            client_order_id=client_order_id,
            price_precision=instrument.price_precision,
            size_precision=instrument.size_precision,
            report_id=UUID4(),
            enum_parser=self._enum_parser,
            ts_init=self._clock.timestamp_ns(),
        )

        strategy_id = None

        if report.client_order_id:
            strategy_id = self._cache.strategy_id_for_order(report.client_order_id)

        if strategy_id is None:
            # External order
            self._send_order_status_report(report)
            return

        order = self._cache.order(report.client_order_id)

        if order is None:
            self._log.error(f"Cannot handle order event: order {report.client_order_id} not found")
            return

        if order_msg.status in (DYDXOrderStatus.BEST_EFFORT_OPENED, DYDXOrderStatus.OPEN):
            self.generate_order_accepted(
                strategy_id=strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
            )
        elif order_msg.status == DYDXOrderStatus.CANCELED:
            if order.status != OrderStatus.CANCELED:
                self.generate_order_canceled(
                    strategy_id=strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    ts_event=report.ts_last,
                )
        elif order_msg.status in (
            DYDXOrderStatus.UNTRIGGERED,
            DYDXOrderStatus.BEST_EFFORT_CANCELED,
        ):
            self.generate_order_updated(
                strategy_id=strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                quantity=report.quantity,
                price=report.price,
                trigger_price=report.trigger_price,
                ts_event=report.ts_last,
            )
        elif order_msg.status == DYDXOrderStatus.FILLED:
            # Skip order filled message. The _handle_fill_message generates
            # a fill report.
            self._log.debug(f"Skip order fill message: {order_msg}")
        else:
            message = f"Unknown order status `{order_msg.status}`"
            self._log.error(message)

    def _handle_fill_message(self, fill_msg: DYDXWsFillSubaccountMessageContents) -> None:
        instrument_id = DYDXSymbol(fill_msg.ticker).to_instrument_id()
        instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            message = f"Cannot handle fill event: instrument {instrument_id} not found"
            self._log.error(message)
            return

        venue_order_id = VenueOrderId(fill_msg.orderId)
        client_order_id = self._cache.client_order_id(venue_order_id)

        if client_order_id is None:
            self._log.error(
                f"Cannot process order execution for {venue_order_id!r}: no `ClientOrderId` found (most likely due to being an external order)",
            )
            return

        order = self._cache.order(client_order_id)

        if order is None:
            message = f"Cannot handle fill event: instrument order `{client_order_id}` not found"
            self._log.error(message)
            return

        commission = (
            Money(Decimal(fill_msg.fee), instrument.quote_currency)
            if fill_msg.fee is not None
            else Money(Decimal(0), instrument.quote_currency)
        )

        self.generate_order_filled(
            strategy_id=order.strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            venue_position_id=None,
            trade_id=TradeId(fill_msg.id),
            order_side=self._enum_parser.parse_dydx_order_side(fill_msg.side),
            order_type=order.order_type,
            last_qty=Quantity(Decimal(fill_msg.size), instrument.size_precision),
            last_px=Price(Decimal(fill_msg.price), instrument.price_precision),
            quote_currency=instrument.quote_currency,
            commission=commission,
            liquidity_side=self._enum_parser.parse_dydx_liquidity_side(fill_msg.liquidity),
            ts_event=dt_to_unix_nanos(fill_msg.createdAt),
        )

    def _get_order_builder(self, instrument: Instrument) -> OrderBuilder:
        """
        Construct an OrderBuilder for a specific instrument.
        """
        order_builder = self._order_builders.get(instrument.id)

        if order_builder is None:
            order_builder = OrderBuilder(
                atomic_resolution=instrument.info["atomicResolution"],
                step_base_quantums=instrument.info["stepBaseQuantums"],
                subticks_per_tick=instrument.info["subticksPerTick"],
                quantum_conversion_exponent=instrument.info["quantumConversionExponent"],
                clob_pair_id=int(instrument.info["clobPairId"]),
            )
            self._order_builders[instrument.id] = order_builder

        return order_builder

    def _parse_order_tags(self, order: Order) -> DYDXOrderTags:
        """
        Parse the order tags to submit short term and long term orders.
        """
        result = DYDXOrderTags()

        if order.tags is not None:
            for order_tag in order.tags:
                if order_tag.startswith("DYDXOrderTags:"):
                    result = DYDXOrderTags.parse(order_tag.replace("DYDXOrderTags:", ""))

        return result

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        """
        Submit a batch of orders at once.

        dYdX does not support sending a batch of orders at once, but this method ensures
        that the wallet sequence number is correctly incremented when sending multiple
        orders at once.

        In case orders are canceled and submitted in parallel, the wallet sequence
        number is sometimes incorrect resulting in rejected orders or rejected cancels.

        """
        self._log.debug(f"Submit {len(command.order_list.orders)} orders", LogColor.CYAN)

        for order in command.order_list.orders:
            await self._submit_order_single(order=order)

    async def _submit_order_single(self, order) -> None:
        """
        Submit a single order.
        """
        if order.is_closed:
            self._log.warning(f"Order {order} is already closed")
            return

        instrument = self._cache.instrument(order.instrument_id)

        if instrument is None:
            rejection_reason = f"Cannot submit order: no instrument for {order.instrument_id}"
            self._log.error(rejection_reason)

            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=rejection_reason,
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

        order_builder = self._get_order_builder(instrument)

        client_order_id_int = self._client_order_id_generator.generate_client_order_id_int(
            client_order_id=order.client_order_id,
        )

        dydx_order_tags = self._parse_order_tags(order=order)
        order_flags = OrderFlags.SHORT_TERM
        good_til_date_secs: int | None = None
        good_til_block: int | None = None

        if dydx_order_tags.is_short_term_order:
            try:
                latest_block = await self._grpc_account.latest_block_height()
            except AioRpcError as e:
                rejection_reason = f"Failed to submit the order while retrieve the latest block height: code {e.code} {e.details}"
                self._log.error(rejection_reason)

                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=rejection_reason,
                    ts_event=self._clock.timestamp_ns(),
                )
                return

            good_til_block = latest_block + dydx_order_tags.num_blocks_open
        else:
            order_flags = OrderFlags.LONG_TERM
            good_til_date_secs = (
                int(nanos_to_secs(order.expire_time_ns)) if order.expire_time_ns else None
            )

        order_id = order_builder.create_order_id(
            address=self._wallet_address,
            subaccount_number=self._subaccount,
            client_id=client_order_id_int,
            order_flags=order_flags,
        )
        order_type_map = {
            OrderType.LIMIT: DYDXGRPCOrderType.LIMIT,
            OrderType.MARKET: DYDXGRPCOrderType.MARKET,
        }
        order_side_map = {
            OrderSide.NO_ORDER_SIDE: DYDXOrder.Side.SIDE_UNSPECIFIED,
            OrderSide.BUY: DYDXOrder.Side.SIDE_BUY,
            OrderSide.SELL: DYDXOrder.Side.SIDE_SELL,
        }
        time_in_force_map = {
            TimeInForce.GTC: DYDXOrder.TimeInForce.TIME_IN_FORCE_UNSPECIFIED,
            TimeInForce.GTD: DYDXOrder.TimeInForce.TIME_IN_FORCE_UNSPECIFIED,
            TimeInForce.IOC: DYDXOrder.TimeInForce.TIME_IN_FORCE_IOC,
            TimeInForce.FOK: DYDXOrder.TimeInForce.TIME_IN_FORCE_FILL_OR_KILL,
        }

        price = 0

        if order.order_type == OrderType.LIMIT:
            price = order.price.as_double()
        elif order.order_type == OrderType.MARKET:
            price = 0
        else:
            rejection_reason = (
                f"Cannot submit order: order type `{order.order_type}` not (yet) supported."
            )
            self._log.error(rejection_reason)

            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=rejection_reason,
                ts_event=self._clock.timestamp_ns(),
            )
            return

        order_msg = order_builder.create_order(
            order_id=order_id,
            order_type=order_type_map[order.order_type],
            side=order_side_map[order.side],
            size=order.quantity.as_double(),
            price=price,
            time_in_force=time_in_force_map[order.time_in_force],
            reduce_only=order.is_reduce_only,
            post_only=order.is_post_only,
            good_til_block=good_til_block,
            good_til_block_time=good_til_date_secs,
        )

        await self._place_order(order_msg=order_msg, order=order)

    async def _place_order(self, order_msg: DYDXOrder, order: Order):
        if self._wallet is None:
            rejection_reason = "Cannot submit order: no wallet available."
            self._log.error(rejection_reason)

            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=rejection_reason,
                ts_event=self._clock.timestamp_ns(),
            )
            return

        async with self._retry_manager_pool as retry_manager:
            await retry_manager.run(
                name="place_order",
                details=[order.client_order_id],
                func=self._grpc_account.place_order,
                wallet=self._wallet,
                order=order_msg,
            )
            if not retry_manager.result:
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=retry_manager.message,
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _submit_order(self, command: SubmitOrder) -> None:
        await self._submit_order_single(order=command.order)

    async def _cancel_order(self, command: CancelOrder) -> None:
        await self._cancel_order_single(
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
        )

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        # Check open orders for the strategy
        open_orders_strategy: list[Order] = self._cache.orders_open(strategy_id=command.strategy_id)
        open_order_ids = {order.client_order_id for order in open_orders_strategy}

        # Filter orders that are actually open
        valid_cancels: list[CancelOrder] = []

        for cancel in command.cancels:
            if cancel.client_order_id in open_order_ids:
                valid_cancels.append(cancel)
            else:
                self._log.warning(f"{cancel.client_order_id!r} not open for cancel")

        if not valid_cancels:
            self._log.warning("No orders open for batch cancel")
            return

        for order in valid_cancels:
            await self._cancel_order_single(
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        open_orders_strategy: list[Order] = self._cache.orders_open(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )

        for order in open_orders_strategy:
            await self._cancel_order_single(
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
            )

    async def _cancel_order_single(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
    ) -> None:
        order: Order | None = self._cache.order(client_order_id)

        if order is None:
            self._log.error(f"{client_order_id!r} not found to cancel")
            return

        if order.is_closed:
            self._log.warning(
                f"CancelOrder command for {client_order_id!r} when order already {order.status_string()} (will not send to exchange)",
            )
            return

        instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            self._log.error(
                f"Cannot cancel order {client_order_id!r}: no instrument for {instrument_id}",
            )
            return

        order_builder = self._get_order_builder(instrument)

        client_order_id_int = self._client_order_id_generator.get_client_order_id_int(
            client_order_id=client_order_id,
        )

        if client_order_id_int is None:
            self._log.error(
                f"Cannot cancel order: ClientOrderId integer not found for {client_order_id!r}",
            )
            return

        dydx_order_tags = self._parse_order_tags(order=order)
        order_flags = OrderFlags.SHORT_TERM
        good_til_date_secs: int | None = None

        if dydx_order_tags.is_short_term_order is False:
            order_flags = OrderFlags.LONG_TERM
            good_til_date_secs = (
                int(nanos_to_secs(order.expire_time_ns)) if order.expire_time_ns else None
            )

        order_id = order_builder.create_order_id(
            address=self._wallet_address,
            subaccount_number=self._subaccount,
            client_id=client_order_id_int,
            order_flags=order_flags,
        )

        await self._cancel_order_single_and_retry(
            order=order,
            order_id=order_id,
            good_til_date_secs=good_til_date_secs,
        )

    async def _cancel_order_single_and_retry(
        self,
        order: Order,
        order_id: DYDXOrderId,
        good_til_date_secs: int | None,
    ) -> None:
        if self._wallet is None:
            reason = f"Cannot cancel order {order.client_order_id!r}: no wallet available."
            self._log.error(reason)
            self.generate_order_cancel_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=reason,
                ts_event=self._clock.timestamp_ns(),
            )
            return

        is_expired = (
            nanos_to_secs(self._clock.timestamp_ns()) > good_til_date_secs
            if good_til_date_secs
            else False
        )

        if is_expired:
            reason = f"Cannot cancel order: order {order.client_order_id!r} is expired"
            self._log.warning(reason)
            self.generate_order_cancel_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=reason,
                ts_event=self._clock.timestamp_ns(),
            )
            return

        current_block = await self._grpc_account.latest_block_height()

        async with self._retry_manager_pool as retry_manager:
            await retry_manager.run(
                name="cancel_order",
                details=[order.client_order_id, order.venue_order_id],
                func=self._grpc_account.cancel_order,
                wallet=self._wallet,
                order_id=order_id,
                good_til_block=current_block + 10,
                good_til_block_time=good_til_date_secs,
            )
            if not retry_manager.result:
                self._log.error(f"Failed to cancel order: {retry_manager.message}")
                self.generate_order_cancel_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=retry_manager.message,
                    ts_event=self._clock.timestamp_ns(),
                )
