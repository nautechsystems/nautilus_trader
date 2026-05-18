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
from unittest.mock import ANY
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderSide
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_condition_id
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
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.data import OrderBookDelta
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


def test_tick_size_change_noop_preserves_book_and_quote(event_loop) -> None:
    # Same tick_size on both sides must be ignored, not treated as an epoch.
    client, provider = _make_data_client(event_loop)

    instrument = _make_binary_option("0.01")
    client._cache.add_instrument(instrument)
    client._add_subscription_quote_ticks(instrument.id)

    snapshot = _build_snapshot(("0.50", "0.54", "0.56", "0.59"))
    client._handle_book_snapshot(instrument=instrument, ws_message=snapshot)
    assert instrument.id in client._local_books
    assert instrument.id in client._last_quotes
    book_before = client._local_books[instrument.id]
    quote_before = client._last_quotes[instrument.id]
    emitted_before = list(client.emitted)

    change = PolymarketTickSizeChange(
        market="0xMARKET",
        asset_id="0xASSET",
        new_tick_size="0.01",
        old_tick_size="0.01",
        timestamp="1700000001000",
    )
    client._handle_instrument_update(instrument=instrument, ws_message=change)

    # Local book and last quote must survive; nothing should be queued.
    assert client._local_books[instrument.id] is book_before
    assert client._last_quotes[instrument.id] is quote_before
    assert instrument.id not in client._pending_snapshot_after_tick_change

    # A no-op must not emit a phantom instrument update.
    provider.add.assert_not_called()
    assert client.emitted == emitted_before


def test_unsubscribe_clears_stale_local_book_when_no_sub_remains(event_loop) -> None:
    # Resubscribe would diff a fresh snapshot against the stale leaked book.
    client, _provider = _make_data_client(event_loop)

    instrument = _make_binary_option(
        "0.01",
        instrument_id=InstrumentId.from_str("0xCOND-0xTOKEN.POLYMARKET"),
    )
    client._cache.add_instrument(instrument)
    client._add_subscription_quote_ticks(instrument.id)

    snapshot = _build_snapshot(("0.50", "0.54", "0.56", "0.59"))
    client._handle_book_snapshot(instrument=instrument, ws_message=snapshot)
    assert instrument.id in client._local_books
    assert instrument.id in client._last_quotes

    client._remove_subscription_quote_ticks(instrument.id)
    command = UnsubscribeQuoteTicks(
        instrument_id=instrument.id,
        client_id=None,
        venue=instrument.id.venue,
        command_id=UUID4(),
        ts_init=0,
        params=None,
    )

    async def _run() -> None:
        await client._unsubscribe_quote_ticks(command)

    event_loop.run_until_complete(_run())

    assert instrument.id not in client._local_books
    assert instrument.id not in client._last_quotes


def test_unsubscribe_deltas_clears_stale_local_book_when_no_sub_remains(event_loop) -> None:
    # Sibling of the quotes-path test: the deltas teardown must clear too.
    client, _provider = _make_data_client(event_loop)

    instrument = _make_binary_option(
        "0.01",
        instrument_id=InstrumentId.from_str("0xCOND-0xTOKEN.POLYMARKET"),
    )
    client._cache.add_instrument(instrument)
    client._add_subscription_order_book_deltas(instrument.id)

    snapshot = _build_snapshot(("0.50", "0.54", "0.56", "0.59"))
    client._handle_book_snapshot(instrument=instrument, ws_message=snapshot)
    assert instrument.id in client._local_books

    client._remove_subscription_order_book_deltas(instrument.id)
    command = UnsubscribeOrderBook(
        instrument_id=instrument.id,
        book_data_type=OrderBookDelta,
        client_id=None,
        venue=instrument.id.venue,
        command_id=UUID4(),
        ts_init=0,
        params=None,
    )

    async def _run() -> None:
        await client._unsubscribe_order_book_deltas(command)

    event_loop.run_until_complete(_run())

    assert instrument.id not in client._local_books
    assert instrument.id not in client._last_quotes


