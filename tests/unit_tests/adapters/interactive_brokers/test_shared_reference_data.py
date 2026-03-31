from __future__ import annotations

import asyncio
import json
from contextlib import suppress

import pytest

from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.shared_reference.config import (
    InteractiveBrokersSharedReferenceDataClientConfig,
)
from nautilus_trader.adapters.interactive_brokers.shared_reference.data import (
    InteractiveBrokersSharedReferenceDataClient,
)
from nautilus_trader.adapters.interactive_brokers.shared_reference.data import (
    SharedReferenceInstrumentProvider,
)
from nautilus_trader.adapters.interactive_brokers.shared_reference.data import (
    build_shared_reference_quote_tick,
)
from nautilus_trader.adapters.interactive_brokers.shared_reference.data import (
    shared_reference_quote_channel,
)
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class StubPubSub:
    def __init__(self, messages: list[dict] | None = None) -> None:
        self.messages = list(messages or [])
        self.subscribed: list[str] = []
        self.unsubscribed: list[str] = []

    def subscribe(self, channel: str) -> None:
        self.subscribed.append(channel)

    def unsubscribe(self, channel: str) -> None:
        self.unsubscribed.append(channel)

    def get_message(self, ignore_subscribe_messages: bool = True, timeout: float = 0) -> dict | None:
        _ = ignore_subscribe_messages
        _ = timeout
        if self.messages:
            return self.messages.pop(0)
        return None

    def close(self) -> None:
        return None


class StubRedis:
    def __init__(self, pubsub: StubPubSub, *, values: dict[str, bytes | str] | None = None) -> None:
        self._pubsub = pubsub
        self._values = dict(values or {})

    def pubsub(self) -> StubPubSub:
        return self._pubsub

    def get(self, key: str) -> bytes | str | None:
        return self._values.get(key)

    def close(self) -> None:
        return None


def _make_shared_reference_client(
    *,
    loop: asyncio.AbstractEventLoop,
    pubsub: StubPubSub,
    values: dict[str, bytes | str] | None = None,
) -> InteractiveBrokersSharedReferenceDataClient:
    clock = LiveClock()
    msgbus = MessageBus(
        trader_id=TestIdStubs.trader_id(),
        clock=clock,
    )
    return InteractiveBrokersSharedReferenceDataClient(
        loop=loop,
        msgbus=msgbus,
        cache=TestComponentStubs.cache(),
        clock=clock,
        instrument_provider=SharedReferenceInstrumentProvider(),
        config=InteractiveBrokersSharedReferenceDataClientConfig(
            profile_id="equities",
            account_scope_id="ibkr.reference.main",
            subscription_poll_interval_secs=0.001,
        ),
        redis_client=StubRedis(pubsub, values=values),
    )


def _subscribe_quote_ticks_command(
    instrument_id: InstrumentId,
    ts_init: int,
) -> SubscribeQuoteTicks:
    return SubscribeQuoteTicks(
        client_id=None,
        venue=IB_VENUE,
        instrument_id=instrument_id,
        command_id=UUID4(),
        ts_init=ts_init,
    )


def test_shared_reference_quote_channel_is_profile_scoped() -> None:
    assert shared_reference_quote_channel(
        profile_id="equities",
        account_scope_id="ibkr.reference.main",
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
    ) == "flux:v1:profile:market:last:equities:ibkr.reference.main:ibkr:AAPL.NASDAQ:changed"


def test_build_shared_reference_quote_tick_translates_snapshot_payload() -> None:
    quote = build_shared_reference_quote_tick(
        payload={
            "instrument_id": "AAPL.NASDAQ",
            "bid": 190.25,
            "ask": 190.5,
            "bid_size": 7,
            "ask_size": 9,
            "ts_event_ms": 9_900,
            "route": "SMART",
            "session": "RTH",
        },
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        ts_init_ns=10_000_000_000,
    )

    assert quote.instrument_id == InstrumentId.from_str("AAPL.NASDAQ")
    assert quote.bid_price == Price.from_str("190.25")
    assert quote.ask_price == Price.from_str("190.50")
    assert quote.bid_size == Quantity.from_int(7)
    assert quote.ask_size == Quantity.from_int(9)
    assert quote.ts_event == 9_900_000_000
    assert quote.ts_init == 10_000_000_000


