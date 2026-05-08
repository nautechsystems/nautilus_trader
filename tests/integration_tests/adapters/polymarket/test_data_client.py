# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from decimal import Decimal
from typing import Any
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderSide
from nautilus_trader.adapters.polymarket.config import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket.data import PolymarketDataClient
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookLevel
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookSnapshot
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuote
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuotes
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTickSizeChange
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class _RecordingPolymarketDataClient(PolymarketDataClient):
    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self.emitted: list[Any] = []

    def _handle_data(self, data: Any) -> None:
        self.emitted.append(data)
        # Mirror the data engine: instruments flow into the cache so that
        # downstream `self._cache.instrument(id)` lookups succeed.
        if isinstance(data, BinaryOption):
            self._cache.add_instrument(data)


def _make_binary_option(
    price_inc: str,
    instrument_id: InstrumentId | None = None,
) -> BinaryOption:
    if instrument_id is None:
        instrument_id = InstrumentId.from_str("0xABCDEF.POLYMARKET")
    price_increment = Price.from_str(price_inc)
    size_increment = Quantity.from_str("0.01")
    return BinaryOption(
        instrument_id=instrument_id,
        raw_symbol=instrument_id.symbol,
        outcome="YES",
        description="Test Polymarket Instrument",
        asset_class=AssetClass.ALTERNATIVE,
        currency=USDC,
        price_precision=price_increment.precision,
        price_increment=price_increment,
        size_precision=size_increment.precision,
        size_increment=size_increment,
        activation_ns=0,
        expiration_ns=0,
        max_quantity=None,
        min_quantity=Quantity.from_int(1),
        maker_fee=Decimal(0),
        taker_fee=Decimal(0),
        ts_event=0,
        ts_init=0,
    )


def _build_snapshot(prices: tuple[str, str, str, str]) -> PolymarketBookSnapshot:
    bid_low, bid_high, ask_low, ask_high = prices
    return PolymarketBookSnapshot(
        market="0xMARKET",
        asset_id="0xASSET",
        bids=[
            PolymarketBookLevel(price=bid_low, size="15"),
            PolymarketBookLevel(price=bid_high, size="10"),
        ],
        asks=[
            PolymarketBookLevel(price=ask_high, size="12"),
            PolymarketBookLevel(price=ask_low, size="8"),
        ],
        timestamp="1700000000000",
    )


def _make_data_client(
    event_loop,
    *,
    config: PolymarketDataClientConfig | None = None,
) -> tuple[_RecordingPolymarketDataClient, MagicMock]:
    clock = LiveClock()
    msgbus = MessageBus(trader_id=TraderId("TEST-001"), clock=clock)
    cache = Cache()
    provider = MagicMock(spec=PolymarketInstrumentProvider)
    http_client = MagicMock()

    client = _RecordingPolymarketDataClient(
        loop=event_loop,
        http_client=http_client,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=provider,
        config=config or PolymarketDataClientConfig(),
        name="TEST-POLYMARKET",
    )

    client._ws_client = MagicMock()
    client._ws_client.is_connected = MagicMock(return_value=True)
    client._ws_client.subscribe = AsyncMock()
    client._ws_client.unsubscribe = AsyncMock()
    client._ws_client.disconnect = AsyncMock()

    return client, provider


def test_handle_raw_ws_message_with_non_utf8_payload_does_not_raise(event_loop) -> None:
    client, _ = _make_data_client(event_loop)
    client._decoder_market_msg = MagicMock()
    client._decoder_market_msg.decode.side_effect = ValueError("invalid payload")

    # Non-UTF8 bytes should still be safely logged on parser failure.
    client._handle_raw_ws_message(b"\xff")


