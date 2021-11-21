# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from datetime import datetime
from decimal import Decimal
from typing import Any, Dict, List, Optional

import orjson

from nautilus_trader.adapters.binance.common import BINANCE_VENUE
from nautilus_trader.adapters.binance.http.api.spot_account import BinanceSpotAccountHttpAPI
from nautilus_trader.adapters.binance.http.api.spot_market import BinanceSpotMarketHttpAPI
from nautilus_trader.adapters.binance.http.api.user import BinanceUserDataHttpAPI
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceError
from nautilus_trader.adapters.binance.parsing import binance_order_type
from nautilus_trader.adapters.binance.parsing import parse_account_balances
from nautilus_trader.adapters.binance.parsing import parse_account_balances_ws
from nautilus_trader.adapters.binance.parsing import parse_order_type
from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
from nautilus_trader.adapters.binance.websocket.user import BinanceUserDataWebSocket
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.c_enums.account_type import AccountType
from nautilus_trader.model.c_enums.order_side import OrderSideParser
from nautilus_trader.model.c_enums.order_type import OrderType
from nautilus_trader.model.c_enums.time_in_force import TimeInForceParser
from nautilus_trader.model.c_enums.venue_type import VenueType
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import ModifyOrder
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.commands.trading import SubmitOrderList
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.market import MarketOrder
from nautilus_trader.model.orders.stop_limit import StopLimitOrder
from nautilus_trader.msgbus.bus import MessageBus


VALID_TIF = (TimeInForce.GTC, TimeInForce.FOK, TimeInForce.IOC)


class BinanceSpotExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Binance SPOT markets.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BinanceHttpClient,
        account_id: AccountId,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: BinanceInstrumentProvider,
    ):
        """
        Initialize a new instance of the ``BinanceSpotExecutionClient`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        client : BinanceHttpClient
            The binance HTTP client.
        account_id : AccountId
            The account ID for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.
        instrument_provider : BinanceInstrumentProvider
            The instrument provider.

        """
        super().__init__(
            loop=loop,
            client_id=ClientId(BINANCE_VENUE.value),
            instrument_provider=instrument_provider,
            venue_type=VenueType.EXCHANGE,
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config={"name": "BinanceExecClient"},
        )

        self._client = client

        # Hot caches
        self._instrument_ids: Dict[str, InstrumentId] = {}

        # HTTP API
        self._account_spot = BinanceSpotAccountHttpAPI(client=self._client)
        self._market_spot = BinanceSpotMarketHttpAPI(client=self._client)
        self._user = BinanceUserDataHttpAPI(client=self._client)

        # Listen keys
        self._ping_listen_keys_interval: int = 60 * 5  # Once every 5 mins (hardcode)
        self._ping_listen_keys_task: Optional[asyncio.Task] = None
        self._listen_key_spot: Optional[str] = None
        self._listen_key_margin: Optional[str] = None
        self._listen_key_isolated: Optional[str] = None

        # WebSocket API
        self._ws_user_spot = BinanceUserDataWebSocket(
            loop=loop,
            clock=clock,
            logger=logger,
            handler=self._handle_user_ws_message,
        )

    def connect(self) -> None:
        """
        Connect the client to Binance.
        """
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    def disconnect(self) -> None:
        """
        Disconnect the client from Binance.
        """
        self._log.info("Disconnecting...")
        self._loop.create_task(self._disconnect())

    async def _connect(self) -> None:
        # Connect HTTP client
        if not self._client.connected:
            await self._client.connect()
        try:
            await self._instrument_provider.load_all_or_wait_async()
        except BinanceError as ex:
            self._log.exception(ex)
            return

        # Authenticate API key and update account(s)
        raw: bytes = await self._account_spot.account(recv_window=5000)
        response: Dict[str, Any] = orjson.loads(raw)

        self._authenticate_api_key(response=response)
        self._update_account_state(response=response)

        # Get listen keys
        raw = await self._user.create_listen_key_spot()
        self._listen_key_spot = orjson.loads(raw)["listenKey"]
        self._ping_listen_keys_task = self._loop.create_task(self._ping_listen_keys())

        # Connect WebSocket clients
        self._ws_user_spot.subscribe(key=self._listen_key_spot)
        await self._ws_user_spot.connect()

        self._set_connected(True)
        self._log.info("Connected.")

    def _authenticate_api_key(self, response: Dict[str, Any]) -> None:
        if response["canTrade"]:
            self._log.info("Binance API key authenticated.", LogColor.GREEN)
            self._log.info(f"API key {self._client.api_key} has trading permissions.")
        else:
            self._log.error("Binance API key does not have trading permissions.")

    def _update_account_state(self, response: Dict[str, Any]) -> None:
        self.generate_account_state(
            balances=parse_account_balances(raw_balances=response["balances"]),
            reported=True,
            ts_event=response["updateTime"],
        )

    async def _ping_listen_keys(self):
        while True:
            self._log.debug(
                f"Scheduled ping listen keys to run in " f"{self._ping_listen_keys_interval}s."
            )
            await asyncio.sleep(self._ping_listen_keys_interval)
            if self._listen_key_spot:
                self._log.debug(f"Pinging WebSocket listen key {self._listen_key_spot}...")
                await self._user.ping_listen_key_spot(self._listen_key_spot)

    async def _disconnect(self) -> None:
        # Cancel tasks
        if self._ping_listen_keys_task:
            self._log.debug("Canceling ping listen keys task...")
            self._ping_listen_keys_task.cancel()

        # Disconnect WebSocket clients
        if self._ws_user_spot.is_connected:
            self._log.debug("Disconnecting websockets...")
            await self._ws_user_spot.disconnect()

        # Disconnect HTTP client
        if self._client.connected:
            await self._client.disconnect()

        self._set_connected(False)
        self._log.info("Disconnected.")

    # -- COMMAND HANDLERS --------------------------------------------------------------------------

    def submit_order(self, command: SubmitOrder) -> None:
        if command.order.type == OrderType.STOP_MARKET:
            self._log.error(
                "Cannot submit order: "
                "STOP_MARKET orders not supported by the exchange for SPOT markets. "
                "Use any of MARKET, LIMIT, STOP_LIMIT."
            )
            return
        elif command.order.type == OrderType.STOP_LIMIT:
            self._log.warning(
                "STOP_LIMIT `post_only` orders not supported by the exchange. "
                "This order may become a liquidity TAKER."
            )
        if command.order.time_in_force not in VALID_TIF:
            self._log.error(
                f"Cannot submit order: "
                f"{TimeInForceParser.to_str_py(command.order.time_in_force)} "
                f"not supported by the exchange. Use any of {VALID_TIF}.",
            )
            return
        self._loop.create_task(self._submit_order(command))

    def submit_order_list(self, command: SubmitOrderList) -> None:
        self._loop.create_task(self._submit_order_list(command))

    def modify_order(self, command: ModifyOrder) -> None:
        self._log.error(  # pragma: no cover
            "Cannot modify order: Not supported by the exchange.",
        )

    def cancel_order(self, command: CancelOrder) -> None:
        self._loop.create_task(self._cancel_order(command))

    async def _submit_order(self, command: SubmitOrder) -> None:
        self._log.debug(f"Submitting {command.order}.")

        # Generate event here to ensure correct ordering of events
        self._order_submitted(command)

        if command.order.type == OrderType.MARKET:
            await self._submit_market_order(command)
        elif command.order.type == OrderType.LIMIT:
            await self._submit_limit_order(command)
        elif command.order.type == OrderType.STOP_LIMIT:
            await self._submit_stop_limit_order(command)

    async def _submit_market_order(self, command: SubmitOrder):
        try:
            order: MarketOrder = command.order
            await self._account_spot.new_order(
                symbol=order.instrument_id.symbol.value,
                side=OrderSideParser.to_str_py(order.side),
                type="MARKET",
                quantity=str(order.quantity),
                new_client_order_id=order.client_order_id.value,
                recv_window=5000,
            )
        except BinanceError as ex:
            self._reject_order(command, ex)

    async def _submit_limit_order(self, command: SubmitOrder):
        try:
            order: LimitOrder = command.order
            if order.is_post_only:
                time_in_force = None
            else:
                time_in_force = TimeInForceParser.to_str_py(order.time_in_force)

            await self._account_spot.new_order(
                symbol=order.instrument_id.symbol.value,
                side=OrderSideParser.to_str_py(order.side),
                type=binance_order_type(order=order),
                time_in_force=time_in_force,
                quantity=str(order.quantity),
                price=str(order.price),
                iceberg_qty=str(order.display_qty) if order.display_qty is not None else None,
                new_client_order_id=order.client_order_id.value,
                recv_window=5000,
            )
        except BinanceError as ex:
            self._reject_order(command, ex)

    async def _submit_stop_limit_order(self, command: SubmitOrder):
        try:
            order: StopLimitOrder = command.order

            # Get current market price
            raw: bytes = await self._market_spot.ticker_price(order.instrument_id.symbol.value)
            response: Dict[str, Any] = orjson.loads(raw)
            market_price = Decimal(response["price"])

            await self._account_spot.new_order(
                symbol=order.instrument_id.symbol.value,
                side=OrderSideParser.to_str_py(order.side),
                type=binance_order_type(order=order, market_price=market_price),
                time_in_force=TimeInForceParser.to_str_py(order.time_in_force),
                quantity=str(order.quantity),
                price=str(order.price),
                stop_price=str(order.trigger),
                iceberg_qty=str(order.display_qty) if order.display_qty is not None else None,
                new_client_order_id=order.client_order_id.value,
                recv_window=5000,
            )
        except BinanceError as ex:
            self._reject_order(command, ex)

    def _reject_order(self, command: SubmitOrder, ex: BinanceError):
        self.generate_order_rejected(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.order.client_order_id,
            reason=ex.message,  # type: ignore  # TODO(cs): Improve errors
            ts_event=self._clock.timestamp_ns(),  # TODO(cs): Parse from response
        )

    def _order_submitted(self, command: SubmitOrder):
        # Generate event
        self.generate_order_submitted(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        self._log.error(  # pragma: no cover
            "Cannot submit order list: not yet implemented.",
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        self._log.debug(f"Canceling order {command.client_order_id.value}.")

        try:
            await self._account_spot.cancel_order(
                symbol=command.instrument_id.symbol.value,
                orig_client_order_id=command.client_order_id.value,
            )
        except BinanceError as ex:
            self._log.error(ex.message)  # type: ignore  # TODO(cs): Improve errors

    # -- RECONCILIATION ----------------------------------------------------------------------------

    async def generate_order_status_report(self, order: Order) -> OrderStatusReport:  # type: ignore
        """
        Generate an order status report for the given order.

        If an error occurs then logs and returns ``None``.

        Parameters
        ----------
        order : Order
            The order for the report.

        Returns
        -------
        OrderStatusReport or ``None``

        """
        self._log.error(  # pragma: no cover
            "Cannot generate order status report: not yet implemented.",
        )

    async def generate_exec_reports(
        self,
        venue_order_id: VenueOrderId,
        symbol: Symbol,
        since: datetime = None,
    ) -> List[ExecutionReport]:  # type: ignore
        """
        Generate a list of execution reports.

        The returned list may be empty if no trades match the given parameters.

        Parameters
        ----------
        venue_order_id : VenueOrderId
            The venue order ID for the trades.
        symbol : Symbol
            The symbol for the trades.
        since : datetime, optional
            The timestamp to filter trades on.

        Returns
        -------
        list[ExecutionReport]

        """
        self._log.error(  # pragma: no cover
            "Cannot generate execution report: not yet implemented.",
        )

        return []

    def _handle_user_ws_message(self, raw: bytes):
        msg: Dict[str, Any] = orjson.loads(raw)
        data: Dict[str, Any] = msg.get("data")

        # TODO(cs): Uncomment for development
        # self._log.info(str(json.dumps(msg, indent=4)), color=LogColor.GREEN)

        try:
            msg_type: str = data.get("e")
            if msg_type == "outboundAccountPosition":
                self._handle_account_position(data)
            elif msg_type == "executionReport":
                self._handle_execution_report(data)
        except Exception as ex:
            self._log.exception(ex)

    def _handle_account_position(self, data: Dict[str, Any]):
        self.generate_account_state(
            balances=parse_account_balances_ws(raw_balances=data["B"]),
            reported=True,
            ts_event=data["u"],
        )

    def _handle_execution_report(self, data: Dict[str, Any]):
        execution_type: str = data["x"]

        # Parse instrument ID
        symbol: str = data["s"]
        instrument_id: Optional[InstrumentId] = self._instrument_ids.get(symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(symbol), BINANCE_VENUE)
            self._instrument_ids[symbol] = instrument_id

        # Parse client order ID
        client_order_id_str: str = data["c"]
        if not client_order_id_str or not client_order_id_str.startswith("O"):
            client_order_id_str = data["C"]
        client_order_id = ClientOrderId(client_order_id_str)

        # Fetch strategy ID
        strategy_id = self._cache.strategy_id_for_order(client_order_id)
        if strategy_id is None:
            # TODO(cs): Implement external order handling
            self._log.error(
                f"Cannot handle execution report: " f"strategy ID for {client_order_id} not found.",
            )
            return

        venue_order_id = VenueOrderId(str(data["i"]))
        order_type_str: str = data["o"]
        ts_event: int = millis_to_nanos(data["E"])

        if execution_type == "NEW":
            self.generate_order_accepted(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
        elif execution_type == "TRADE":
            commission_asset: str = data["N"]
            commission_amount: str = data["n"]
            if commission_asset is not None:
                commission = Money.from_str(f"{commission_amount} {commission_asset}")
            else:
                instrument: Instrument = self._instrument_provider.find(instrument_id=instrument_id)
                # Binance typically charges commission as base asset or BNB
                commission = Money(0, instrument.base_currency)

            self.generate_order_filled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                venue_position_id=None,  # NETTING accounts
                execution_id=ExecutionId(str(data["t"])),  # Trade ID
                order_side=OrderSideParser.from_str_py(data["S"]),
                order_type=parse_order_type(order_type_str),
                last_qty=Quantity.from_str(data["l"]),
                last_px=Price.from_str(data["L"]),
                quote_currency=instrument.quote_currency,
                commission=commission,
                liquidity_side=LiquiditySide.MAKER if data["m"] else LiquiditySide.TAKER,
                ts_event=ts_event,
            )
        elif execution_type == "CANCELED" or execution_type == "EXPIRED":
            self.generate_order_canceled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