def test_unsubscribe_preserves_local_book_when_other_sub_remains(event_loop) -> None:
    # The surviving channel still depends on the local book.
    client, _provider = _make_data_client(event_loop)

    instrument = _make_binary_option(
        "0.01",
        instrument_id=InstrumentId.from_str("0xCOND-0xTOKEN.POLYMARKET"),
    )
    client._cache.add_instrument(instrument)
    client._add_subscription_order_book_deltas(instrument.id)
    client._add_subscription_quote_ticks(instrument.id)

    snapshot = _build_snapshot(("0.50", "0.54", "0.56", "0.59"))
    client._handle_book_snapshot(instrument=instrument, ws_message=snapshot)
    book_before = client._local_books[instrument.id]
    quote_before = client._last_quotes[instrument.id]

    client._remove_subscription_quote_ticks(instrument.id)
    command = UnsubscribeQuoteTicks(
        instrument_id=instrument.id,
        client_id=None,
        venue=instrument.id.venue,
        command_id=UUID4(),
        ts_init=0,
        params=None,
    )

    async def _run() -> None:
        await client._unsubscribe_quote_ticks(command)

    event_loop.run_until_complete(_run())

    assert client._local_books[instrument.id] is book_before
    assert client._last_quotes[instrument.id] is quote_before


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
        provider.load_ids_async.assert_awaited_once_with(
            [instrument.id],
            transient_condition_ids=ANY,
        )
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
    provider.load_ids_async.assert_awaited_once_with(
        [instrument.id],
        transient_condition_ids=ANY,
    )


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
    failure = RuntimeError("gamma unavailable")
    provider.load_ids_async.side_effect = failure

    # Pre-register the future so the test can observe its terminal state.
    # `_ensure_instrument_loaded` reuses an existing future for the same id,
    # so this is equivalent to the production path while exposing the
    # exception contract to the assertion.
    future = event_loop.create_future()
    client._pending_instrument_loads[instrument.id] = future

    result = await client._ensure_instrument_loaded(instrument.id)

    assert result is False
    provider.load_ids_async.assert_awaited_once()
    assert client._cache.instrument(instrument.id) is None
    # The future must carry the original exception object so awaiters can
    # distinguish a load failure from a clean-but-missing instrument.
    # Identity (not just type+message) guards against wrap-and-rethrow.
    assert future.done()
    assert future.exception() is failure


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