def test_tick_size_change_clears_book_and_marks_pending(event_loop) -> None:
    # Coarsens 0.001 -> 0.01; old levels like 0.505 are invalid on 0.01.
    client, provider = _make_data_client(event_loop)

    instrument_old = _make_binary_option("0.001")
    client._cache.add_instrument(instrument_old)
    client._add_subscription_quote_ticks(instrument_old.id)

    snapshot_old = _build_snapshot(("0.501", "0.504", "0.506", "0.509"))
    deltas_old = snapshot_old.parse_to_snapshot(instrument=instrument_old, ts_init=0)
    book_old = OrderBook(instrument_old.id, book_type=BookType.L2_MBP)
    book_old.apply_deltas(deltas_old)
    client._local_books[instrument_old.id] = book_old

    quote_old = snapshot_old.parse_to_quote(
        instrument=instrument_old,
        ts_init=0,
        drop_quotes_missing_side=False,
    )
    assert quote_old is not None
    client._last_quotes[instrument_old.id] = quote_old

    change = PolymarketTickSizeChange(
        market="0xMARKET",
        asset_id="0xASSET",
        new_tick_size="0.01",
        old_tick_size="0.001",
        timestamp="1700000001000",
    )

    client._handle_instrument_update(instrument=instrument_old, ws_message=change)

    instrument_id = instrument_old.id
    provider.add.assert_called_once()

    cached_instrument = client._cache.instrument(instrument_id)
    assert cached_instrument is not None
    assert cached_instrument.price_precision == 2

    assert instrument_id not in client._local_books
    assert instrument_id not in client._last_quotes
    assert instrument_id in client._pending_snapshot_after_tick_change

    # Only the BinaryOption update is emitted; no deltas, no quote.
    assert sum(1 for item in client.emitted if isinstance(item, BinaryOption)) == 1
    assert not any(isinstance(item, OrderBookDeltas) for item in client.emitted)
    assert not any(isinstance(item, QuoteTick) for item in client.emitted)


def test_pending_drops_price_change_until_snapshot(event_loop) -> None:
    # Mixed sequence: instrument update -> stale delta dropped -> snapshot reseeds.
    client, _provider = _make_data_client(event_loop)

    instrument_new = _make_binary_option("0.01")
    client._cache.add_instrument(instrument_new)
    client._add_subscription_quote_ticks(instrument_new.id)
    client._pending_snapshot_after_tick_change.add(instrument_new.id)

    delta_msg = PolymarketQuotes(
        market="0xMARKET",
        price_changes=[
            PolymarketQuote(
                asset_id="0xASSET",
                price="0.50",
                side=PolymarketOrderSide.BUY,
                size="20",
                hash="",
            ),
        ],
        timestamp="1700000002000",
    )

    client._handle_quote(
        instrument=instrument_new,
        ws_message=delta_msg,
        price_change=delta_msg.price_changes[0],
    )

    # Drop is silent on the data engine: nothing emitted, no local book created.
    assert not any(isinstance(item, OrderBookDeltas) for item in client.emitted)
    assert not any(isinstance(item, QuoteTick) for item in client.emitted)
    assert instrument_new.id not in client._local_books

    snapshot_new = _build_snapshot(("0.45", "0.49", "0.51", "0.55"))
    client._handle_book_snapshot(instrument=instrument_new, ws_message=snapshot_new)

    assert instrument_new.id not in client._pending_snapshot_after_tick_change
    rebuilt_book = client._local_books[instrument_new.id]
    bid_price = rebuilt_book.best_bid_price()
    ask_price = rebuilt_book.best_ask_price()
    assert bid_price is not None
    assert ask_price is not None
    assert bid_price.precision == ask_price.precision == 2

    assert any(isinstance(item, OrderBookDeltas) for item in client.emitted)
    quote_emitted = next(
        (item for item in client.emitted if isinstance(item, QuoteTick)),
        None,
    )
    assert quote_emitted is not None
    assert quote_emitted.bid_price.precision == 2
    assert quote_emitted.ask_price.precision == 2


def test_tick_size_change_finer_then_snapshot_clean_transition(event_loop) -> None:
    # 0.01 -> 0.001: snapshot (prec=2), tick_size_change, snapshot (prec=3).
    client, _provider = _make_data_client(event_loop)

    instrument_old = _make_binary_option("0.01")
    client._cache.add_instrument(instrument_old)
    client._add_subscription_quote_ticks(instrument_old.id)

    snapshot_old = _build_snapshot(("0.50", "0.54", "0.56", "0.59"))
    client._handle_book_snapshot(instrument=instrument_old, ws_message=snapshot_old)
    assert client._local_books[instrument_old.id].best_bid_price().precision == 2

    change = PolymarketTickSizeChange(
        market="0xMARKET",
        asset_id="0xASSET",
        new_tick_size="0.001",
        old_tick_size="0.01",
        timestamp="1700000001000",
    )
    client._handle_instrument_update(instrument=instrument_old, ws_message=change)

    instrument_new = client._cache.instrument(instrument_old.id)
    assert instrument_new is not None
    assert instrument_new.price_precision == 3

    snapshot_new = _build_snapshot(("0.501", "0.541", "0.561", "0.591"))
    client._handle_book_snapshot(instrument=instrument_new, ws_message=snapshot_new)

    assert instrument_new.id not in client._pending_snapshot_after_tick_change
    rebuilt_book = client._local_books[instrument_new.id]
    bid_price = rebuilt_book.best_bid_price()
    assert bid_price is not None
    assert bid_price.precision == 3


