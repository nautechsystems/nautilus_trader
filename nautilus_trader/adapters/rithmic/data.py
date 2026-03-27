"""Live data client for Rithmic."""

from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING

from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar, BarAggregation, BarType
from nautilus_trader.model.data import QuoteTick as NautilusQuoteTick
from nautilus_trader.model.data import TradeTick as NautilusTradeTick
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId, InstrumentId, TradeId, Venue
from nautilus_trader.model.objects import Price, Quantity

from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic.config import to_binding_environment
from nautilus_trader.adapters.rithmic.providers import normalize_rithmic_symbol, resolve_exchange_hint

if TYPE_CHECKING:
    from nautilus_trader.cache import Cache
    from nautilus_trader.common.component import MessageBus


RITHMIC_VENUE = Venue("RITHMIC")
_HANDLER_EXCEPTIONS = (AttributeError, KeyError, LookupError, RuntimeError, TypeError, ValueError)


class RithmicLiveDataClient(LiveMarketDataClient):
    """
    Provides a live data client for Rithmic.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client_id : ClientId
        The client ID.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    config : RithmicDataClientConfig
        The configuration for the client.
    """

    def __init__(
        self,
        loop,
        client_id,
        msgbus: "MessageBus",
        cache: "Cache",
        clock,
        config: RithmicDataClientConfig,
    ) -> None:
        from nautilus_trader.adapters.rithmic.providers import RithmicInstrumentProvider

        if not isinstance(client_id, ClientId):
            client_id = ClientId(str(client_id))

        super().__init__(
            loop=loop,
            client_id=client_id,
            venue=RITHMIC_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=RithmicInstrumentProvider(config),
        )
        self._config = config
        self._gateway = None
        self._rust_client = None
        self._subscription_channels: dict[tuple[str, str], set[str]] = {}
        self._subscription_mode: dict[tuple[str, str], str] = {}
        self._bar_subscriptions: dict[tuple[str, str, str, int], BarType] = {}

    @property
    def venue(self) -> Venue:
        """Return the venue."""
        return RITHMIC_VENUE

    async def _connect(self) -> None:
        """Connect to the Rithmic ticker plant."""
        try:
            from nautilus_trader.adapters.rithmic.bindings import RithmicDataClient
            from nautilus_trader.adapters.rithmic.bindings import RithmicGateway
        except ImportError as e:
            raise ImportError(
                "Failed to import Rust bindings. Make sure the native extension is built."
            ) from e

        # Create gateway from config
        self._gateway = RithmicGateway(
            environment=to_binding_environment(self._config.environment),
            username=self._config.username,
            password=self._config.password,
            system_name=self._config.system_name,
            app_name=self._config.app_name,
            app_version=self._config.app_version,
            fcm_id=self._config.fcm_id or "",
            ib_id=self._config.ib_id or "",
            account_id="",  # Data client doesn't need account
            enable_ticker=True,
            enable_order=False,
            enable_pnl=False,
            enable_history=self._config.enable_history,
        )

        # Connect gateway
        await self._gateway.connect()

        bind_gateway = getattr(self._instrument_provider, "bind_gateway", None)
        if bind_gateway is not None:
            bind_gateway(self._gateway)

        await self._instrument_provider.initialize()
        self._send_all_instruments_to_data_engine()

        # Create data client
        self._rust_client = RithmicDataClient(self._gateway)

        # Set up callback for market data events
        self._rust_client.set_data_callback(self._on_market_data)

        # Start Rust event loop dispatching
        await self._rust_client.start_event_loop()

        self._log.info("Connected to Rithmic ticker plant")

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self._instrument_provider.get_all().values():
            self._publish_instrument_to_data_engine(instrument)

    def _publish_instrument_to_data_engine(self, instrument) -> None:
        if self._cache.instrument(instrument.id) is None:
            self._handle_data(instrument)

        currency = getattr(instrument, "currency", None)
        if currency is not None:
            self._cache.add_currency(currency)

    def _warn_unsupported(self, capability: str) -> None:
        self._log.warning(f"{capability} not implemented for Rithmic")

    async def _disconnect(self) -> None:
        """Disconnect from the Rithmic ticker plant."""
        if self._rust_client:
            self._rust_client.stop_event_loop()
            self._rust_client.clear_data_callback()
            self._rust_client.unsubscribe_all()
            self._rust_client = None
            self._subscription_channels.clear()
            self._subscription_mode.clear()
            self._bar_subscriptions.clear()

        if self._gateway:
            clear_binding = getattr(self._instrument_provider, "clear_gateway_binding", None)
            if clear_binding is not None:
                clear_binding()
            gateway = self._gateway
            self._gateway = None
            try:
                await asyncio.wait_for(gateway.disconnect(), timeout=3.0)
            except asyncio.TimeoutError:
                self._log.warning(
                    "Timed out waiting for Rithmic gateway disconnect; continuing local shutdown",
                )

        self._log.info("Disconnected from Rithmic ticker plant")

    def _on_market_data(self, event) -> None:
        """Handle market data events from Rust."""
        try:
            if event.is_quote():
                quote = event.as_quote()
                self._handle_quote_tick(quote)
            elif event.is_trade():
                trade = event.as_trade()
                self._handle_trade_tick(trade)
            elif event.is_bar():
                bar = event.as_bar()
                self._handle_live_bar(bar)
            elif event.is_error():
                error = event.as_error()
                self._log.error(f"Market data error: {error}")
        except _HANDLER_EXCEPTIONS as e:
            self._log.exception("Error handling market data event", e)

    def _handle_quote_tick(self, tick) -> None:
        """Convert Rust QuoteTick to Nautilus QuoteTick and publish."""
        instrument_id = InstrumentId.from_str(f"{tick.symbol}.{RITHMIC_VENUE.value}")

        # Get instrument for precision info
        instrument = self._lookup_instrument(instrument_id)
        if instrument is None:
            self._log.warning(f"No instrument found for {instrument_id}, using default precision")
            price_precision = 2
            size_precision = 0
        else:
            price_precision = instrument.price_precision
            size_precision = instrument.size_precision

        nautilus_tick = NautilusQuoteTick(
            instrument_id=instrument_id,
            bid_price=Price(tick.bid_price, price_precision),
            ask_price=Price(tick.ask_price, price_precision),
            bid_size=Quantity(tick.bid_size, size_precision),
            ask_size=Quantity(tick.ask_size, size_precision),
            ts_event=tick.ts_event,
            ts_init=tick.ts_init,
        )

        self._handle_data(nautilus_tick)

    def _handle_trade_tick(self, tick) -> None:
        """Convert Rust TradeTick to Nautilus TradeTick and publish."""
        from nautilus_trader.model.enums import AggressorSide

        instrument_id = InstrumentId.from_str(f"{tick.symbol}.{RITHMIC_VENUE.value}")

        # Get instrument for precision info
        instrument = self._lookup_instrument(instrument_id)
        if instrument is None:
            self._log.warning(f"No instrument found for {instrument_id}, using default precision")
            price_precision = 2
            size_precision = 0
        else:
            price_precision = instrument.price_precision
            size_precision = instrument.size_precision

        # Map aggressor side
        if tick.aggressor_side == "BUY":
            aggressor_side = AggressorSide.BUYER
        elif tick.aggressor_side == "SELL":
            aggressor_side = AggressorSide.SELLER
        else:
            aggressor_side = AggressorSide.NO_AGGRESSOR

        fallback_trade_id = tick.trade_id or format(tick.ts_event, "x")

        nautilus_tick = NautilusTradeTick(
            instrument_id=instrument_id,
            price=Price(tick.price, price_precision),
            size=Quantity(tick.size, size_precision),
            aggressor_side=aggressor_side,
            trade_id=TradeId(fallback_trade_id),
            ts_event=tick.ts_event,
            ts_init=tick.ts_init,
        )

        self._handle_data(nautilus_tick)

    def _handle_live_bar(self, tick) -> None:
        """Convert a live Rithmic time bar update into a Nautilus bar."""
        key = self._bar_subscription_key(
            tick.symbol,
            tick.exchange,
            tick.bar_kind,
            tick.bar_period,
        )
        bar_type = self._bar_subscriptions.get(key)
        if bar_type is None:
            self._log.warning(
                "Received live Rithmic bar with no matching subscription: "
                f"{tick.exchange}:{tick.symbol} {tick.bar_kind}/{tick.bar_period}"
            )
            return

        instrument = self._lookup_instrument(bar_type.instrument_id)
        if instrument is None:
            self._log.warning(f"Instrument not loaded for live bar {bar_type.instrument_id}")
            return

        open_price = tick.open_price or 0.0
        high_price = tick.high_price or open_price
        low_price = tick.low_price or open_price
        close_price = tick.close_price or open_price

        if high_price < low_price:
            high_price, low_price = low_price, high_price
        if high_price < open_price:
            high_price = open_price
        if high_price < close_price:
            high_price = close_price
        if low_price > open_price:
            low_price = open_price
        if low_price > close_price:
            low_price = close_price

        self._handle_data(
            Bar(
                bar_type=bar_type,
                open=Price(open_price, instrument.price_precision),
                high=Price(high_price, instrument.price_precision),
                low=Price(low_price, instrument.price_precision),
                close=Price(close_price, instrument.price_precision),
                volume=Quantity(tick.volume, instrument.size_precision),
                ts_event=tick.ts_event,
                ts_init=tick.ts_init,
            )
        )

    def _subscription_key(self, symbol: str, exchange: str) -> tuple[str, str]:
        return (symbol, exchange)

    def _logical_subscriptions_for(self, symbol: str, exchange: str) -> set[str]:
        key = self._subscription_key(symbol, exchange)
        return self._subscription_channels.setdefault(key, set())

    def _bar_subscription_key(
        self,
        symbol: str,
        exchange: str,
        bar_type_name: str,
        bar_period: int,
    ) -> tuple[str, str, str, int]:
        return (symbol, exchange, bar_type_name, bar_period)

    async def _subscribe_order_book_deltas(self, command) -> None:
        """Subscribe to order book deltas."""
        # Rithmic order book support is limited
        self._log.warning("Order book deltas not fully implemented for Rithmic")

    async def _subscribe_order_book_depth(self, command) -> None:
        """Subscribe to order book depth."""
        self._warn_unsupported("Order book depth subscriptions")

    async def _subscribe_instruments(self, command) -> None:
        """Load and publish the current venue instrument snapshot."""
        await self._instrument_provider.load_all_async(getattr(command, "params", None))
        self._send_all_instruments_to_data_engine()
        self._log.info(
            "Rithmic does not provide streaming instrument updates; loaded current instrument snapshot"
        )

    async def _subscribe_instrument(self, command) -> None:
        """Load and publish a specific instrument definition."""
        instrument_id = command.instrument_id
        exchange = self._resolve_exchange(instrument_id, getattr(command, "params", None))
        if exchange is None:
            raise ValueError(f"Missing exchange for instrument {instrument_id}")

        await self._ensure_instrument_loaded(instrument_id, exchange)
        self._log.info(
            f"Loaded instrument definition for {instrument_id}; streaming instrument updates are not available"
        )

    async def _subscribe_quote_ticks(self, command) -> None:
        """Subscribe to quote ticks (best bid/offer)."""
        if not self._rust_client:
            raise RuntimeError("Not connected")

        instrument_id = command.instrument_id
        symbol = self._resolve_rithmic_symbol(instrument_id)
        exchange = self._resolve_exchange(instrument_id, getattr(command, "params", None))
        if exchange is None:
            raise ValueError(f"Missing exchange for instrument {instrument_id}")

        await self._ensure_instrument_loaded(instrument_id, exchange)
        logical = self._logical_subscriptions_for(symbol, exchange)
        if "quotes" in logical:
            return
        if self._subscription_mode.get(self._subscription_key(symbol, exchange)) == "trades":
            logical.add("quotes")
            self._log.info(f"Reusing trade subscription for quotes on {symbol} ({exchange})")
            return

        await self._rust_client.subscribe_quotes(symbol, exchange)
        logical.add("quotes")
        self._subscription_mode[self._subscription_key(symbol, exchange)] = "quotes"
        self._log.info(f"Subscribed to quotes for {symbol} on {exchange}")

    async def _subscribe_trade_ticks(self, command) -> None:
        """Subscribe to trade ticks."""
        if not self._rust_client:
            raise RuntimeError("Not connected")

        instrument_id = command.instrument_id
        symbol = self._resolve_rithmic_symbol(instrument_id)
        exchange = self._resolve_exchange(instrument_id, getattr(command, "params", None))
        if exchange is None:
            raise ValueError(f"Missing exchange for instrument {instrument_id}")

        await self._ensure_instrument_loaded(instrument_id, exchange)
        logical = self._logical_subscriptions_for(symbol, exchange)
        if "trades" in logical:
            return
        mode = self._subscription_mode.get(self._subscription_key(symbol, exchange))
        if mode == "quotes":
            logical.add("trades")
            self._log.warning(
                f"Trade subscription requested after quote subscription for {symbol} on {exchange}; "
                "keeping existing venue subscription",
            )
            return

        # Rithmic sends both quotes and trades with single subscription
        await self._rust_client.subscribe_trades(symbol, exchange)
        logical.add("trades")
        self._subscription_mode[self._subscription_key(symbol, exchange)] = "trades"
        self._log.info(f"Subscribed to trades for {symbol} on {exchange}")

    async def _subscribe_mark_prices(self, command) -> None:
        """Subscribe to mark prices."""
        self._warn_unsupported("Mark price subscriptions")

    async def _subscribe_index_prices(self, command) -> None:
        """Subscribe to index prices."""
        self._warn_unsupported("Index price subscriptions")

    async def _subscribe_funding_rates(self, command) -> None:
        """Subscribe to funding rates."""
        self._warn_unsupported("Funding rate subscriptions")

    async def _subscribe_bars(self, command) -> None:
        """Subscribe to live bars."""
        if not self._rust_client:
            raise RuntimeError("Not connected")
        if not self._config.enable_history:
            raise RuntimeError("Live bars requested, but Rithmic history is disabled")

        bar_type = command.bar_type
        if not bar_type.is_externally_aggregated():
            self._log.error(
                f"Cannot subscribe to {bar_type}: only EXTERNAL bars are supported, "
                "use INTERNAL aggregation instead",
            )
            return
        if bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot subscribe to {bar_type}: only LAST price bars are supported",
            )
            return
        if not bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot subscribe to {bar_type}: only time bars are supported",
            )
            return

        instrument_id = bar_type.instrument_id
        symbol = self._resolve_rithmic_symbol(instrument_id)
        exchange = self._resolve_exchange(instrument_id, getattr(command, "params", None))
        if exchange is None:
            raise ValueError(f"Missing exchange for instrument {instrument_id}")

        await self._ensure_instrument_loaded(instrument_id, exchange)

        bar_type_name = self._resolve_bar_type_name(bar_type)
        bar_period = int(bar_type.spec.step)
        key = self._bar_subscription_key(symbol, exchange, bar_type_name, bar_period)
        if key in self._bar_subscriptions:
            return

        await self._rust_client.subscribe_bars(symbol, exchange, bar_type_name, bar_period)
        self._bar_subscriptions[key] = bar_type
        self._log.info(
            f"Subscribed to live {bar_type_name} bars(period={bar_period}) for {symbol} on {exchange}"
        )

    async def _subscribe_instrument_status(self, command) -> None:
        """Subscribe to instrument status updates."""
        self._warn_unsupported("Instrument status subscriptions")

    async def _subscribe_instrument_close(self, command) -> None:
        """Subscribe to instrument close updates."""
        self._warn_unsupported("Instrument close subscriptions")

    async def _subscribe_option_greeks(self, command) -> None:
        """Subscribe to option greeks."""
        self._warn_unsupported("Option greeks subscriptions")

    async def _unsubscribe_order_book_deltas(self, command) -> None:
        """Unsubscribe from order book deltas."""
        self._log.warning("Order book unsubscribe not fully implemented for Rithmic")

    async def _unsubscribe_order_book_depth(self, command) -> None:
        """Unsubscribe from order book depth."""
        self._warn_unsupported("Order book depth unsubscriptions")

    async def _unsubscribe_instruments(self, command) -> None:
        """Unsubscribe from instrument snapshots."""
        self._log.debug("Rithmic instrument snapshots do not maintain a live subscription")

    async def _unsubscribe_instrument(self, command) -> None:
        """Unsubscribe from a single instrument snapshot."""
        self._log.debug("Rithmic instrument snapshots do not maintain a live subscription")

    async def _unsubscribe_quote_ticks(self, command) -> None:
        """Unsubscribe from quote ticks."""
        if not self._rust_client:
            return

        instrument_id = command.instrument_id
        symbol = self._resolve_rithmic_symbol(instrument_id)
        exchange = self._resolve_exchange(instrument_id, getattr(command, "params", None))
        if exchange is None:
            return

        key = self._subscription_key(symbol, exchange)
        logical = self._subscription_channels.get(key)
        if logical is None or "quotes" not in logical:
            return

        logical.discard("quotes")
        if logical:
            return

        await self._rust_client.unsubscribe(symbol, exchange)
        self._subscription_channels.pop(key, None)
        self._subscription_mode.pop(key, None)
        self._log.info(f"Unsubscribed from quotes for {symbol} on {exchange}")

    async def _unsubscribe_trade_ticks(self, command) -> None:
        """Unsubscribe from trade ticks."""
        if not self._rust_client:
            return

        instrument_id = command.instrument_id
        symbol = self._resolve_rithmic_symbol(instrument_id)
        exchange = self._resolve_exchange(instrument_id, getattr(command, "params", None))
        if exchange is None:
            return

        key = self._subscription_key(symbol, exchange)
        logical = self._subscription_channels.get(key)
        if logical is None or "trades" not in logical:
            return

        logical.discard("trades")
        if logical:
            return

        await self._rust_client.unsubscribe(symbol, exchange)
        self._subscription_channels.pop(key, None)
        self._subscription_mode.pop(key, None)
        self._log.info(f"Unsubscribed from trades for {symbol} on {exchange}")

    async def _unsubscribe_mark_prices(self, command) -> None:
        """Unsubscribe from mark prices."""
        self._warn_unsupported("Mark price unsubscriptions")

    async def _unsubscribe_index_prices(self, command) -> None:
        """Unsubscribe from index prices."""
        self._warn_unsupported("Index price unsubscriptions")

    async def _unsubscribe_funding_rates(self, command) -> None:
        """Unsubscribe from funding rates."""
        self._warn_unsupported("Funding rate unsubscriptions")

    async def _unsubscribe_bars(self, command) -> None:
        """Unsubscribe from live bars."""
        if not self._rust_client:
            return

        bar_type = command.bar_type
        if not bar_type.spec.is_time_aggregated():
            return

        instrument_id = bar_type.instrument_id
        symbol = self._resolve_rithmic_symbol(instrument_id)
        exchange = self._resolve_exchange(instrument_id, getattr(command, "params", None))
        if exchange is None:
            return

        bar_type_name = self._resolve_bar_type_name(bar_type)
        bar_period = int(bar_type.spec.step)
        key = self._bar_subscription_key(symbol, exchange, bar_type_name, bar_period)
        if key not in self._bar_subscriptions:
            return

        await self._rust_client.unsubscribe_bars(symbol, exchange, bar_type_name, bar_period)
        self._bar_subscriptions.pop(key, None)
        self._log.info(
            f"Unsubscribed from live {bar_type_name} bars(period={bar_period}) for {symbol} on {exchange}"
        )

    async def _unsubscribe_instrument_status(self, command) -> None:
        """Unsubscribe from instrument status."""
        self._warn_unsupported("Instrument status unsubscriptions")

    async def _unsubscribe_instrument_close(self, command) -> None:
        """Unsubscribe from instrument close."""
        self._warn_unsupported("Instrument close unsubscriptions")

    async def _unsubscribe_option_greeks(self, command) -> None:
        """Unsubscribe from option greeks."""
        self._warn_unsupported("Option greeks unsubscriptions")

    async def _request(self, request) -> None:
        """Request custom data."""
        self._log.warning(f"Data request not implemented: {request}")

    async def _request_order_book_deltas(self, request) -> None:
        """Request order book deltas."""
        self._warn_unsupported("Historical order book delta requests")

    async def _request_order_book_depth(self, request) -> None:
        """Request order book depth."""
        self._warn_unsupported("Historical order book depth requests")

    async def _request_order_book_snapshot(self, request) -> None:
        """Request order book snapshots."""
        self._warn_unsupported("Order book snapshot requests")

    async def _request_instrument(self, request) -> None:
        """Request an instrument definition."""
        instrument_id = request.instrument_id
        exchange = self._resolve_exchange(instrument_id, request.params)
        if exchange is None:
            raise ValueError(f"Missing exchange for instrument {instrument_id}")

        await self._instrument_provider.load_async(
            instrument_id,
            filters={"exchange": exchange},
        )

        instrument = self._lookup_instrument(instrument_id)
        if instrument is None:
            self._log.warning(f"Instrument not found after load: {instrument_id}")
            return

        self._publish_instrument_to_data_engine(instrument)
        self._handle_instrument(
            instrument,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_instruments(self, request) -> None:
        """Request all instruments for the venue."""
        await self._instrument_provider.load_all_async(request.params)
        instruments = list(self._instrument_provider.get_all().values())
        for instrument in instruments:
            self._publish_instrument_to_data_engine(instrument)
        self._handle_instruments(
            request.venue,
            instruments,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_quote_ticks(self, request) -> None:
        """Request historical quote ticks."""
        self._warn_unsupported("Historical quote tick requests")

    async def _request_trade_ticks(self, request) -> None:
        """Request historical trade tick requests."""
        self._warn_unsupported("Historical trade tick requests")

    async def _request_bars(self, request) -> None:
        """Request historical bars."""
        if not self._config.enable_history:
            raise RuntimeError("Historical bars requested, but Rithmic history is disabled")

        temporary_connection = False
        if not self._rust_client:
            await self._connect()
            temporary_connection = True

        try:
            instrument_id = request.bar_type.instrument_id
            exchange = self._resolve_exchange(instrument_id, request.params)
            if exchange is None:
                raise ValueError(f"Missing exchange for instrument {instrument_id}")

            if request.start is None or request.end is None:
                raise ValueError("Both start and end must be provided for bar requests")

            if request.start >= request.end:
                raise ValueError("Start must be earlier than end for bar requests")

            await self._ensure_instrument_loaded(instrument_id, exchange)

            bar_type_name = self._resolve_bar_type_name(request.bar_type)
            bar_period = int(request.bar_type.spec.step)
            start_time_sec = int(request.start.timestamp())
            end_time_sec = int(request.end.timestamp())

            responses = await self._rust_client.request_bars(
                self._resolve_rithmic_symbol(instrument_id),
                exchange,
                bar_type_name,
                bar_period,
                start_time_sec,
                end_time_sec,
            )

            bars = self._convert_time_bars(
                request.bar_type,
                responses,
                instrument_id,
            )
            self._handle_bars(
                request.bar_type,
                bars,
                request.id,
                request.start,
                request.end,
                request.params,
            )
        finally:
            if temporary_connection:
                await self._disconnect()

    async def _request_funding_rates(self, request) -> None:
        """Request funding rates."""
        self._warn_unsupported("Funding rate requests")

    async def _request_forward_prices(self, request) -> None:
        """Request forward prices."""
        self._warn_unsupported("Forward price requests")

    async def _ensure_instrument_loaded(self, instrument_id: InstrumentId, exchange: str) -> None:
        instrument = self._lookup_instrument(instrument_id)
        if instrument is not None:
            self._publish_instrument_to_data_engine(instrument)
            return

        await self._instrument_provider.load_async(
            instrument_id,
            filters={"exchange": exchange},
        )

        instrument = self._lookup_instrument(instrument_id)
        if instrument is None:
            raise ValueError(f"Instrument not found after load: {instrument_id}")

        self._publish_instrument_to_data_engine(instrument)

    def _lookup_instrument(self, instrument_id: InstrumentId):
        candidates = [instrument_id]
        symbol = self._resolve_rithmic_symbol(instrument_id)
        if symbol != instrument_id.symbol.value:
            candidates.append(InstrumentId.from_str(f"{symbol}.{RITHMIC_VENUE.value}"))

        for candidate in candidates:
            instrument = self._cache.instrument(candidate)
            if instrument is not None:
                return instrument

            instrument = self._instrument_provider.find(candidate)
            if instrument is not None:
                return instrument

        return None

    def _resolve_rithmic_symbol(self, instrument_id: InstrumentId) -> str:
        return normalize_rithmic_symbol(instrument_id.symbol.value)

    def _resolve_exchange(self, instrument_id: InstrumentId, params: dict | None = None) -> str | None:
        instrument = self._lookup_instrument(instrument_id)
        if instrument is not None:
            exchange = getattr(instrument, "exchange", None)
            if exchange:
                return exchange

            info = getattr(instrument, "info", None)
            if isinstance(info, dict):
                exchange = info.get("exchange")
                if exchange:
                    return exchange

        return resolve_exchange_hint(instrument_id.symbol.value, params)

    def _resolve_bar_type_name(self, bar_type: BarType) -> str:
        aggregation = bar_type.spec.aggregation
        if aggregation == BarAggregation.SECOND:
            return "SecondBar"
        if aggregation == BarAggregation.MINUTE:
            return "MinuteBar"
        if aggregation == BarAggregation.DAY:
            return "DailyBar"
        if aggregation == BarAggregation.WEEK:
            return "WeeklyBar"

        raise ValueError(f"Unsupported bar aggregation: {aggregation}")

    def _convert_time_bars(self, bar_type: BarType, responses, instrument_id: InstrumentId):
        instrument = self._lookup_instrument(instrument_id)
        if instrument is None:
            raise ValueError(f"Instrument not loaded: {instrument_id}")

        price_precision = instrument.price_precision
        size_precision = instrument.size_precision

        bars = []
        for response in responses:
            open_price = response.open_price or 0.0
            high_price = response.high_price or open_price
            low_price = response.low_price or open_price
            close_price = response.close_price or open_price
            volume = response.volume or 0

            if not response.period and not any(
                (open_price, high_price, low_price, close_price, volume)
            ):
                continue

            ts_event = self._bar_timestamp_from_response(response, bar_type)
            ts_init = ts_event

            # Validate and correct OHLC relationships if needed
            ohlc_corrected = False
            if high_price < low_price:
                high_price, low_price = low_price, high_price
                ohlc_corrected = True
            if high_price < open_price:
                high_price = open_price
                ohlc_corrected = True
            if high_price < close_price:
                high_price = close_price
                ohlc_corrected = True
            if low_price > open_price:
                low_price = open_price
                ohlc_corrected = True
            if low_price > close_price:
                low_price = close_price
                ohlc_corrected = True

            if ohlc_corrected:
                self._log.warning(
                    f"Corrected invalid OHLC data for {instrument_id} at {ts_event}"
                )

            bars.append(
                Bar(
                    bar_type=bar_type,
                    open=Price(open_price, price_precision),
                    high=Price(high_price, price_precision),
                    low=Price(low_price, price_precision),
                    close=Price(close_price, price_precision),
                    volume=Quantity(volume, size_precision),
                    ts_event=ts_event,
                    ts_init=ts_init,
                )
            )

        return bars

    def _bar_timestamp_from_response(self, response, bar_type: BarType) -> int:
        """Extract bar timestamp from Rithmic response.

        Prefer the Rithmic `marker` field when present, which is the bar's
        epoch-based marker in seconds. Fall back to parsing `period` only when
        `marker` is unavailable.
        """
        marker = getattr(response, "marker", None)
        if marker:
            try:
                return int(marker) * 1_000_000_000
            except (TypeError, ValueError):
                self._log.warning(f"Could not parse bar marker '{marker}' as timestamp")

        if response.period:
            try:
                seconds = int(response.period)
                return seconds * 1_000_000_000
            except ValueError:
                self._log.warning(
                    f"Could not parse bar period '{response.period}' as timestamp"
                )

        return self._fallback_bar_timestamp(bar_type)

    def _fallback_bar_timestamp(self, bar_type: BarType) -> int:
        step = bar_type.spec.step
        aggregation = bar_type.spec.aggregation
        if aggregation == BarAggregation.SECOND:
            return step * 1_000_000_000
        if aggregation == BarAggregation.MINUTE:
            return step * 60 * 1_000_000_000
        if aggregation == BarAggregation.DAY:
            return step * 24 * 60 * 60 * 1_000_000_000
        if aggregation == BarAggregation.WEEK:
            return step * 7 * 24 * 60 * 60 * 1_000_000_000

        return 0
