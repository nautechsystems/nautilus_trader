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


def shared_reference_quote_key(
    *,
    profile_id: str,
    account_scope_id: str,
    instrument_id: InstrumentId | str,
) -> str:
    resolved_instrument_id = _coerce_instrument_id(instrument_id)
    return FluxRedisKeys.profile_market_last(
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
        self._snapshot_keys: dict[InstrumentId, str] = {}
        self._last_snapshot_messages: dict[InstrumentId, bytes] = {}
        self._listener_task: asyncio.Task | None = None

    @property
    def instrument_provider(self) -> SharedReferenceInstrumentProvider:
        return self._instrument_provider  # type: ignore[return-value]

    async def _connect(self) -> None:
        await self.instrument_provider.initialize()
        if self._listener_task is not None and self._listener_task.done():
            self._listener_task = None
        self._ensure_pubsub()
        if self._listener_task is None:
            self._listener_task = self.create_task(
                self._listen_for_shared_reference_updates(),
                log_msg="shared_reference_listener",
            )

    async def _disconnect(self) -> None:
        pubsub = self._pubsub
        self._pubsub = None
        if pubsub is not None:
            with suppress(Exception):
                pubsub.close()
        listener_task = self._listener_task
        self._listener_task = None
        if listener_task is not None:
            if not listener_task.done():
                listener_task.cancel()
            with suppress(asyncio.CancelledError, Exception):
                await listener_task
        close = getattr(self._redis, "close", None)
        if callable(close):
            with suppress(Exception):
                close()

    async def _subscribe_quote_ticks(self, command) -> None:
        instrument_id = command.instrument_id
        channel = shared_reference_quote_channel(
            profile_id=self._client_config.profile_id,
            account_scope_id=self._client_config.account_scope_id,
            instrument_id=instrument_id,
        )
        self._subscriptions[instrument_id] = channel
        self._channel_to_instrument_id[channel] = instrument_id
        self._ensure_pubsub().subscribe(channel)
        snapshot_key = shared_reference_quote_key(
            profile_id=self._client_config.profile_id,
            account_scope_id=self._client_config.account_scope_id,
            instrument_id=instrument_id,
        )
        self._snapshot_keys[instrument_id] = snapshot_key
        self._ingest_shared_reference_message(
            instrument_id=instrument_id,
            data=self._redis.get(snapshot_key),
        )

    async def _unsubscribe_quote_ticks(self, command) -> None:
        self._snapshot_keys.pop(command.instrument_id, None)
        self._last_snapshot_messages.pop(command.instrument_id, None)
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

    async def recover_quote_ticks(self, instrument_id: InstrumentId) -> dict[str, object]:
        if instrument_id not in self._subscriptions:
            return {
                "instrument_id": instrument_id.value,
                "ok": False,
                "status": "not_subscribed",
                "error_summary": None,
                "cache_refreshed": False,
            }

        self._reset_pubsub()
        self._rebuild_pubsub_subscriptions()
        snapshot_key = self._snapshot_keys.get(instrument_id)
        cache_refreshed = False
        if snapshot_key is not None:
            self._last_snapshot_messages.pop(instrument_id, None)
            self._ingest_shared_reference_message(
                instrument_id=instrument_id,
                data=self._redis.get(snapshot_key),
            )
            cache_refreshed = True
        return {
            "instrument_id": instrument_id.value,
            "ok": True,
            "status": "replayed",
            "error_summary": None,
            "cache_refreshed": cache_refreshed,
        }

    async def _listen_for_shared_reference_updates(self) -> None:
        while True:
            try:
                if self._pubsub is None:
                    self._rebuild_pubsub_subscriptions()
                    await asyncio.sleep(self._client_config.subscription_poll_interval_secs)
                    continue

                message = self._pubsub.get_message(ignore_subscribe_messages=True, timeout=0)
                if message:
                    channel = _optional_text(message.get("channel"))
                    if channel is not None:
                        instrument_id = self._channel_to_instrument_id.get(channel)
                        if instrument_id is not None:
                            self._ingest_shared_reference_message(
                                instrument_id=instrument_id,
                                data=message.get("data"),
                            )

                self._poll_snapshot_keys()
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                self._log.warning(
                    f"Shared reference listener error; rebuilding subscriptions: {exc}"
                )
                self._reset_pubsub()
            await asyncio.sleep(self._client_config.subscription_poll_interval_secs)

    def _ensure_pubsub(self):
        if self._pubsub is None:
            self._pubsub = self._redis.pubsub()
        return self._pubsub

    def _reset_pubsub(self) -> None:
        pubsub = self._pubsub
        self._pubsub = None
        if pubsub is not None:
            with suppress(Exception):
                pubsub.close()

    def _rebuild_pubsub_subscriptions(self) -> None:
        pubsub = self._ensure_pubsub()
        for channel in self._subscriptions.values():
            pubsub.subscribe(channel)

    def _poll_snapshot_keys(self) -> None:
        for instrument_id, snapshot_key in tuple(self._snapshot_keys.items()):
            self._ingest_shared_reference_message(
                instrument_id=instrument_id,
                data=self._redis.get(snapshot_key),
            )

    def _ingest_shared_reference_message(
        self,
        *,
        instrument_id: InstrumentId,
        data: Any,
    ) -> None:
        normalized = _normalize_shared_reference_message(data)
        if normalized is None:
            return
        if self._last_snapshot_messages.get(instrument_id) == normalized:
            return
        payload = _decode_shared_reference_message(normalized)
        if payload is None:
            return
        self._last_snapshot_messages[instrument_id] = normalized
        self.handle_shared_reference_snapshot(
            instrument_id=instrument_id,
            payload=payload,
        )


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


def _normalize_shared_reference_message(data: Any) -> bytes | None:
    if data is None:
        return None
    if isinstance(data, bytes):
        return data
    if isinstance(data, bytearray):
        return bytes(data)
    if isinstance(data, str):
        return data.encode()
    if isinstance(data, Mapping):
        return json.dumps(dict(data), sort_keys=True).encode()
    return None


__all__ = [
    "InteractiveBrokersSharedReferenceDataClient",
    "SharedReferenceInstrumentProvider",
    "build_shared_reference_quote_tick",
    "shared_reference_quote_key",
    "shared_reference_quote_channel",
]