def test_tick_size_change_skips_pending_for_trade_only_sub(event_loop) -> None:
    # Trade-only subs don't read the book; pending would be dead state.
    client, _provider = _make_data_client(event_loop)

    instrument_old = _make_binary_option("0.01")
    client._cache.add_instrument(instrument_old)
    client._add_subscription_trade_ticks(instrument_old.id)

    change = PolymarketTickSizeChange(
        market="0xMARKET",
        asset_id="0xASSET",
        new_tick_size="0.001",
        old_tick_size="0.01",
        timestamp="1700000001000",
    )
    client._handle_instrument_update(instrument=instrument_old, ws_message=change)

    assert instrument_old.id not in client._pending_snapshot_after_tick_change


@pytest.mark.asyncio
async def test_unsubscribe_clears_pending_when_no_book_or_quote_remains(
    event_loop,
) -> None:
    # Removing the last book/quote sub clears pending so a later resubscribe
    # via a still-open trade stream is not blocked.
    client, _provider = _make_data_client(event_loop)

    instrument = _make_binary_option(
        "0.01",
        instrument_id=InstrumentId.from_str("0xCOND-0xTOKEN.POLYMARKET"),
    )
    client._cache.add_instrument(instrument)
    client._add_subscription_quote_ticks(instrument.id)
    client._pending_snapshot_after_tick_change.add(instrument.id)

    client._remove_subscription_quote_ticks(instrument.id)
    command = UnsubscribeQuoteTicks(
        instrument_id=instrument.id,
        client_id=None,
        venue=instrument.id.venue,
        command_id=UUID4(),
        ts_init=0,
        params=None,
    )
    await client._unsubscribe_quote_ticks(command)

    assert instrument.id not in client._pending_snapshot_after_tick_change


def _make_client_for_auto_load(
    loop: asyncio.AbstractEventLoop,
    *,
    auto_load: bool = True,
    debounce_ms: int = 20,
    instruments: list[BinaryOption] | None = None,
) -> tuple[_RecordingPolymarketDataClient, MagicMock]:
    clock = LiveClock()
    msgbus = MessageBus(trader_id=TraderId("TEST-001"), clock=clock)
    cache = Cache()
    provider = MagicMock(spec=PolymarketInstrumentProvider)
    provider.load_ids_async = AsyncMock()
    http_client = MagicMock()

    instruments_by_id = {inst.id: inst for inst in (instruments or [])}
    provider.find.side_effect = lambda inst_id: instruments_by_id.get(inst_id)

    config = PolymarketDataClientConfig(
        auto_load_missing_instruments=auto_load,
        auto_load_debounce_ms=debounce_ms,
    )
    client = _RecordingPolymarketDataClient(
        loop=loop,
        http_client=http_client,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=provider,
        config=config,
        name="TEST-POLYMARKET",
    )

    # Swap the real WS client for a mock so tests can assert on subscribe calls
    # without spinning up a socket.
    client._ws_client = MagicMock()
    client._ws_client.is_connected = MagicMock(return_value=True)
    client._ws_client.subscribe = AsyncMock()
    client._ws_client.add_subscription = MagicMock()
    client._ws_client.disconnect = AsyncMock()

    return client, provider


@pytest.mark.parametrize(
    ("cache_hit", "auto_load", "disconnecting", "expected", "expect_load"),
    [
        (True, True, False, True, False),
        (False, True, False, True, True),
        (False, False, False, False, False),
        (False, True, True, False, False),
    ],
    ids=[
        "cache_hit",
        "cache_miss_auto_load_on",
        "cache_miss_auto_load_off",
        "disconnecting_gate",
    ],
)
@pytest.mark.asyncio
async def test_ensure_instrument_loaded_state_table(
    event_loop,
    cache_hit: bool,
    auto_load: bool,
    disconnecting: bool,
    expected: bool,
    expect_load: bool,
) -> None:
    instrument = _make_binary_option("0.01")
    client, provider = _make_client_for_auto_load(
        event_loop,
        auto_load=auto_load,
        instruments=[instrument],
    )

    if cache_hit:
        client._cache.add_instrument(instrument)

    if disconnecting:
        client._disconnecting = True

    result = await client._ensure_instrument_loaded(instrument.id)
    assert result is expected

    if expect_load:
        provider.load_ids_async.assert_awaited_once_with([instrument.id])
        # Strengthened from earlier: verify the production _handle_data path
        # emitted the instrument (not just that load_ids_async was called).
        assert instrument in client.emitted
    else:
        provider.load_ids_async.assert_not_awaited()


