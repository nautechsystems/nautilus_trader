"""
Maintain MakerV3 market data state and trigger quote cycles.
"""

from __future__ import annotations

from decimal import Decimal
from typing import TYPE_CHECKING

from flux.strategies.makerv3 import pricing as pricing_mod
from flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_SKIPPED
from flux.strategies.makerv3.constants import REASON_SKIPPED_BOT_OFF
from flux.strategies.makerv3.constants import REASON_SKIPPED_QUOTE_FAIL_CIRCUIT_OPEN
from flux.strategies.makerv3.constants import REASON_SKIPPED_REQUOTE_THROTTLED
from flux.strategies.makerv3.constants import TOPIC_FV
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.identifiers import InstrumentId


if TYPE_CHECKING:
    from flux.strategies.makerv3.strategy import MakerV3Strategy


_price_to_decimal = pricing_mod.price_to_decimal


def _update_market_bbo(
    strategy: MakerV3Strategy,
    *,
    instrument_id: InstrumentId,
    bid_dec: Decimal,
    ask_dec: Decimal,
    now_ns: int,
) -> None:
    last = strategy._last_bbo.get(instrument_id)
    bbo_changed = last != (bid_dec, ask_dec)
    if bbo_changed:
        strategy._last_bbo[instrument_id] = (bid_dec, ask_dec)
    strategy._last_bbo_ts_ns[instrument_id] = now_ns

    should_publish_bbo = should_publish_market_bbo(
        bbo_changed=bbo_changed,
        last_publish_ns=strategy._last_market_bbo_publish_ns.get(instrument_id, 0),
        now_ns=now_ns,
        heartbeat_ms=strategy.MARKET_BBO_HEARTBEAT_MS,
    )
    if not should_publish_bbo:
        return

    strategy._last_market_bbo_publish_ns[instrument_id] = now_ns
    strategy._publish_market_bbo(
        instrument_id=instrument_id,
        bid=bid_dec,
        ask=ask_dec,
        ts_ns=now_ns,
    )
    if (
        bbo_changed
        or now_ns - strategy._last_fv_snapshot_ts_ns
        >= strategy.MARKET_BBO_HEARTBEAT_MS * 1_000_000
    ):
        strategy._recompute_and_publish_fv()
    strategy._publish_state_if_due()


def _maybe_refresh_quotes(
    strategy: MakerV3Strategy,
    *,
    instrument_id: InstrumentId,
    now_ns: int,
    context: str,
) -> None:
    bot_on_now = strategy._effective_bot_on()
    if strategy._quote_management_suspended():
        return
    is_maker_update = strategy.config.maker_instrument_id == instrument_id

    if is_maker_update and not bot_on_now:
        state_name = (
            "blocked_startup_cleanup"
            if bool(getattr(strategy, "_startup_cleanup_pending", False))
            else "bot_off"
        )
        strategy._publish_state(state_name)
        quote_cycle_id = strategy._next_quote_cycle_id(now_ns=now_ns)
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_SKIPPED,
            reason_code=REASON_SKIPPED_BOT_OFF,
            quote_cycle_id=quote_cycle_id,
        )
        return
    if not bot_on_now:
        return

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
    except Exception as e:
        strategy._handle_quote_failure(now_ns=now_ns, exc=e, context=context)


def should_publish_market_bbo(
    *,
    bbo_changed: bool,
    last_publish_ns: int,
    now_ns: int,
    heartbeat_ms: int,
) -> bool:
    """
    Return True when a BBO snapshot should be published (change or heartbeat).
    """
    if bbo_changed:
        return True
    if last_publish_ns <= 0:
        return True
    interval_ns = max(1, int(heartbeat_ms)) * 1_000_000
    return now_ns - last_publish_ns >= interval_ns


def on_order_book_deltas(strategy: MakerV3Strategy, deltas: OrderBookDeltas) -> None:  # noqa: C901
    """
    Process market deltas and trigger quote-cycle refresh when eligible.
    """
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
    _update_market_bbo(
        strategy,
        instrument_id=deltas.instrument_id,
        bid_dec=bid_dec,
        ask_dec=ask_dec,
        now_ns=now_ns,
    )

    strategy._publish_balances_if_due()
    _maybe_refresh_quotes(
        strategy,
        instrument_id=deltas.instrument_id,
        now_ns=int(strategy.clock.timestamp_ns()),
        context="on_order_book_deltas",
    )


def on_quote_tick(strategy: MakerV3Strategy, tick: object) -> None:
    """
    Process quote ticks and trigger quote-cycle refresh when eligible.
    """
    instrument_id = getattr(tick, "instrument_id", None)
    if instrument_id is None:
        return
    if not strategy._reference_uses_quote_ticks():
        return
    if instrument_id != strategy.config.reference_instrument_id:
        return
    bid = getattr(tick, "bid_price", None)
    ask = getattr(tick, "ask_price", None)
    if bid is None or ask is None:
        return

    now_ns = int(strategy.clock.timestamp_ns())
    bid_dec = _price_to_decimal(bid)
    ask_dec = _price_to_decimal(ask)
    _update_market_bbo(
        strategy,
        instrument_id=instrument_id,
        bid_dec=bid_dec,
        ask_dec=ask_dec,
        now_ns=now_ns,
    )
    strategy._publish_balances_if_due()
    _maybe_refresh_quotes(
        strategy,
        instrument_id=instrument_id,
        now_ns=now_ns,
        context="on_quote_tick",
    )


def best_bid_ask(
    strategy: MakerV3Strategy,
    instrument_id: InstrumentId,
) -> tuple[Decimal, Decimal] | None:
    """
    Return the best bid/ask decimal prices for an instrument.
    """
    book = strategy._books.get(instrument_id)
    if book is None:
        return None
    bid = book.best_bid_price()
    ask = book.best_ask_price()
    if bid is None or ask is None:
        return None
    return bid.as_decimal(), ask.as_decimal()


def best_mid(strategy: MakerV3Strategy, instrument_id: InstrumentId) -> Decimal | None:
    """
    Return the mid price derived from the current best bid/ask.
    """
    bbo = strategy._best_bid_ask(instrument_id)
    if bbo is None:
        return None
    bid, ask = bbo
    return (bid + ask) / Decimal(2)


def book_spread(strategy: MakerV3Strategy, instrument_id: InstrumentId) -> Decimal | None:
    """
    Return the current top-of-book spread, if available.
    """
    bbo = strategy._best_bid_ask(instrument_id)
    if bbo is None:
        return None
    bid, ask = bbo
    return ask - bid


def recompute_and_publish_fv(strategy: MakerV3Strategy) -> None:
    """
    Compute fair-value midpoint and publish it.
    """
    maker_mid = strategy._best_mid(strategy.config.maker_instrument_id)
    reference_mid = strategy._best_mid(strategy.config.reference_instrument_id)
    if maker_mid is None and reference_mid is None:
        return

    if maker_mid is not None and reference_mid is not None:
        strategy._last_fv = (maker_mid + reference_mid) / Decimal(2)
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
