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

"""
Provides a data client for Hyperliquid.

This module provides the `HyperliquidDataClient` class which connects to the
Hyperliquid WebSocket API and HTTP API to provide real-time market data.
"""

import asyncio
from typing import Any

from nautilus_trader.adapters.hyperliquid2.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid2.providers import HyperliquidInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


class HyperliquidDataClient(LiveMarketDataClient):
    """
    Provides a data client for the `Hyperliquid` DEX.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.HyperliquidHttpClient
        The Hyperliquid HTTP client.
    ws_client : nautilus_pyo3.HyperliquidWebSocketClient
        The Hyperliquid WebSocket client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : HyperliquidInstrumentProvider
        The instrument provider.
    base_url_http : str, optional
        The base HTTP URL.
    base_url_ws : str, optional
        The base WebSocket URL.
    update_instruments_interval_mins : int, optional
        The interval for updating instruments.
    name : str, optional
        The custom client name.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.HyperliquidHttpClient,
        ws_client: nautilus_pyo3.HyperliquidWebSocketClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: HyperliquidInstrumentProvider,
        base_url_http: str | None = None,
        base_url_ws: str | None = None,
        update_instruments_interval_mins: int | None = None,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or "HYPERLIQUID"),
            venue=HYPERLIQUID_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Clients
        self._http_client = client
        self._ws_client = ws_client

        # Configuration
        self._base_url_http = base_url_http
        self._base_url_ws = base_url_ws
        self._update_instruments_interval_mins = update_instruments_interval_mins

        # Subscription management
        self._subscribed_instruments: set[InstrumentId] = set()

    @property
    def hyperliquid_instrument_provider(self) -> HyperliquidInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        # Load instruments
        self._log.info("Loading Hyperliquid instruments...")
        await self._instrument_provider.load_all_async()
        
        # Send instruments to data engine
        self._send_all_instruments_to_data_engine()

        # Setup WebSocket message handler and connect
        if self._ws_client:
            self._log.info("Setting up Hyperliquid WebSocket message handler...")
            self._ws_client.set_message_handler(self._on_ws_message)
            
            self._log.info("Connecting to Hyperliquid WebSocket...")
            await self._ws_client.connect()
            
            # Verify connection
            if self._ws_client.is_connected():
                self._log.info("Connected to Hyperliquid WebSocket", LogColor.GREEN)
            else:
                self._log.error("Failed to establish WebSocket connection")
                raise RuntimeError("WebSocket connection failed")

    async def _disconnect(self) -> None:
        # Disconnect WebSocket
        if self._ws_client:
            self._log.info("Disconnecting from Hyperliquid WebSocket...")
            await self._ws_client.disconnect()
            self._log.info("Disconnected from Hyperliquid WebSocket", LogColor.BLUE)

        # Clear subscriptions
        self._subscribed_instruments.clear()

    def _on_ws_message(self, message: str) -> None:
        """Handle incoming WebSocket message."""
        try:
            import json
            data = json.loads(message)
            self._log.debug(f"Received WebSocket message: {data}")
            
            # Parse different message types
            if isinstance(data, dict):
                # Check for different Hyperliquid message types
                if "channel" in data:
                    channel = data["channel"]
                    if channel == "allMids":
                        self._handle_all_mids(data.get("data", {}))
                    elif channel == "l2Book":
                        self._handle_l2_book(data.get("data", {}))
                    elif channel == "trades":
                        self._handle_trades(data.get("data", {}))
                elif "mids" in data:
                    # Direct allMids data format
                    self._handle_all_mids(data)
                elif "type" in data:
                    # Handle subscription responses or other message types
                    msg_type = data["type"]
                    self._log.debug(f"Received message type: {msg_type}")
                else:
                    # Handle direct data updates (common format)
                    self._handle_generic_data(data)
            
        except Exception as e:
            self._log.error(f"Error processing WebSocket message: {e}")

    def _handle_all_mids(self, data: dict) -> None:
        """Handle allMids price data and convert to quote ticks."""
        try:
            from nautilus_trader.model.data import QuoteTick
            from nautilus_trader.model.objects import Price, Quantity
            
            self._log.debug(f"📊 Received allMids data with {len(data)} items")
            
            if "mids" in data:
                mids = data["mids"]
                # mids is a dictionary like {'BTC': '98234.0', 'ETH': '3456.7', ...}
                if isinstance(mids, dict):
                    for coin, price_str in mids.items():
                        # Create instrument ID (handle special cases)
                        if coin.startswith('k'):
                            # Handle kBONK, kPEPE, etc. - remove 'k' prefix
                            base_coin = coin[1:]
                            instrument_id = InstrumentId.from_str(f"{base_coin}-PERP.HYPERLIQUID")
                        elif '/' in coin:
                            # Handle PURR/USDC type - use first part
                            base_coin = coin.split('/')[0]
                            instrument_id = InstrumentId.from_str(f"{base_coin}-PERP.HYPERLIQUID")
                        else:
                            instrument_id = InstrumentId.from_str(f"{coin}-PERP.HYPERLIQUID")
                        
                        # Check if we're subscribed to this instrument
                        if instrument_id in self._subscribed_instruments:
                            try:
                                price = Price.from_str(price_str)
                                
                                # Create a quote tick with bid=ask=mid price
                                quote_tick = QuoteTick(
                                    instrument_id=instrument_id,
                                    bid_price=price,
                                    ask_price=price, 
                                    bid_size=Quantity.from_int(0),  # Size not available in allMids
                                    ask_size=Quantity.from_int(0),
                                    ts_event=self._clock.timestamp_ns(),
                                    ts_init=self._clock.timestamp_ns(),
                                )
                                
                                self._handle_data(quote_tick)
                                self._log.info(f"💰 Quote tick: {instrument_id} | Price: {price}")
                                
                            except Exception as e:
                                self._log.error(f"Error creating quote tick for {coin}: {e}")
            
        except Exception as e:
            self._log.error(f"Error handling allMids data: {e}")

    def _handle_l2_book(self, data: dict) -> None:
        """Handle L2 book data."""
        try:
            self._log.info(f"📚 Received L2 book data: {data}")
            # TODO: Implement order book processing
        except Exception as e:
            self._log.error(f"Error handling L2 book data: {e}")

    def _handle_trades(self, data: dict) -> None:
        """Handle trade data and convert to trade ticks."""
        try:
            from nautilus_trader.core.uuid import UUID4
            from nautilus_trader.model.data import TradeTick
            from nautilus_trader.model.enums import AggressorSide
            from nautilus_trader.model.objects import Price, Quantity
            
            self._log.info(f"📈 Received trades data: {data}")
            
            if isinstance(data, list):
                for trade_data in data:
                    if isinstance(trade_data, dict) and all(k in trade_data for k in ["coin", "px", "sz", "side"]):
                        coin = trade_data["coin"]
                        price_str = trade_data["px"]
                        size_str = trade_data["sz"]
                        side = trade_data["side"]
                        
                        # Create instrument ID
                        instrument_id = InstrumentId.from_str(f"{coin}-PERP.HYPERLIQUID")
                        
                        # Check if we're subscribed to this instrument
                        if instrument_id in self._subscribed_instruments:
                            try:
                                price = Price.from_str(price_str)
                                size = Quantity.from_str(size_str)
                                aggressor_side = AggressorSide.BUYER if side.lower() == "b" else AggressorSide.SELLER
                                
                                from nautilus_trader.model.identifiers import TradeId
                                
                                trade_id = TradeId(str(trade_data.get("tid", "0")))
                                
                                trade_tick = TradeTick(
                                    instrument_id=instrument_id,
                                    price=price,
                                    size=size,
                                    aggressor_side=aggressor_side,
                                    trade_id=trade_id,
                                    ts_event=self._clock.timestamp_ns(),
                                    ts_init=self._clock.timestamp_ns(),
                                )
                                
                                self._handle_data(trade_tick)
                                self._log.debug(f"📈 Processed trade tick for {instrument_id}: {price} x {size}")
                                
                            except Exception as e:
                                self._log.error(f"Error creating trade tick for {coin}: {e}")
            
        except Exception as e:
            self._log.error(f"Error handling trades data: {e}")

    def _handle_generic_data(self, data: dict) -> None:
        """Handle generic data formats."""
        try:
            self._log.debug(f"Received generic data: {data}")
            # Log data structure to understand format
            self._log.info(f"🔍 Unknown data format: {list(data.keys()) if isinstance(data, dict) else type(data)}")
        except Exception as e:
            self._log.error(f"Error handling generic data: {e}")

    def _send_all_instruments_to_data_engine(self) -> None:
        """Send all instruments to the data engine."""
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        instruments = self._instrument_provider.get_all()
        self._log.info(f"📊 Sending {len(instruments)} instruments to data engine")
        
        for instrument in instruments.values():
            # Add to cache first
            self._cache.add_instrument(instrument)
            # Then send to data engine
            self._handle_data(instrument)

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        # Instruments are loaded on connection
        pass

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        # Instruments are loaded on connection
        pass

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        self._subscribed_instruments.add(command.instrument_id)
        self._log.info(f"📚 Subscribed to order book for {command.instrument_id}")
        
        # Extract coin symbol for subscription
        coin = command.instrument_id.symbol.value.replace("-PERP", "")
        if self._ws_client:
            await self._ws_client.subscribe_l2_book(coin)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        self._subscribed_instruments.add(command.instrument_id)
        self._log.info(f"📊 Subscribed to quotes for {command.instrument_id}")
        
        if self._ws_client:
            await self._ws_client.subscribe_all_mids()

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        self._subscribed_instruments.add(command.instrument_id)
        self._log.info(f"📈 Subscribed to trades for {command.instrument_id}")
        
        # Extract coin symbol for subscription
        coin = command.instrument_id.symbol.value.replace("-PERP", "")
        if self._ws_client:
            await self._ws_client.subscribe_trades(coin)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        self._log.error("Bar subscriptions are not yet supported")

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        pass

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        pass

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        self._subscribed_instruments.discard(command.instrument_id)
        self._log.info(f"📚 Unsubscribed from order book for {command.instrument_id}")

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        self._subscribed_instruments.discard(command.instrument_id)
        self._log.info(f"📊 Unsubscribed from quotes for {command.instrument_id}")

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        self._subscribed_instruments.discard(command.instrument_id)
        self._log.info(f"📈 Unsubscribed from trades for {command.instrument_id}")

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        pass

    # -- REQUESTS ---------------------------------------------------------------------------------

    async def _request_instrument(self, request: RequestInstrument) -> None:
        # Instruments are pre-loaded
        pass

    async def _request_instruments(self, request: RequestInstruments) -> None:
        # Instruments are pre-loaded
        pass

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        self._log.error("Historical quote tick requests are not supported")

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        self._log.error("Historical trade tick requests are not supported")

    async def _request_bars(self, request: RequestBars) -> None:
        self._log.error("Historical bar requests are not supported")