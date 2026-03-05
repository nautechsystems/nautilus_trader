from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.flux.strategies import MakerV3Strategy as MakerV3StrategyFromRoot
from nautilus_trader.flux.strategies import MakerV3StrategyConfig as MakerV3StrategyConfigFromRoot
from nautilus_trader.flux.strategies.makerv3 import MakerV3Strategy
from nautilus_trader.flux.strategies.makerv3 import MakerV3StrategyConfig
from nautilus_trader.flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_BLOCKED
from nautilus_trader.flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_COMPLETED
from nautilus_trader.flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_SKIPPED
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_MD_STALE
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_REFERENCE_MD_STALE
from nautilus_trader.flux.strategies.makerv3.constants import REASON_COMPLETED_REBALANCED
from nautilus_trader.flux.strategies.makerv3.constants import REASON_SKIPPED_REQUOTE_THROTTLED
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_ALERT
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_EVENT


def test_quote_cycle_skipped_event_has_envelope_and_reason_code(clocked_strategy_factory) -> None:
    strategy = clocked_strategy_factory([1_000_000_000, 1_000_000_000])
    strategy._last_requote_ns = 900_000_000
    strategy._publish_event = MakerV3Strategy._publish_event.__get__(strategy, MakerV3Strategy)

    class _Book:
        def apply_deltas(self, _deltas: object) -> None:
            return

        def best_bid_price(self) -> Decimal:
            return Decimal(100)

        def best_ask_price(self) -> Decimal:
            return Decimal(101)

    strategy._books = {strategy.config.maker_instrument_id: _Book()}
    strategy._last_bbo = {strategy.config.maker_instrument_id: None}
    strategy._last_market_bbo_publish_ns = {strategy.config.maker_instrument_id: 0}
    strategy._publish_market_bbo = lambda *_args, **_kwargs: None
    strategy._publish_state_if_due = lambda: None
    strategy._publish_balances_if_due = lambda: None
    strategy._recompute_and_publish_fv = lambda: None

    payloads: list[tuple[str, dict[str, object]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy.on_order_book_deltas(
        SimpleNamespace(instrument_id=strategy.config.maker_instrument_id),
    )

    quote_cycle_events = [
        payload
        for topic, payload in payloads
        if topic == TOPIC_EVENT and payload.get("event") == "quote_cycle"
    ]
    assert len(quote_cycle_events) == 1
    assert quote_cycle_events[0]["quote_cycle_event"] == QUOTE_CYCLE_EVENT_SKIPPED
    assert quote_cycle_events[0]["reason_code"] == REASON_SKIPPED_REQUOTE_THROTTLED
    assert quote_cycle_events[0]["run_id"]
    assert quote_cycle_events[0]["quote_cycle_id"]
    assert quote_cycle_events[0]["ts_ms"] == 1_000


def test_quote_cycle_blocked_event_has_transition_details(clocked_strategy_factory) -> None:
    strategy = clocked_strategy_factory(
        [1_000_000_001, 1_000_000_002, 1_000_000_003, 1_000_000_004],
    )
    strategy._publish_event = MakerV3Strategy._publish_event.__get__(strategy, MakerV3Strategy)
    strategy._publish_alert = lambda *_args, **_kwargs: None
    strategy._cancel_managed_quotes = lambda *_args, **_kwargs: None
    strategy._managed_orders = list
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000 - 200_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000 - 10_000_000

    payloads: list[tuple[str, dict[str, object]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._refresh_quotes(now_ns=1_000_000_000)

    quote_cycle_events = [
        payload
        for topic, payload in payloads
        if topic == TOPIC_EVENT and payload.get("event") == "quote_cycle"
    ]
    assert len(quote_cycle_events) == 1
    assert quote_cycle_events[0]["quote_cycle_event"] == QUOTE_CYCLE_EVENT_BLOCKED
    assert quote_cycle_events[0]["reason_code"] == REASON_BLOCKED_MAKER_MD_STALE
    assert quote_cycle_events[0]["blocked_transition"] is True
    assert quote_cycle_events[0]["to_state"] == "blocked_maker_md"


def test_quote_cycle_completed_event_contains_action_counts(clocked_strategy_factory) -> None:
    strategy = clocked_strategy_factory(
        [1_000_000_001, 1_000_000_002, 1_000_000_003, 1_000_000_004],
    )
    strategy._publish_event = MakerV3Strategy._publish_event.__get__(strategy, MakerV3Strategy)
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000 - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000 - 10_000_000
    strategy._managed_orders = list
    strategy._rebalance_side = lambda **_kwargs: 1
    strategy._place_missing_levels = lambda **_kwargs: 2

    payloads: list[tuple[str, dict[str, object]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._refresh_quotes(now_ns=1_000_000_000)

    quote_cycle_events = [
        payload
        for topic, payload in payloads
        if topic == TOPIC_EVENT and payload.get("event") == "quote_cycle"
    ]
    assert len(quote_cycle_events) == 1
    assert quote_cycle_events[0]["quote_cycle_event"] == QUOTE_CYCLE_EVENT_COMPLETED
    assert quote_cycle_events[0]["reason_code"] == REASON_COMPLETED_REBALANCED
    assert quote_cycle_events[0]["cancel_count"] == 2
    assert quote_cycle_events[0]["place_count"] == 4


def test_blocked_alerts_are_rate_limited_until_unblocked_transition(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory(
        [
            1_000_000_000,
            1_001_000_000,
            1_002_000_000,
            1_003_000_000,
            1_004_000_000,
            1_005_000_000,
        ],
    )
    strategy.STALE_CANCEL_COOLDOWN_MS = 0
    strategy._publish_event = MakerV3Strategy._publish_event.__get__(strategy, MakerV3Strategy)
    strategy._publish_alert = MakerV3Strategy._publish_alert.__get__(strategy, MakerV3Strategy)
    strategy._managed_orders = list
    strategy._cancel_managed_quotes = lambda *_args, **_kwargs: None

    payloads: list[tuple[str, dict[str, object]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._handle_stale_quote_block(
        now_ns=100_000_000,
        state="blocked_reference_md",
        cancel_reason="reference_md_stale",
        reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
        quote_cycle_id="cycle-1",
        warning_message="blocked",
    )
    strategy._handle_stale_quote_block(
        now_ns=110_000_000,
        state="blocked_reference_md",
        cancel_reason="reference_md_stale",
        reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
        quote_cycle_id="cycle-2",
        warning_message="blocked",
    )
    strategy._publish_state("running")
    strategy._handle_stale_quote_block(
        now_ns=120_000_000,
        state="blocked_reference_md",
        cancel_reason="reference_md_stale",
        reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
        quote_cycle_id="cycle-3",
        warning_message="blocked",
    )

    alerts = [payload for topic, payload in payloads if topic == TOPIC_ALERT]
    assert len(alerts) == 2
    assert all(payload["level"] == "warning" for payload in alerts)


def test_canonical_strategy_exports_match_root_surface() -> None:
    assert MakerV3StrategyFromRoot is MakerV3Strategy
    assert MakerV3StrategyConfigFromRoot is MakerV3StrategyConfig


def test_publish_json_emits_canonical_topic() -> None:
    published_topics: list[str] = []
    strategy = SimpleNamespace(
        msgbus=SimpleNamespace(
            publish=lambda topic, msg: published_topics.append(topic),
        ),
    )
    publish_json = MakerV3Strategy._publish_json.__get__(strategy, MakerV3Strategy)

    publish_json(TOPIC_EVENT, {"event": "compat"})

    assert published_topics == [TOPIC_EVENT]