def _make_client_for_retry(
    loop: asyncio.AbstractEventLoop,
    *,
    max_retries: int,
    instruments_by_id: dict[InstrumentId, BinaryOption],
    load_side_effect: Any,
) -> tuple[_RecordingPolymarketDataClient, MagicMock]:
    clock = LiveClock()
    msgbus = MessageBus(trader_id=TraderId("TEST-001"), clock=clock)
    cache = Cache()
    provider = MagicMock(spec=PolymarketInstrumentProvider)
    provider.find.side_effect = lambda inst_id: instruments_by_id.get(inst_id)
    provider.load_ids_async = AsyncMock(side_effect=load_side_effect)

    config = PolymarketDataClientConfig(
        auto_load_missing_instruments=True,
        auto_load_debounce_ms=5,
        auto_load_max_retries=max_retries,
        auto_load_retry_delay_initial_secs=0.01,
        auto_load_retry_delay_max_secs=0.01,
    )
    client = _RecordingPolymarketDataClient(
        loop=loop,
        http_client=MagicMock(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=provider,
        config=config,
        name="TEST-POLYMARKET",
    )
    client._ws_client = MagicMock()
    client._ws_client.is_connected = MagicMock(return_value=True)
    client._ws_client.subscribe = AsyncMock()
    client._ws_client.add_subscription = MagicMock()
    client._ws_client.disconnect = AsyncMock()
    return client, provider


_POLY_INSTRUMENT_ID = InstrumentId.from_str("0xCOND-0xTOKEN.POLYMARKET")


@pytest.mark.asyncio
async def test_ensure_instrument_loaded_retries_transient_empty_token(event_loop) -> None:
    # Reproduces the CLOB lifecycle race on newly-minted markets: the provider
    # initially reports empty token_ids for the condition, then returns
    # populated tokens on a later attempt.
    instrument = _make_binary_option("0.01", instrument_id=_POLY_INSTRUMENT_ID)
    instruments_by_id: dict[InstrumentId, BinaryOption] = {}
    call_count = 0

    async def fake_load(ids, *, transient_condition_ids=None, **_kw):
        nonlocal call_count
        call_count += 1
        if call_count == 1:
            for inst_id in ids:
                if transient_condition_ids is not None:
                    transient_condition_ids.add(get_polymarket_condition_id(inst_id))
        else:
            for inst_id in ids:
                instruments_by_id[inst_id] = instrument

    client, provider = _make_client_for_retry(
        event_loop,
        max_retries=3,
        instruments_by_id=instruments_by_id,
        load_side_effect=fake_load,
    )

    result = await client._ensure_instrument_loaded(instrument.id)

    assert result is True
    assert provider.load_ids_async.await_count == 2
    assert client._cache.instrument(instrument.id) is instrument
    assert instrument in client.emitted


@pytest.mark.asyncio
async def test_ensure_instrument_loaded_exhausts_retries_when_transient_persists(
    event_loop,
) -> None:
    instrument = _make_binary_option("0.01", instrument_id=_POLY_INSTRUMENT_ID)
    instruments_by_id: dict[InstrumentId, BinaryOption] = {}

    async def fake_load(ids, *, transient_condition_ids=None, **_kw):
        if transient_condition_ids is not None:
            for inst_id in ids:
                transient_condition_ids.add(get_polymarket_condition_id(inst_id))

    client, provider = _make_client_for_retry(
        event_loop,
        max_retries=2,
        instruments_by_id=instruments_by_id,
        load_side_effect=fake_load,
    )

    result = await client._ensure_instrument_loaded(instrument.id)

    assert result is False
    # Initial attempt + 2 retries = 3 total
    assert provider.load_ids_async.await_count == 3
    assert client._cache.instrument(instrument.id) is None


@pytest.mark.asyncio
async def test_ensure_instrument_loaded_terminal_miss_skips_retry(event_loop) -> None:
    # A genuine "not on venue" miss (no empty tokens reported) must not waste
    # retry budget polling for an instrument that will never appear.
    instrument = _make_binary_option("0.01", instrument_id=_POLY_INSTRUMENT_ID)
    instruments_by_id: dict[InstrumentId, BinaryOption] = {}

    async def fake_load(ids, *, transient_condition_ids=None, **_kw):
        pass  # Don't populate instruments_by_id, don't mark transient

    client, provider = _make_client_for_retry(
        event_loop,
        max_retries=5,
        instruments_by_id=instruments_by_id,
        load_side_effect=fake_load,
    )

    result = await client._ensure_instrument_loaded(instrument.id)

    assert result is False
    assert provider.load_ids_async.await_count == 1


def _make_client_for_retry_with_delay(
    loop: asyncio.AbstractEventLoop,
    *,
    max_retries: int,
    retry_delay_secs: float,
    instruments_by_id: dict[InstrumentId, BinaryOption],
    load_side_effect: Any,
) -> tuple[_RecordingPolymarketDataClient, MagicMock]:
    # Variant of `_make_client_for_retry` with a tunable retry delay; used by
    # tests that need a long-enough sleep window to interleave another action
    # (such as `_disconnect`) between attempts.
    clock = LiveClock()
    msgbus = MessageBus(trader_id=TraderId("TEST-001"), clock=clock)
    cache = Cache()
    provider = MagicMock(spec=PolymarketInstrumentProvider)
    provider.find.side_effect = lambda inst_id: instruments_by_id.get(inst_id)
    provider.load_ids_async = AsyncMock(side_effect=load_side_effect)

    config = PolymarketDataClientConfig(
        auto_load_missing_instruments=True,
        auto_load_debounce_ms=5,
        auto_load_max_retries=max_retries,
        auto_load_retry_delay_initial_secs=retry_delay_secs,
        auto_load_retry_delay_max_secs=retry_delay_secs,
    )
    client = _RecordingPolymarketDataClient(
        loop=loop,
        http_client=MagicMock(),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=provider,
        config=config,
        name="TEST-POLYMARKET",
    )
    client._ws_client = MagicMock()
    client._ws_client.is_connected = MagicMock(return_value=True)
    client._ws_client.subscribe = AsyncMock()
    client._ws_client.add_subscription = MagicMock()
    client._ws_client.disconnect = AsyncMock()
    return client, provider


@pytest.mark.asyncio
async def test_disconnect_cancels_in_flight_retry_sleep(event_loop) -> None:
    # Disconnect during the retry sleep (between attempts) must cancel the
    # flush task and the pending future, so callers do not hang.
    instrument = _make_binary_option("0.01", instrument_id=_POLY_INSTRUMENT_ID)
    instruments_by_id: dict[InstrumentId, BinaryOption] = {}

    async def fake_load(ids, *, transient_condition_ids=None, **_kw):
        # Always transient: the loop would otherwise terminate on its own.
        if transient_condition_ids is not None:
            for inst_id in ids:
                transient_condition_ids.add(get_polymarket_condition_id(inst_id))

    client, provider = _make_client_for_retry_with_delay(
        event_loop,
        max_retries=10,
        retry_delay_secs=5.0,  # long enough to interleave the disconnect
        instruments_by_id=instruments_by_id,
        load_side_effect=fake_load,
    )

    # Pre-register the future so the test can observe its terminal state.
    # `_flush_pending_loads` drains `_pending_instrument_loads` before it
    # sleeps, so this is the only handle to the future after dispatch.
    future = event_loop.create_future()
    client._pending_instrument_loads[instrument.id] = future

    ensure_task = event_loop.create_task(client._ensure_instrument_loaded(instrument.id))

    # Wait until the first attempt completes and the flush task is in the
    # retry sleep window. Polling the await_count avoids timing flakiness.
    async def _first_attempt_done() -> None:
        while provider.load_ids_async.await_count < 1:
            await asyncio.sleep(0.01)

    await asyncio.wait_for(_first_attempt_done(), timeout=1.0)

    await client._disconnect()

    assert await ensure_task is False
    # No further attempts after the first.
    assert provider.load_ids_async.await_count == 1
    assert client._pending_instrument_loads == {}
    # The future must be cancelled, not silently resolved with `None`, so
    # awaiters see "shutdown" rather than "loaded clean".
    assert future.cancelled()


@pytest.mark.asyncio
async def test_flush_pending_loads_mixed_batch_outcome(event_loop) -> None:
    # One batch with three instruments exercising every branch of the
    # per-instrument outcome decision in `_flush_pending_loads`: A loads on
    # the first pass, C is terminal-miss on the first pass, B is transient
    # and only loads on the second pass.
    inst_a = _make_binary_option(
        "0.01",
        instrument_id=InstrumentId.from_str("0xCONDA-0xTOKENA.POLYMARKET"),
    )
    inst_b = _make_binary_option(
        "0.01",
        instrument_id=InstrumentId.from_str("0xCONDB-0xTOKENB.POLYMARKET"),
    )
    inst_c = _make_binary_option(
        "0.01",
        instrument_id=InstrumentId.from_str("0xCONDC-0xTOKENC.POLYMARKET"),
    )
    instruments_by_id: dict[InstrumentId, BinaryOption] = {}
    call_count = 0

    async def fake_load(ids, *, transient_condition_ids=None, **_kw):
        nonlocal call_count
        call_count += 1
        if call_count == 1:
            # First pass: A loads, B is transient, C is terminal-miss.
            instruments_by_id[inst_a.id] = inst_a
            if transient_condition_ids is not None:
                transient_condition_ids.add(get_polymarket_condition_id(inst_b.id))
        else:
            # Second pass: B loads. C was already terminal-resolved.
            instruments_by_id[inst_b.id] = inst_b

    client, provider = _make_client_for_retry(
        event_loop,
        max_retries=3,
        instruments_by_id=instruments_by_id,
        load_side_effect=fake_load,
    )

    results = await asyncio.gather(
        client._ensure_instrument_loaded(inst_a.id),
        client._ensure_instrument_loaded(inst_b.id),
        client._ensure_instrument_loaded(inst_c.id),
    )

    assert results == [True, True, False]
    # Two total attempts: the second only re-runs the transient set {B}.
    assert provider.load_ids_async.await_count == 2
    (second_ids,), _ = provider.load_ids_async.await_args_list[1]
    assert list(second_ids) == [inst_b.id]
    # Emitted instruments must mirror what landed in the cache.
    assert inst_a in client.emitted
    assert inst_b in client.emitted
    assert inst_c not in client.emitted


@pytest.mark.asyncio
async def test_ensure_instrument_loaded_max_retries_zero_disables_retry(event_loop) -> None:
    # The config docs state max_retries=0 disables retry. Verify a transient
    # is observed exactly once and the future is then terminally resolved.
    instrument = _make_binary_option("0.01", instrument_id=_POLY_INSTRUMENT_ID)
    instruments_by_id: dict[InstrumentId, BinaryOption] = {}

    async def fake_load(ids, *, transient_condition_ids=None, **_kw):
        if transient_condition_ids is not None:
            for inst_id in ids:
                transient_condition_ids.add(get_polymarket_condition_id(inst_id))

    client, provider = _make_client_for_retry(
        event_loop,
        max_retries=0,
        instruments_by_id=instruments_by_id,
        load_side_effect=fake_load,
    )

    result = await client._ensure_instrument_loaded(instrument.id)

    assert result is False
    assert provider.load_ids_async.await_count == 1
    assert client._cache.instrument(instrument.id) is None
