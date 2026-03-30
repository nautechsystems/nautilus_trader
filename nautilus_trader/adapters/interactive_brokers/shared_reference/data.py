from __future__ import annotations

import asyncio
import json
from collections.abc import Mapping
from contextlib import suppress
from decimal import Decimal
from typing import Any

import redis

from flux.common.keys import FluxRedisKeys
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.shared_reference.config import (
    InteractiveBrokersSharedReferenceDataClientConfig,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.identifiers import Symbol


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, (bytes, bytearray)):
        value = value.decode()
    text = str(value).strip()
    return text or None


def _coerce_instrument_id(instrument_id: InstrumentId | str) -> InstrumentId:
    if isinstance(instrument_id, InstrumentId):
        return instrument_id
    return InstrumentId.from_str(str(instrument_id))


def _decimal_places(value: Any) -> int:
    text = str(value).strip()
    if "." not in text:
        return 0
    return len(text.rsplit(".", maxsplit=1)[-1].rstrip("0"))


def shared_reference_quote_channel(
    *,
    profile_id: str,
    account_scope_id: str,
    instrument_id: InstrumentId | str,
) -> str:
    resolved_instrument_id = _coerce_instrument_id(instrument_id)
    return FluxRedisKeys.profile_market_last_channel(
        profile_id=profile_id,
        account_scope_id=account_scope_id,
        exchange="ibkr",
        instrument_id=str(resolved_instrument_id),
    )


def build_shared_reference_quote_tick(
    *,
    payload: Mapping[str, Any],
    instrument_id: InstrumentId | str,
    ts_init_ns: int,
) -> QuoteTick:
    resolved_instrument_id = _coerce_instrument_id(payload.get("instrument_id") or instrument_id)
    ts_event_ms = payload.get("ts_event_ms", payload.get("ts_event"))
    if ts_event_ms is None:
        raise ValueError("Shared reference payload requires `ts_event_ms` or `ts_event`")

    price_precision = max(_decimal_places(payload["bid"]), _decimal_places(payload["ask"]))
    bid = Price.from_str(f"{Decimal(str(payload['bid'])):.{price_precision}f}")
    ask = Price.from_str(f"{Decimal(str(payload['ask'])):.{price_precision}f}")
    bid_size = Quantity.from_str(str(payload.get("bid_size", 0)))
    ask_size = Quantity.from_str(str(payload.get("ask_size", 0)))
    return QuoteTick(
        instrument_id=resolved_instrument_id,
        bid_price=bid,
        ask_price=ask,
        bid_size=bid_size,
        ask_size=ask_size,
        ts_event=int(ts_event_ms) * 1_000_000,
        ts_init=int(ts_init_ns),
    )


class SharedReferenceInstrumentProvider(InstrumentProvider):
    async def load_all_async(self, filters: dict | None = None) -> None:
        _ = filters
        await self.load_ids_async(
            [
                instrument_id if isinstance(instrument_id, InstrumentId) else InstrumentId.from_str(instrument_id)
                for instrument_id in (self._load_ids_on_start or [])
            ],
            filters,
        )

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        _ = filters
        for instrument_id in instrument_ids:
            self.add(_build_equity_instrument(instrument_id))


def _build_equity_instrument(instrument_id: InstrumentId) -> Equity:
    symbol = Symbol(str(instrument_id.symbol))
    return Equity(
        instrument_id=instrument_id,
        raw_symbol=symbol,
        currency=USD,
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )


def _redis_client_from_config(config: InteractiveBrokersSharedReferenceDataClientConfig) -> redis.Redis:
    return redis.Redis(
        host=config.redis_host,
        port=config.redis_port,
        db=config.redis_db,
        username=config.redis_username,
        password=config.redis_password,
        ssl=config.redis_ssl,
        socket_connect_timeout=config.redis_connect_timeout_secs,
        socket_timeout=config.redis_read_timeout_secs,
        decode_responses=False,
    )