@pytest.mark.asyncio
async def test_shared_reference_data_client_subscribe_quote_ticks_uses_typed_config() -> None:
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    pubsub = StubPubSub()
    client = _make_shared_reference_client(
        loop=asyncio.get_running_loop(),
        pubsub=pubsub,
    )

    await client._subscribe_quote_ticks(
        _subscribe_quote_ticks_command(
            instrument_id=instrument_id,
            ts_init=client._clock.timestamp_ns(),
        ),
    )

    expected_channel = shared_reference_quote_channel(
        profile_id="equities",
        account_scope_id="ibkr.reference.main",
        instrument_id=instrument_id,
    )
    assert client._subscriptions[instrument_id] == expected_channel
    assert pubsub.subscribed == [expected_channel]


@pytest.mark.asyncio
async def test_shared_reference_data_client_subscribe_quote_ticks_replays_current_snapshot() -> None:
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    expected_channel = shared_reference_quote_channel(
        profile_id="equities",
        account_scope_id="ibkr.reference.main",
        instrument_id=instrument_id,
    )
    expected_key = expected_channel.removesuffix(":changed")
    expected_payload = {
        "instrument_id": "AAPL.NASDAQ",
        "bid": 190.25,
        "ask": 190.50,
        "bid_size": 7,
        "ask_size": 9,
        "ts_event_ms": 9_900,
    }
    pubsub = StubPubSub()
    client = _make_shared_reference_client(
        loop=asyncio.get_running_loop(),
        pubsub=pubsub,
        values={expected_key: json.dumps(expected_payload)},
    )

    observed: list[tuple[InstrumentId, dict[str, object]]] = []

    def _capture_snapshot(*, instrument_id: InstrumentId, payload: dict[str, object]) -> None:
        observed.append((instrument_id, payload))

    client.handle_shared_reference_snapshot = _capture_snapshot  # type: ignore[method-assign]

    await client._subscribe_quote_ticks(
        _subscribe_quote_ticks_command(
            instrument_id=instrument_id,
            ts_init=client._clock.timestamp_ns(),
        ),
    )

    assert pubsub.subscribed == [expected_channel]
    assert observed == [(instrument_id, expected_payload)]


@pytest.mark.asyncio
async def test_shared_reference_data_client_listener_decodes_bytes_channel() -> None:
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    expected_channel = shared_reference_quote_channel(
        profile_id="equities",
        account_scope_id="ibkr.reference.main",
        instrument_id=instrument_id,
    )
    expected_payload = {
        "instrument_id": "AAPL.NASDAQ",
        "bid": 190.25,
        "ask": 190.50,
        "bid_size": 7,
        "ask_size": 9,
        "ts_event_ms": 9_900,
    }
    pubsub = StubPubSub(
        messages=[
            {
                "channel": expected_channel.encode(),
                "data": json.dumps(expected_payload).encode(),
            },
        ],
    )
    client = _make_shared_reference_client(
        loop=asyncio.get_running_loop(),
        pubsub=pubsub,
    )
    await client._subscribe_quote_ticks(
        _subscribe_quote_ticks_command(
            instrument_id=instrument_id,
            ts_init=client._clock.timestamp_ns(),
        ),
    )

    received: asyncio.Future[tuple[InstrumentId, dict]] = asyncio.get_running_loop().create_future()

    def _capture_snapshot(*, instrument_id: InstrumentId, payload: dict) -> None:
        if not received.done():
            received.set_result((instrument_id, payload))

    client.handle_shared_reference_snapshot = _capture_snapshot  # type: ignore[method-assign]

    listener_task = asyncio.create_task(client._listen_for_shared_reference_updates())
    try:
        observed_instrument_id, observed_payload = await asyncio.wait_for(received, timeout=0.2)
    finally:
        listener_task.cancel()
        with suppress(asyncio.CancelledError):
            await listener_task

    assert observed_instrument_id == instrument_id
    assert observed_payload == expected_payload