@pytest.mark.asyncio
async def test_ensure_instrument_loaded_coalesces_same_id(event_loop) -> None:
    instrument = _make_binary_option("0.01")
    client, provider = _make_client_for_auto_load(event_loop, instruments=[instrument])

    results = await asyncio.gather(
        client._ensure_instrument_loaded(instrument.id),
        client._ensure_instrument_loaded(instrument.id),
        client._ensure_instrument_loaded(instrument.id),
    )

    assert all(results)
    provider.load_ids_async.assert_awaited_once_with([instrument.id])


@pytest.mark.asyncio
async def test_ensure_instrument_loaded_coalesces_distinct_ids(event_loop) -> None:
    inst_a = _make_binary_option("0.01", instrument_id=InstrumentId.from_str("0xAAA.POLYMARKET"))
    inst_b = _make_binary_option("0.01", instrument_id=InstrumentId.from_str("0xBBB.POLYMARKET"))
    client, provider = _make_client_for_auto_load(event_loop, instruments=[inst_a, inst_b])

    results = await asyncio.gather(
        client._ensure_instrument_loaded(inst_a.id),
        client._ensure_instrument_loaded(inst_b.id),
    )

    assert all(results)
    # Both ids must land in the same batched load call.
    provider.load_ids_async.assert_awaited_once()
    (called_ids,), _ = provider.load_ids_async.await_args
    assert set(called_ids) == {inst_a.id, inst_b.id}


@pytest.mark.asyncio
async def test_flush_pending_loads_exception_propagates_to_callers(event_loop) -> None:
    instrument = _make_binary_option("0.01")
    client, provider = _make_client_for_auto_load(event_loop, instruments=[instrument])
    provider.load_ids_async.side_effect = RuntimeError("gamma unavailable")

    result = await client._ensure_instrument_loaded(instrument.id)

    assert result is False
    provider.load_ids_async.assert_awaited_once()
    assert client._cache.instrument(instrument.id) is None


@pytest.mark.asyncio
async def test_disconnect_cancels_in_flight_flush_and_futures(event_loop) -> None:
    instrument = _make_binary_option("0.01")
    client, provider = _make_client_for_auto_load(
        event_loop,
        debounce_ms=5000,  # long enough that disconnect interrupts the sleep
        instruments=[instrument],
    )

    ensure_task = event_loop.create_task(client._ensure_instrument_loaded(instrument.id))

    # Give the ensure call a chance to register its future and spawn the flush.
    await asyncio.sleep(0.01)
    assert instrument.id in client._pending_instrument_loads
    assert client._auto_load_task is not None

    await client._disconnect()

    assert await ensure_task is False
    assert client._pending_instrument_loads == {}
    assert client._auto_load_tasks == set()
    # Nothing should have emitted because the flush was cancelled before load_ids_async ran.
    provider.load_ids_async.assert_not_awaited()


@pytest.mark.asyncio
async def test_subscribe_quote_ticks_unsubscribed_during_autoload_skips_ws(event_loop) -> None:
    instrument = _make_binary_option("0.01")
    client, _provider = _make_client_for_auto_load(event_loop, instruments=[instrument])

    command = SubscribeQuoteTicks(
        instrument_id=instrument.id,
        client_id=None,
        venue=instrument.id.venue,
        command_id=UUID4(),
        ts_init=0,
        params=None,
    )
    # Simulate the base class adding the logical subscription at dispatch time.
    client._add_subscription_quote_ticks(instrument.id)

    original_ensure = client._ensure_instrument_loaded

    async def _ensure_then_unsubscribe(inst_id: InstrumentId) -> bool:
        result = await original_ensure(inst_id)
        # Caller unsubscribed while we were awaiting the Gamma batch.
        client._remove_subscription_quote_ticks(inst_id)
        return result

    client._ensure_instrument_loaded = _ensure_then_unsubscribe  # type: ignore[assignment]

    await client._subscribe_quote_ticks(command)

    client._ws_client.subscribe.assert_not_awaited()  # type: ignore[attr-defined]
    client._ws_client.add_subscription.assert_not_called()  # type: ignore[attr-defined]


@pytest.mark.asyncio
async def test_connect_resets_disconnecting_flag(event_loop) -> None:
    instrument = _make_binary_option("0.01")
    client, provider = _make_client_for_auto_load(event_loop, instruments=[instrument])
    provider.initialize = AsyncMock()
    provider.get_all = MagicMock(return_value={})
    provider.currencies = MagicMock(return_value={})
    client._disconnecting = True

    await client._connect()

    assert client._disconnecting is False
