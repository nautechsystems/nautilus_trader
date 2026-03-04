"""Maintain MakerV3 market data state and trigger quote cycles."""

from __future__ import annotations

from decimal import Decimal
from typing import Any

from nautilus_trader.flux.strategies.makerv3 import pricing as pricing_mod
from nautilus_trader.flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_SKIPPED
from nautilus_trader.flux.strategies.makerv3.constants import REASON_SKIPPED_BOT_OFF
from nautilus_trader.flux.strategies.makerv3.constants import REASON_SKIPPED_QUOTE_FAIL_CIRCUIT_OPEN
from nautilus_trader.flux.strategies.makerv3.constants import REASON_SKIPPED_REQUOTE_THROTTLED
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_FV


_price_to_decimal = pricing_mod.price_to_decimal


def should_publish_market_bbo(
    *,
    bbo_changed: bool,
    last_publish_ns: int,
    now_ns: int,
    heartbeat_ms: int,
) -> bool:
    """Return True when a BBO snapshot should be published (change or heartbeat)."""
    if bbo_changed:
        return True
    if last_publish_ns <= 0:
        return True
    interval_ns = max(1, int(heartbeat_ms)) * 1_000_000
    return now_ns - last_publish_ns >= interval_ns


def on_order_book_deltas(strategy: Any, deltas: Any) -> None:
    """Process market deltas and trigger quote-cycle refresh when eligible."""
    book = strategy._books.get(deltas.instrument_id)
    if book is None:
        return

    book.apply_deltas(deltas)
    bid = book.best_bid_price()
    ask = book.best_ask_price()
    if bid is None or ask is None:
        return

    now_ns = int(strategy.clock.timestamp_ns())
    bid_dec = _price_to_decimal(bid)
    ask_dec = _price_to_decimal(ask)
    last = strategy._last_bbo.get(deltas.instrument_id)
    bbo_changed = last != (bid_dec, ask_dec)
    if bbo_changed:
        strategy._last_bbo[deltas.instrument_id] = (bid_dec, ask_dec)
    strategy._last_bbo_ts_ns[deltas.instrument_id] = now_ns

    should_publish_bbo = should_publish_market_bbo(
        bbo_changed=bbo_changed,
        last_publish_ns=strategy._last_market_bbo_publish_ns.get(deltas.instrument_id, 0),
        now_ns=now_ns,
        heartbeat_ms=strategy.MARKET_BBO_HEARTBEAT_MS,
    )
    if should_publish_bbo:
        strategy._last_market_bbo_publish_ns[deltas.instrument_id] = now_ns
        strategy._publish_market_bbo(
            instrument_id=deltas.instrument_id,
            bid=bid_dec,
            ask=ask_dec,
            ts_ns=now_ns,
        )
        if bbo_changed or now_ns - strategy._last_fv_snapshot_ts_ns >= strategy.MARKET_BBO_HEARTBEAT_MS * 1_000_000:
            strategy._recompute_and_publish_fv()
        strategy._publish_state_if_due()

    strategy._publish_balances_if_due()

    bot_on_now = strategy._effective_bot_on()
    if strategy.config.maker_instrument_id != deltas.instrument_id:
        return

    if not bot_on_now:
        strategy._cancel_managed_quotes("bot_off")
        strategy._publish_state("bot_off")
        quote_cycle_id = strategy._next_quote_cycle_id(now_ns=now_ns)
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_SKIPPED,
            reason_code=REASON_SKIPPED_BOT_OFF,
            quote_cycle_id=quote_cycle_id,
        )
        return

    now_ns = int(strategy.clock.timestamp_ns())
    quote_cycle_id = strategy._next_quote_cycle_id(now_ns=now_ns)
    if now_ns - strategy._last_requote_ns < strategy.INTERNAL_REQUOTE_THROTTLE_MS * 1_000_000:
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_SKIPPED,
            reason_code=REASON_SKIPPED_REQUOTE_THROTTLED,
            quote_cycle_id=quote_cycle_id,
            payload={
                "throttle_ms": strategy.INTERNAL_REQUOTE_THROTTLE_MS,
            },
        )
        return
    if strategy._quote_failure_circuit_open:
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_SKIPPED,
            reason_code=REASON_SKIPPED_QUOTE_FAIL_CIRCUIT_OPEN,
            quote_cycle_id=quote_cycle_id,
        )
        return
    try:
        strategy._refresh_quotes(now_ns=now_ns, quote_cycle_id=quote_cycle_id)
        strategy._quote_failures_ns.clear()
    except Exception as exc:
        strategy._handle_quote_failure(now_ns=now_ns, exc=exc, context="on_order_book_deltas")


def best_bid_ask(strategy: Any, instrument_id: Any) -> tuple[Decimal, Decimal] | None:
    """Return the best bid/ask decimal prices for an instrument."""
    book = strategy._books.get(instrument_id)
    if book is None:
        return None
    bid = book.best_bid_price()
    ask = book.best_ask_price()
    if bid is None or ask is None:
        return None
    return bid.as_decimal(), ask.as_decimal()


def best_mid(strategy: Any, instrument_id: Any) -> Decimal | None:
    """Return the mid price derived from the current best bid/ask."""
    bbo = strategy._best_bid_ask(instrument_id)
    if bbo is None:
        return None
    bid, ask = bbo
    return (bid + ask) / Decimal("2")


def book_spread(strategy: Any, instrument_id: Any) -> Decimal | None:
    """Return the current top-of-book spread, if available."""
    bbo = strategy._best_bid_ask(instrument_id)
    if bbo is None:
        return None
    bid, ask = bbo
    return ask - bid


def recompute_and_publish_fv(strategy: Any) -> None:
    """Compute fair-value midpoint and publish it."""
    maker_mid = strategy._best_mid(strategy.config.maker_instrument_id)
    reference_mid = strategy._best_mid(strategy.config.reference_instrument_id)
    if maker_mid is None and reference_mid is None:
        return

    if maker_mid is not None and reference_mid is not None:
        strategy._last_fv = (maker_mid + reference_mid) / Decimal("2")
    else:
        strategy._last_fv = maker_mid or reference_mid

    now_ns = int(strategy.clock.timestamp_ns())
    payload = {
        "strategy_id": strategy._external_strategy_id,
        "fv": str(strategy._last_fv),
        "maker_mid": str(maker_mid) if maker_mid is not None else None,
        "reference_mid": str(reference_mid) if reference_mid is not None else None,
        "ts_event": now_ns,
        "ts_ms": now_ns // 1_000_000,
    }
    strategy._publish_json(TOPIC_FV, [payload])
    strategy._last_fv_snapshot_ts_ns = now_ns
