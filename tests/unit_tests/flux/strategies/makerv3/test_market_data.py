from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace


def test_on_order_book_deltas_stale_cancel_is_cooled_down_beyond_requote_throttle(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory(
        [
            20_000_000_000,
            20_000_000_000,
            20_250_000_000,
            20_250_000_000,
        ],
    )
    strategy.INTERNAL_REQUOTE_THROTTLE_MS = 100
    strategy.STALE_CANCEL_COOLDOWN_MS = 1_000
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._publish_market_bbo = lambda *_args, **_kwargs: None
    strategy._recompute_and_publish_fv = lambda: None
    strategy._publish_state_if_due = lambda: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state = lambda *_args, **_kwargs: None

    class _Book:
        def apply_deltas(self, _deltas: object) -> None:
            return

        def best_bid_price(self) -> Decimal:
            return Decimal(100)

        def best_ask_price(self) -> Decimal:
            return Decimal(101)

    strategy._books = {strategy.config.maker_instrument_id: _Book()}
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 0

    cancels: list[str] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )

    delta = SimpleNamespace(instrument_id=strategy.config.maker_instrument_id)
    strategy.on_order_book_deltas(delta)
    strategy.on_order_book_deltas(delta)

    assert cancels == ["reference_md_stale:False"]
    assert strategy._last_requote_ns == 20_250_000_000


def test_on_order_book_deltas_uses_decimal_bbo_without_string_conversions(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])

    class _Price:
        def __init__(self, value: str) -> None:
            self._value = Decimal(value)

        def as_decimal(self) -> Decimal:
            return self._value

        def __str__(self) -> str:
            raise AssertionError("hot-path BBO should avoid string conversion")

    class _Book:
        def apply_deltas(self, _deltas: object) -> None:
            return

        def best_bid_price(self) -> _Price:
            return _Price("100")

        def best_ask_price(self) -> _Price:
            return _Price("101")

    strategy._books = {strategy.config.reference_instrument_id: _Book()}
    strategy._last_bbo = {strategy.config.reference_instrument_id: None}
    strategy._last_market_bbo_publish_ns = {strategy.config.reference_instrument_id: 0}
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_if_due = lambda: None
    strategy._recompute_and_publish_fv = lambda: None

    published: list[tuple[Decimal, Decimal]] = []
    strategy._publish_market_bbo = lambda *, instrument_id, bid, ask, ts_ns: published.append(
        (bid, ask),
    )

    strategy.on_order_book_deltas(
        SimpleNamespace(instrument_id=strategy.config.reference_instrument_id),
    )

    assert strategy._last_bbo[strategy.config.reference_instrument_id] == (
        Decimal(100),
        Decimal(101),
    )
    assert published == [(Decimal(100), Decimal(101))]


def test_on_order_book_deltas_reference_leg_refreshes_quotes_when_not_throttled(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000, 1_000_000_000])
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_if_due = lambda: None
    strategy._publish_market_bbo = lambda *_args, **_kwargs: None
    strategy._recompute_and_publish_fv = lambda: None
    strategy._effective_bot_on = lambda: True

    class _Book:
        def apply_deltas(self, _deltas: object) -> None:
            return

        def best_bid_price(self) -> Decimal:
            return Decimal(100)

        def best_ask_price(self) -> Decimal:
            return Decimal(101)

    reference_id = strategy.config.reference_instrument_id
    strategy._books = {reference_id: _Book()}
    strategy._last_bbo = {reference_id: None}
    strategy._last_market_bbo_publish_ns = {reference_id: 0}

    refresh_calls: list[tuple[int, str | None]] = []
    strategy._refresh_quotes = lambda now_ns, *, quote_cycle_id=None: refresh_calls.append(
        (now_ns, quote_cycle_id),
    )

    strategy.on_order_book_deltas(SimpleNamespace(instrument_id=reference_id))

    assert len(refresh_calls) == 1
    assert refresh_calls[0][0] == 1_000_000_000
    assert refresh_calls[0][1]


def test_on_order_book_deltas_does_not_cancel_bot_off_quotes_during_market_exit(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_if_due = lambda: None
    strategy._publish_market_bbo = lambda *_args, **_kwargs: None
    strategy._recompute_and_publish_fv = lambda: None
    strategy._effective_bot_on = lambda: False
    strategy.is_exiting = lambda: True

    class _Book:
        def apply_deltas(self, _deltas: object) -> None:
            return

        def best_bid_price(self) -> Decimal:
            return Decimal(100)

        def best_ask_price(self) -> Decimal:
            return Decimal(101)

    maker_id = strategy.config.maker_instrument_id
    strategy._books = {maker_id: _Book()}
    strategy._last_bbo = {maker_id: None}
    strategy._last_market_bbo_publish_ns = {maker_id: 0}

    canceled: list[str] = []
    strategy._cancel_managed_quotes = lambda reason, **_kwargs: canceled.append(reason)
    strategy._publish_state = lambda *_args, **_kwargs: None
    strategy._publish_quote_cycle_event = lambda **_kwargs: None

    strategy.on_order_book_deltas(SimpleNamespace(instrument_id=maker_id))

    assert canceled == []


def test_on_order_book_deltas_does_not_recancel_bot_off_quotes_during_startup_cleanup(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_if_due = lambda: None
    strategy._publish_market_bbo = lambda *_args, **_kwargs: None
    strategy._recompute_and_publish_fv = lambda: None
    strategy._effective_bot_on = lambda: False
    strategy._startup_cleanup_pending = True

    class _Book:
        def apply_deltas(self, _deltas: object) -> None:
            return

        def best_bid_price(self) -> Decimal:
            return Decimal(100)

        def best_ask_price(self) -> Decimal:
            return Decimal(101)

    maker_id = strategy.config.maker_instrument_id
    strategy._books = {maker_id: _Book()}
    strategy._last_bbo = {maker_id: None}
    strategy._last_market_bbo_publish_ns = {maker_id: 0}

    canceled: list[str] = []
    states: list[str] = []
    strategy._cancel_managed_quotes = lambda reason, **_kwargs: canceled.append(reason)
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_quote_cycle_event = lambda **_kwargs: None

    strategy.on_order_book_deltas(SimpleNamespace(instrument_id=maker_id))

    assert canceled == []
    assert states == ["blocked_startup_cleanup"]


def test_publish_market_bbo_formats_prices_with_instrument_precision(strategy_factory) -> None:
    strategy = strategy_factory()
    instrument_id = strategy.config.maker_instrument_id
    strategy._instruments = {
        instrument_id: SimpleNamespace(
            raw_symbol="BTCUSDT",
            base_currency="BTC",
            quote_currency="USDT",
            price_precision=2,
        ),
    }

    payloads: list[dict[str, object]] = []
    strategy._publish_json = lambda _topic, payload: payloads.append(payload)

    strategy._publish_market_bbo(
        instrument_id=instrument_id,
        bid=Decimal(100),
        ask=Decimal("100.1"),
        ts_ns=1_000_000_000,
    )

    assert payloads[-1]["bid"] == "100.00"
    assert payloads[-1]["ask"] == "100.10"