class InteractiveBrokersSharedReferenceDataClient(LiveMarketDataClient):
    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: SharedReferenceInstrumentProvider,
        config: InteractiveBrokersSharedReferenceDataClientConfig,
        redis_client: redis.Redis | None = None,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or f"{IB_VENUE.value}-SHARED-REFERENCE"),
            venue=IB_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )
        self._client_config = config
        self._redis = redis_client or _redis_client_from_config(config)
        self._pubsub = None
        self._subscriptions: dict[InstrumentId, str] = {}
        self._channel_to_instrument_id: dict[str, InstrumentId] = {}
        self._listener_task: asyncio.Task | None = None

    @property
    def instrument_provider(self) -> SharedReferenceInstrumentProvider:
        return self._instrument_provider  # type: ignore[return-value]

    async def _connect(self) -> None:
        await self.instrument_provider.initialize()
        self._ensure_pubsub()
        if self._listener_task is None:
            self._listener_task = self.create_task(
                self._listen_for_shared_reference_updates(),
                log_msg="shared_reference_listener",
            )

    async def _disconnect(self) -> None:
        if self._pubsub is not None:
            with suppress(Exception):
                self._pubsub.close()
        if self._listener_task is not None and not self._listener_task.done():
            self._listener_task.cancel()
        close = getattr(self._redis, "close", None)
        if callable(close):
            with suppress(Exception):
                close()

    async def _subscribe_quote_ticks(self, command) -> None:
        channel = shared_reference_quote_channel(
            profile_id=self._client_config.profile_id,
            account_scope_id=self._client_config.account_scope_id,
            instrument_id=command.instrument_id,
        )
        self._subscriptions[command.instrument_id] = channel
        self._channel_to_instrument_id[channel] = command.instrument_id
        self._ensure_pubsub().subscribe(channel)

    async def _unsubscribe_quote_ticks(self, command) -> None:
        channel = self._subscriptions.pop(command.instrument_id, None)
        if channel is None or self._pubsub is None:
            return
        self._channel_to_instrument_id.pop(channel, None)
        with suppress(Exception):
            self._pubsub.unsubscribe(channel)

    def handle_shared_reference_snapshot(
        self,
        *,
        instrument_id: InstrumentId | str,
        payload: Mapping[str, Any],
    ) -> None:
        tick = build_shared_reference_quote_tick(
            payload=payload,
            instrument_id=instrument_id,
            ts_init_ns=self._clock.timestamp_ns(),
        )
        self._handle_data(tick)

    async def _listen_for_shared_reference_updates(self) -> None:
        while True:
            if self._pubsub is None:
                await asyncio.sleep(self._client_config.subscription_poll_interval_secs)
                continue

            message = self._pubsub.get_message(ignore_subscribe_messages=True, timeout=0)
            if not message:
                await asyncio.sleep(self._client_config.subscription_poll_interval_secs)
                continue

            channel = _optional_text(message.get("channel"))
            if channel is None:
                continue
            instrument_id = self._channel_to_instrument_id.get(channel)
            if instrument_id is None:
                continue
            payload = _decode_shared_reference_message(message.get("data"))
            if payload is None:
                continue
            self.handle_shared_reference_snapshot(
                instrument_id=instrument_id,
                payload=payload,
            )

    def _ensure_pubsub(self):
        if self._pubsub is None:
            self._pubsub = self._redis.pubsub()
        return self._pubsub


def _decode_shared_reference_message(data: Any) -> dict[str, Any] | None:
    if data is None:
        return None
    if isinstance(data, (bytes, bytearray)):
        return json.loads(data.decode())
    if isinstance(data, str):
        return json.loads(data)
    if isinstance(data, Mapping):
        return dict(data)
    return None


__all__ = [
    "InteractiveBrokersSharedReferenceDataClient",
    "SharedReferenceInstrumentProvider",
    "build_shared_reference_quote_tick",
    "shared_reference_quote_channel",
]
