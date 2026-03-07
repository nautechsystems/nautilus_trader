# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
import json
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.bitget.config import BitgetDataClientConfig
from nautilus_trader.adapters.bitget.constants import BITGET_DEFAULT_PRODUCTS
from nautilus_trader.adapters.bitget.constants import BITGET_VENUE
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeData
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


class BitgetDataClient(LiveMarketDataClient):
    """Bitget live data client for public market data streams."""

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: Any,
        msgbus: MessageBus,
        cache: Any,
        clock: LiveClock,
        instrument_provider: InstrumentProvider,
        config: BitgetDataClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or "BITGET"),
            venue=BITGET_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )
        self._http_client = client
        self._instrument_provider = instrument_provider
        self._config = config
        self._product_types = list(config.product_types) if config.product_types else list(
            BITGET_DEFAULT_PRODUCTS,
        )
        self._environment = (
            nautilus_pyo3.BitgetEnvironment.DEMO
            if config.demo
            else nautilus_pyo3.BitgetEnvironment.MAINNET
        )
        self._ws_client: Any | None = None
        self._ws_tasks: set[asyncio.Future] = set()
        self._book_states: dict[tuple[str, str], Any] = {}
        self._active_trade_subs: set[InstrumentId] = set()
        self._active_book_subs: set[InstrumentId] = set()
        self._ticker_subscriptions: dict[str, set[str]] = {}
        self._ticker_instruments: dict[str, InstrumentId] = {}
        self._bar_subscriptions: dict[tuple[str, str], int] = {}
        self._bar_instruments: dict[tuple[str, str], InstrumentId] = {}
        self._instrument_index: dict[tuple[str, str], Any] = {}
        self._update_instruments_interval_mins = config.update_instruments_interval_mins
        self._update_instruments_task: asyncio.Task | None = None

        self._log.info(
            f"Bitget product types: {[self._product_type_key(p) for p in self._product_types]}",
            LogColor.BLUE,
        )
        self._log.info(f"{config.demo=}", LogColor.BLUE)
        self._log.info(f"{config.base_url_ws_public=}", LogColor.BLUE)
        self._log.info(f"{config.update_instruments_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)

    @property
    def instrument_provider(self) -> InstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._rebuild_instrument_index()
        self._send_all_instruments_to_data_engine()

        ws_client = nautilus_pyo3.BitgetWebSocketClient(self._environment)
        ws_config = ws_client.websocket_config(
            base_url=self._config.base_url_ws_public,
            retry_delay_initial_ms=self._config.retry_delay_initial_ms,
            retry_delay_max_ms=self._config.retry_delay_max_ms,
        )

        self._ws_client = await nautilus_pyo3.WebSocketClient.connect(
            loop_=self._loop,
            config=ws_config,
            handler=self._handle_ws_message,
            post_reconnection=self._handle_ws_reconnect,
        )
        self._log.info(f"Connected to Bitget WebSocket {ws_config.url}", LogColor.BLUE)

        if self._update_instruments_interval_mins:
            self._update_instruments_task = self.create_task(
                self._update_instruments(self._update_instruments_interval_mins),
                log_msg="bitget:update_instruments",
            )

    async def _disconnect(self) -> None:
        if self._update_instruments_task:
            self._update_instruments_task.cancel()
            self._update_instruments_task = None

        await asyncio.sleep(1.0)

        if self._ws_client and not self._ws_client.is_closed():
            await self._ws_client.disconnect()
            self._log.info("Disconnected from Bitget WebSocket", LogColor.BLUE)

        await cancel_tasks_with_timeout(
            self._ws_tasks,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )

        self._ws_tasks.clear()
        self._book_states.clear()
        self._active_trade_subs.clear()
        self._active_book_subs.clear()
        self._ticker_subscriptions.clear()
        self._ticker_instruments.clear()
        self._bar_subscriptions.clear()
        self._bar_instruments.clear()
        self._instrument_index.clear()

    async def _subscribe(self, command: SubscribeData) -> None:
        self._log.debug(f"Unhandled subscribe command: {command}")

    async def _unsubscribe(self, command: UnsubscribeData) -> None:
        self._log.debug(f"Unhandled unsubscribe command: {command}")

    async def _request(self, request: RequestData) -> None:
        self._log.debug(f"Unhandled request: {request}")

    async def _subscribe_instruments(self, command: Any) -> None:
        if self._update_instruments_interval_mins:
            self._log.info(
                f"Bitget instruments are refreshed by polling every {self._update_instruments_interval_mins} minutes",
                LogColor.BLUE,
            )
        else:
            self._log.warning(
                "Bitget instrument subscription requested but update_instruments_interval_mins is not configured",
            )

    async def _subscribe_instrument(self, command: Any) -> None:
        await self._subscribe_instruments(command)

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        await self._subscribe_order_book(command)

    async def _subscribe_order_book_depth(self, command: SubscribeOrderBook) -> None:
        self._log.warning(
            f"Order book depth snapshots are not streamed by Bitget; use L2 deltas instead for {command.instrument_id}",
        )

    async def _subscribe_quote_ticks(self, command: Any) -> None:
        await self._subscribe_ticker_stream(command.instrument_id, "quotes")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        await self._subscribe_trade_ticks_by_id(command.instrument_id)

    async def _subscribe_mark_prices(self, command: Any) -> None:
        await self._subscribe_ticker_stream(command.instrument_id, "mark_prices")

    async def _subscribe_index_prices(self, command: Any) -> None:
        await self._subscribe_ticker_stream(command.instrument_id, "index_prices")

    async def _subscribe_funding_rates(self, command: Any) -> None:
        await self._subscribe_ticker_stream(command.instrument_id, "funding_rates")

    async def _subscribe_bars(self, command: Any) -> None:
        bar_type = command.bar_type
        if bar_type.is_internally_aggregated():
            self._log.error(
                f"Cannot subscribe to {bar_type}: Bitget only publishes EXTERNAL bars",
            )
            return
        if not bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot subscribe to {bar_type}: Bitget only publishes time bars",
            )
            return
        if bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot subscribe to {bar_type}: Bitget only publishes LAST price bars",
            )
            return

        interval = self._bitget_bar_interval(bar_type)
        if interval is None:
            self._log.error(f"Cannot subscribe to unsupported Bitget bar type {bar_type}")
            return

        instrument = self._instrument_provider.find(bar_type.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find Bitget instrument for {bar_type.instrument_id}")
            return

        product_type = self._instrument_product_type(instrument)
        if product_type not in self._product_types:
            return

        channel = f"candle{interval}"
        key = (bar_type.instrument_id.value, channel)
        if self._bar_subscriptions.get(key, 0) == 0:
            message = nautilus_pyo3.BitgetWebSocketClient.subscribe_candle_message(
                product_type,
                interval,
                instrument.raw_symbol.value,
            )
            await self._send_ws_text(message)
            self._bar_instruments[key] = bar_type.instrument_id
        self._bar_subscriptions[key] = self._bar_subscriptions.get(key, 0) + 1

    async def _subscribe_instrument_status(self, command: Any) -> None:
        self._log.warning(
            f"Instrument status subscriptions are not implemented for Bitget yet: {command.instrument_id}",
        )

    async def _subscribe_instrument_close(self, command: Any) -> None:
        self._log.warning(
            f"Instrument close subscriptions are not implemented for Bitget yet: {command.instrument_id}",
        )

    async def _unsubscribe_instruments(self, command: Any) -> None:
        return None

    async def _unsubscribe_instrument(self, command: Any) -> None:
        return None

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        await self._unsubscribe_order_book(command.instrument_id)

    async def _unsubscribe_order_book_depth(self, command: UnsubscribeOrderBook) -> None:
        return None

    async def _unsubscribe_quote_ticks(self, command: Any) -> None:
        await self._unsubscribe_ticker_stream(command.instrument_id, "quotes")

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        await self._unsubscribe_trade_ticks_by_id(command.instrument_id)

    async def _unsubscribe_mark_prices(self, command: Any) -> None:
        await self._unsubscribe_ticker_stream(command.instrument_id, "mark_prices")

    async def _unsubscribe_index_prices(self, command: Any) -> None:
        await self._unsubscribe_ticker_stream(command.instrument_id, "index_prices")

    async def _unsubscribe_funding_rates(self, command: Any) -> None:
        await self._unsubscribe_ticker_stream(command.instrument_id, "funding_rates")

    async def _unsubscribe_bars(self, command: Any) -> None:
        bar_type = command.bar_type
        interval = self._bitget_bar_interval(bar_type)
        if interval is None:
            return

        key = (bar_type.instrument_id.value, f"candle{interval}")
        count = self._bar_subscriptions.get(key, 0)
        if count <= 1:
            instrument = self._instrument_provider.find(bar_type.instrument_id)
            if instrument is not None:
                product_type = self._instrument_product_type(instrument)
                message = nautilus_pyo3.BitgetWebSocketClient.unsubscribe_candle_message(
                    product_type,
                    interval,
                    instrument.raw_symbol.value,
                )
                await self._send_ws_text(message)
            self._bar_subscriptions.pop(key, None)
            self._bar_instruments.pop(key, None)
            return

        self._bar_subscriptions[key] = count - 1

    async def _unsubscribe_instrument_status(self, command: Any) -> None:
        return None

    async def _unsubscribe_instrument_close(self, command: Any) -> None:
        return None

    async def _request_instrument(self, request: Any) -> None:
        instrument = self._instrument_provider.find(request.instrument_id)
        if instrument is None:
            return

        product_type = self._instrument_product_type(instrument)
        if product_type not in self._product_types:
            return

        self._handle_data(instrument)

    async def _request_instruments(self, request: Any) -> None:
        await self._instrument_provider.initialize(reload=True)
        self._rebuild_instrument_index()
        self._send_all_instruments_to_data_engine()

    async def _request_quote_ticks(self, request: Any) -> None:
        self._log.error(
            "Cannot request historical quotes: not published by Bitget. Subscribe to quotes or L1_MBP order book",
        )

    async def _request_trade_ticks(self, request: Any) -> None:
        self._log.warning(f"Trade tick requests are not implemented for Bitget yet: {request.instrument_id}")

    async def _request_funding_rates(self, request: Any) -> None:
        instrument = self._instrument_provider.find(request.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find Bitget instrument for {request.instrument_id}")
            return

        product_type = self._instrument_product_type(instrument)
        if product_type not in self._product_types:
            return
        if self._is_spot_product_type(product_type):
            self._log.warning(
                f"Funding rates not applicable for SPOT instrument {request.instrument_id}",
            )
            return

        start_ms = self._datetime_to_millis(request.start)
        end_ms = self._datetime_to_millis(request.end)
        limit = None if request.limit == 0 else request.limit

        rates: list[FundingRateUpdate] = []

        if start_ms is None and end_ms is None:
            payload = await self._http_client.request_funding_rates(
                product_type,
                instrument.raw_symbol.value,
            )
            for entry in json.loads(payload or "[]"):
                ts_event_ms = self._parse_timestamp_ms(entry.get("nextFundingTime")) or (
                    self._clock.timestamp_ns() // 1_000_000
                )
                next_funding_ms = self._parse_timestamp_ms(entry.get("nextFundingTime"))
                rates.append(
                    FundingRateUpdate(
                        instrument.id,
                        Decimal(str(entry.get("fundingRate") or "0")),
                        ts_event=ts_event_ms * 1_000_000,
                        ts_init=self._clock.timestamp_ns(),
                        next_funding_ns=(next_funding_ms * 1_000_000) if next_funding_ms else None,
                    ),
                )
        else:
            page = 1
            remaining = limit
            while True:
                page_limit = min(remaining or 100, 100)
                history_payload = await self._http_client.request_funding_rate_history(
                    product_type,
                    instrument.raw_symbol.value,
                    page,
                    page_limit,
                )
                entries = json.loads(history_payload or "[]")
                if not entries:
                    break

                stop = False
                for entry in entries:
                    funding_ms = self._parse_timestamp_ms(entry.get("fundingTime"))
                    if funding_ms is None:
                        continue
                    if start_ms is not None and funding_ms < start_ms:
                        continue
                    if end_ms is not None and funding_ms > end_ms:
                        continue

                    rates.append(
                        FundingRateUpdate(
                            instrument.id,
                            Decimal(str(entry.get("fundingRate") or "0")),
                            ts_event=funding_ms * 1_000_000,
                            ts_init=self._clock.timestamp_ns(),
                            next_funding_ns=None,
                        ),
                    )
                    if remaining is not None:
                        remaining -= 1
                        if remaining == 0:
                            stop = True
                            break

                if stop or len(entries) < page_limit:
                    break
                page += 1

        rates.sort(key=lambda update: update.ts_event)
        self._handle_funding_rates(
            request.instrument_id,
            rates,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_bars(self, request: Any) -> None:
        if request.bar_type.is_internally_aggregated():
            self._log.error(
                f"Cannot request {request.bar_type} bars: only EXTERNAL aggregation is available from Bitget",
            )
            return
        if not request.bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot request {request.bar_type} bars: Bitget only publishes time bars",
            )
            return
        if request.bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot request {request.bar_type} bars: Bitget only publishes LAST price bars",
            )
            return

        interval = self._bitget_bar_interval(request.bar_type)
        if interval is None:
            self._log.error(f"Cannot request unsupported Bitget bar type {request.bar_type}")
            return

        instrument = self._instrument_provider.find(request.bar_type.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find Bitget instrument for {request.bar_type.instrument_id}")
            return

        product_type = self._instrument_product_type(instrument)
        if product_type not in self._product_types:
            return

        payload = await self._http_client.request_bars(
            product_type,
            instrument.raw_symbol.value,
            interval,
            self._datetime_to_millis(request.start),
            self._datetime_to_millis(request.end),
            None if request.limit == 0 else request.limit,
        )
        rows = json.loads(payload or "[]")
        synthetic = json.dumps(
            {
                "arg": {
                    "channel": f"candle{interval}",
                    "instType": BitgetDataClient._product_type_key(self, product_type),
                    "instId": instrument.raw_symbol.value,
                },
                "data": rows,
            },
        )
        bars = [
            capsule_to_data(capsule)
            for capsule in nautilus_pyo3.BitgetWebSocketClient.parse_bars(synthetic, instrument)
        ]
        bars.sort(key=lambda bar: bar.ts_event)
        self._handle_bars(
            request.bar_type,
            bars,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_order_book_deltas(self, request: Any) -> None:
        await self._request_order_book_snapshot(request)

    async def _request_order_book_depth(self, request: Any) -> None:
        await self._request_order_book_snapshot(request)

    async def _request_order_book_snapshot(self, request: Any) -> None:
        instrument = self._instrument_provider.find(request.instrument_id)
        if instrument is None:
            self._log.warning(f"Cannot find Bitget instrument for {request.instrument_id}")
            return

        product_type = self._instrument_product_type(instrument)
        if product_type not in self._product_types:
            return

        snapshot = await self._http_client.request_order_book_snapshot(
            instrument.raw_symbol.value,
            product_type,
            instrument,
        )
        self._handle_data(capsule_to_data(snapshot))

    def _rebuild_instrument_index(self) -> None:
        self._instrument_index.clear()

        for instrument in self._instrument_provider.get_all().values():
            product_type = self._instrument_product_type(instrument)
            if product_type not in self._product_types:
                continue

            key = (self._product_type_key(product_type), instrument.raw_symbol.value)
            self._instrument_index[key] = instrument

    def _send_all_instruments_to_data_engine(self) -> None:
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        for instrument in self._instrument_provider.get_all().values():
            product_type = self._instrument_product_type(instrument)
            if product_type not in self._product_types:
                continue
            self._handle_data(instrument)

    async def _update_instruments(self, interval_mins: int) -> None:
        while True:
            try:
                await asyncio.sleep(interval_mins * 60)
                await self._instrument_provider.initialize(reload=True)
                self._rebuild_instrument_index()
                self._send_all_instruments_to_data_engine()
                self._log.info(
                    f"Scheduled task 'update_instruments' to run in {interval_mins} minutes",
                    LogColor.BLUE,
                )
            except asyncio.CancelledError:
                self._log.debug("Canceled task 'update_instruments'")
                return
            except Exception as e:
                self._log.error(f"Error updating Bitget instruments: {e}")

    def _handle_ws_reconnect(self) -> None:
        self._loop.call_soon_threadsafe(self._on_ws_reconnect)

    def _on_ws_reconnect(self) -> None:
        self._log.warning("Bitget public WebSocket reconnected; resubscribing active channels")
        self._book_states.clear()

        for instrument_id in list(self._active_trade_subs):
            task = self.create_task(
                self._subscribe_trade_ticks_by_id(instrument_id),
                log_msg=f"bitget:resubscribe_trade:{instrument_id.value}",
            )
            if task:
                self._ws_tasks.add(task)

        for instrument_id in list(getattr(self, "_ticker_instruments", {}).values()):
            task = self.create_task(
                self._resubscribe_ticker(instrument_id),
                log_msg=f"bitget:resubscribe_ticker:{instrument_id.value}",
            )
            if task:
                self._ws_tasks.add(task)

        for (instrument_key, channel), instrument_id in list(getattr(self, "_bar_instruments", {}).items()):
            task = self.create_task(
                self._resubscribe_bar(instrument_id, channel),
                log_msg=f"bitget:resubscribe_bar:{instrument_key}:{channel}",
            )
            if task:
                self._ws_tasks.add(task)

        for instrument_id in list(self._active_book_subs):
            task = self.create_task(
                self._recover_order_book(instrument_id),
                log_msg=f"bitget:recover_book:{instrument_id.value}",
            )
            if task:
                self._ws_tasks.add(task)

    def _handle_ws_message(self, raw: bytes) -> None:
        try:
            message = raw.decode("utf-8")
            if not message or message == "pong":
                return

            payload = json.loads(message)
            if isinstance(payload, str):
                if payload == "pong":
                    return
                self._log.debug(f"Bitget WebSocket message: {payload}")
                return

            if not isinstance(payload, dict):
                return

            if payload.get("event"):
                self._log.debug(f"Bitget WebSocket event: {message}")
                return

            arg = payload.get("arg") or {}
            channel = arg.get("channel")
            inst_type = str(arg.get("instType") or "").upper()
            inst_id = str(arg.get("instId") or "")
            if not channel or not inst_type or not inst_id:
                return

            key = (inst_type, inst_id)
            instrument = self._instrument_index.get(key)
            if instrument is None:
                self._log.debug(f"No cached Bitget instrument for websocket key {key}")
                return

            if channel == "trade":
                for capsule in nautilus_pyo3.BitgetWebSocketClient.parse_trade_ticks(
                    message,
                    instrument,
                ):
                    self._handle_data(capsule_to_data(capsule))
                return

            if channel == "ticker":
                ticker_kinds = self._ticker_subscriptions.get(instrument.id.value, set())
                if "quotes" in ticker_kinds:
                    for capsule in nautilus_pyo3.BitgetWebSocketClient.parse_quote_ticks(
                        message,
                        instrument,
                    ):
                        self._handle_data(capsule_to_data(capsule))
                if "mark_prices" in ticker_kinds:
                    for capsule in nautilus_pyo3.BitgetWebSocketClient.parse_mark_prices(
                        message,
                        instrument,
                    ):
                        self._handle_data(capsule_to_data(capsule))
                if "index_prices" in ticker_kinds:
                    for capsule in nautilus_pyo3.BitgetWebSocketClient.parse_index_prices(
                        message,
                        instrument,
                    ):
                        self._handle_data(capsule_to_data(capsule))
                if "funding_rates" in ticker_kinds:
                    for funding_rate in nautilus_pyo3.BitgetWebSocketClient.parse_funding_rates(
                        message,
                        instrument,
                    ):
                        self._handle_data(FundingRateUpdate.from_pyo3(funding_rate))
                return

            if str(channel).startswith("books"):
                state = self._book_states.setdefault(key, nautilus_pyo3.BitgetBookState())
                try:
                    capsule = state.apply_message(message, instrument)
                except Exception as e:
                    self._log.warning(f"Bitget book desync for {instrument.id}: {e}")
                    state.reset()
                    task = self.create_task(
                        self._recover_order_book(instrument.id),
                        log_msg=f"bitget:resync_book:{instrument.id.value}",
                    )
                    if task:
                        self._ws_tasks.add(task)
                    return

                self._handle_data(capsule_to_data(capsule))
                return

            if str(channel).startswith("candle"):
                bar_key = (instrument.id.value, str(channel))
                if bar_key not in self._bar_subscriptions:
                    return
                for capsule in nautilus_pyo3.BitgetWebSocketClient.parse_bars(
                    message,
                    instrument,
                ):
                    self._handle_data(capsule_to_data(capsule))
                return

            self._log.debug(f"Unhandled Bitget websocket channel: {channel}")
        except Exception as e:
            self._log.exception("Error handling Bitget websocket message", e)

    async def _send_ws_text(self, text: str) -> None:
        if self._ws_client is None:
            self._log.warning("Bitget WebSocket not connected")
            return

        await self._ws_client.send_text(text.encode("utf-8"))

    async def _subscribe_trade_ticks_by_id(self, instrument_id: InstrumentId) -> None:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find Bitget instrument for {instrument_id}")
            return

        product_type = self._instrument_product_type(instrument)
        message = nautilus_pyo3.BitgetWebSocketClient.subscribe_message(
            product_type,
            "trade",
            instrument.raw_symbol.value,
        )
        await self._send_ws_text(message)
        self._active_trade_subs.add(instrument_id)

    async def _unsubscribe_trade_ticks_by_id(self, instrument_id: InstrumentId) -> None:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find Bitget instrument for {instrument_id}")
            return

        product_type = self._instrument_product_type(instrument)
        message = nautilus_pyo3.BitgetWebSocketClient.unsubscribe_message(
            product_type,
            "trade",
            instrument.raw_symbol.value,
        )
        await self._send_ws_text(message)
        self._active_trade_subs.discard(instrument_id)

    async def _subscribe_ticker_stream(self, instrument_id: InstrumentId, stream: str) -> None:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find Bitget instrument for {instrument_id}")
            return

        product_type = self._instrument_product_type(instrument)
        if product_type not in self._product_types:
            return
        if stream in {"mark_prices", "index_prices", "funding_rates"} and self._is_spot_product_type(
            product_type,
        ):
            self._log.warning(f"Cannot subscribe to {stream} for SPOT instrument {instrument_id}")
            return

        key = instrument_id.value
        if key not in self._ticker_subscriptions:
            message = nautilus_pyo3.BitgetWebSocketClient.subscribe_ticker_message(
                product_type,
                instrument.raw_symbol.value,
            )
            await self._send_ws_text(message)
            self._ticker_subscriptions[key] = set()
            self._ticker_instruments[key] = instrument_id

        self._ticker_subscriptions[key].add(stream)

    async def _unsubscribe_ticker_stream(self, instrument_id: InstrumentId, stream: str) -> None:
        key = instrument_id.value
        streams = self._ticker_subscriptions.get(key)
        if not streams:
            return

        streams.discard(stream)
        if streams:
            return

        instrument = self._instrument_provider.find(instrument_id)
        if instrument is not None:
            product_type = self._instrument_product_type(instrument)
            message = nautilus_pyo3.BitgetWebSocketClient.unsubscribe_ticker_message(
                product_type,
                instrument.raw_symbol.value,
            )
            await self._send_ws_text(message)

        self._ticker_subscriptions.pop(key, None)
        self._ticker_instruments.pop(key, None)

    async def _resubscribe_ticker(self, instrument_id: InstrumentId) -> None:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            return

        product_type = self._instrument_product_type(instrument)
        message = nautilus_pyo3.BitgetWebSocketClient.subscribe_ticker_message(
            product_type,
            instrument.raw_symbol.value,
        )
        await self._send_ws_text(message)

    async def _resubscribe_bar(self, instrument_id: InstrumentId, channel: str) -> None:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            return

        product_type = self._instrument_product_type(instrument)
        message = nautilus_pyo3.BitgetWebSocketClient.subscribe_candle_message(
            product_type,
            channel,
            instrument.raw_symbol.value,
        )
        await self._send_ws_text(message)

    async def _subscribe_order_book(self, command: SubscribeOrderBook) -> None:
        if command.book_type != BookType.L2_MBP:
            self._log.warning(
                f"Book type {book_type_to_str(command.book_type)} not supported by Bitget, skipping subscription",
            )
            return

        await self._subscribe_order_book_by_id(command.instrument_id)

    async def _subscribe_order_book_by_id(self, instrument_id: InstrumentId) -> None:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find Bitget instrument for {instrument_id}")
            return

        product_type = self._instrument_product_type(instrument)
        key = (self._product_type_key(product_type), instrument.raw_symbol.value)
        self._book_states[key] = nautilus_pyo3.BitgetBookState()

        message = nautilus_pyo3.BitgetWebSocketClient.subscribe_message(
            product_type,
            "books",
            instrument.raw_symbol.value,
        )
        await self._send_ws_text(message)
        self._active_book_subs.add(instrument_id)

    async def _unsubscribe_order_book(self, instrument_id: InstrumentId) -> None:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find Bitget instrument for {instrument_id}")
            return

        product_type = self._instrument_product_type(instrument)
        key = (self._product_type_key(product_type), instrument.raw_symbol.value)
        message = nautilus_pyo3.BitgetWebSocketClient.unsubscribe_message(
            product_type,
            "books",
            instrument.raw_symbol.value,
        )
        await self._send_ws_text(message)
        self._book_states.pop(key, None)
        self._active_book_subs.discard(instrument_id)

    async def _resubscribe_order_book(self, instrument_id: InstrumentId) -> None:
        await self._unsubscribe_order_book(instrument_id)
        await self._subscribe_order_book_by_id(instrument_id)

    async def _recover_order_book(self, instrument_id: InstrumentId) -> None:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find Bitget instrument for {instrument_id}")
            return

        product_type = self._instrument_product_type(instrument)

        try:
            snapshot = await self._http_client.request_order_book_snapshot(
                instrument.raw_symbol.value,
                product_type,
                instrument,
            )
        except Exception as e:
            self._log.warning(
                f"Failed to fetch Bitget order book snapshot for {instrument_id}: {e}",
            )
        else:
            self._handle_data(capsule_to_data(snapshot))

        await self._resubscribe_order_book(instrument_id)

    def _instrument_product_type(self, instrument: Any) -> object:
        settlement_code = self._currency_code(getattr(instrument, "settlement_currency", None))
        quote_code = self._currency_code(getattr(instrument, "quote_currency", None))
        base_code = self._currency_code(
            getattr(instrument, "base_currency", None) or getattr(instrument, "underlying", None),
        )
        if settlement_code:
            if settlement_code == base_code:
                return nautilus_pyo3.BitgetProductType.COIN_FUTURES
            if settlement_code == "USDC" or quote_code == "USDC":
                return nautilus_pyo3.BitgetProductType.USDC_FUTURES
            if settlement_code == "USDT" or quote_code == "USDT":
                return nautilus_pyo3.BitgetProductType.USDT_FUTURES

        symbol = instrument.id.symbol.value
        if symbol.endswith("-PERP") or self._is_delivery_symbol(symbol):
            raw_symbol = symbol[:-5] if symbol.endswith("-PERP") else symbol.rsplit("-", 1)[0]
            if raw_symbol.endswith("USDC"):
                return nautilus_pyo3.BitgetProductType.USDC_FUTURES
            if raw_symbol.endswith("USD") and not raw_symbol.endswith("USDT") and not raw_symbol.endswith("USDC"):
                return nautilus_pyo3.BitgetProductType.COIN_FUTURES
            return nautilus_pyo3.BitgetProductType.USDT_FUTURES
        return nautilus_pyo3.BitgetProductType.SPOT

    def _product_type_key(self, product_type: object) -> str:
        normalized = str(product_type or "").strip().upper().replace("_", "-")
        if product_type == nautilus_pyo3.BitgetProductType.SPOT or normalized.endswith("SPOT"):
            return "SPOT"
        if (
            product_type == nautilus_pyo3.BitgetProductType.USDT_FUTURES
            or normalized.endswith("USDT-FUTURES")
        ):
            return "USDT-FUTURES"
        if (
            product_type == nautilus_pyo3.BitgetProductType.COIN_FUTURES
            or normalized.endswith("COIN-FUTURES")
        ):
            return "COIN-FUTURES"
        if (
            product_type == nautilus_pyo3.BitgetProductType.USDC_FUTURES
            or normalized.endswith("USDC-FUTURES")
        ):
            return "USDC-FUTURES"
        return normalized

    def _is_spot_product_type(self, product_type: object) -> bool:
        return self._product_type_key(product_type) == "SPOT"

    @staticmethod
    def _currency_code(currency: Any) -> str:
        if currency is None:
            return ""
        code = getattr(currency, "code", None)
        if code is None:
            return str(currency).strip().upper()
        if hasattr(code, "as_str"):
            return str(code.as_str()).strip().upper()
        return str(code).strip().upper()

    @staticmethod
    def _datetime_to_millis(value: Any) -> int | None:
        if value is None:
            return None
        return int(ensure_pydatetime_utc(value).timestamp() * 1000)

    @staticmethod
    def _parse_timestamp_ms(value: Any) -> int | None:
        if value in (None, "", 0, "0"):
            return None
        return int(str(value))

    @staticmethod
    def _bitget_bar_interval(bar_type: Any) -> str | None:
        mapping = {
            (BarAggregation.SECOND, 1): "1s",
            (BarAggregation.MINUTE, 1): "1m",
            (BarAggregation.MINUTE, 3): "3m",
            (BarAggregation.MINUTE, 5): "5m",
            (BarAggregation.MINUTE, 15): "15m",
            (BarAggregation.MINUTE, 30): "30m",
            (BarAggregation.HOUR, 1): "1H",
            (BarAggregation.HOUR, 2): "2H",
            (BarAggregation.HOUR, 4): "4H",
            (BarAggregation.HOUR, 6): "6H",
            (BarAggregation.HOUR, 12): "12H",
            (BarAggregation.DAY, 1): "1D",
            (BarAggregation.DAY, 2): "2D",
            (BarAggregation.DAY, 3): "3D",
            (BarAggregation.DAY, 5): "5D",
            (BarAggregation.WEEK, 1): "1W",
            (BarAggregation.MONTH, 1): "1M",
            (BarAggregation.MONTH, 3): "3M",
            (BarAggregation.MONTH, 6): "6M",
            (BarAggregation.MONTH, 12): "1y",
        }
        return mapping.get((bar_type.spec.aggregation, bar_type.spec.step))

    @staticmethod
    def _is_delivery_symbol(symbol: str) -> bool:
        _, _, suffix = symbol.rpartition("-")
        return len(suffix) == 6 and suffix.isdigit()
