from __future__ import annotations

import json
from decimal import Decimal
from types import SimpleNamespace
from typing import Any

import pytest

from nautilus_trader.flux.api.payloads import build_balances_rows
from nautilus_trader.flux.strategies import MakerV3Strategy as MakerV3StrategyFromRoot
from nautilus_trader.flux.strategies import MakerV3StrategyConfig as MakerV3StrategyConfigFromRoot
from nautilus_trader.flux.strategies.makerv3 import failures as failures_mod
from nautilus_trader.flux.strategies.makerv3 import MakerV3Strategy
from nautilus_trader.flux.strategies.makerv3 import MakerV3StrategyConfig
from nautilus_trader.flux.strategies.makerv3 import publisher as publisher_mod
from nautilus_trader.flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_BLOCKED
from nautilus_trader.flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_COMPLETED
from nautilus_trader.flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_SKIPPED
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_MD_STALE
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_REFERENCE_MD_STALE
from nautilus_trader.flux.strategies.makerv3.constants import REASON_COMPLETED_NO_ACTIONS
from nautilus_trader.flux.strategies.makerv3.constants import REASON_COMPLETED_REBALANCED
from nautilus_trader.flux.strategies.makerv3.constants import REASON_SKIPPED_REQUOTE_THROTTLED
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_ALERT
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_BALANCES
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_EVENT
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_STATE
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_TRADE
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity


def _json_mapping(value: Any) -> dict[str, Any]:
    if isinstance(value, str):
        return json.loads(value)
    return value


class _MutatingPendingCancelFirstSeen(dict[str, int]):
    def __init__(
        self,
        *,
        strategy: MakerV3Strategy,
        mutate_client_order_id: str,
        values: dict[str, int],
    ) -> None:
        super().__init__(values)
        self._strategy = strategy
        self._mutate_client_order_id = mutate_client_order_id
        self._did_mutate = False

    def get(self, key: object, default: Any = None) -> Any:
        if not self._did_mutate:
            self._did_mutate = True
            self._strategy._pending_cancel_client_order_ids.discard(self._mutate_client_order_id)
        return super().get(key, default)


def test_makerv3_role_map_payload_keeps_ref_as_hedge_leg(monkeypatch) -> None:
    strategy = SimpleNamespace(
        config=SimpleNamespace(
            maker_instrument_id="maker-leg",
            reference_instrument_id="ref-leg",
        ),
    )
    role_ids = {
        "maker-leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
        "ref-leg": "ibkr:AAPL.NASDAQ",
    }
    monkeypatch.setattr(
        publisher_mod,
        "_contract_role_id",
        lambda _strategy, instrument_id: role_ids[instrument_id],
    )

    assert publisher_mod._maker_role_map_payload(strategy) == {
        "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
        "ref_leg": "ibkr:AAPL.NASDAQ",
        "hedge_leg": "ibkr:AAPL.NASDAQ",
    }


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

    payloads: list[tuple[str, dict[str, Any]]] = []
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


def test_quote_cycle_skipped_event_exports_trigger_timestamps(
    clocked_strategy_factory,
) -> None:
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

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy.on_order_book_deltas(
        SimpleNamespace(
            instrument_id=strategy.config.maker_instrument_id,
            ts_event=111_111_111,
            ts_init=222_222_222,
        ),
    )

    quote_cycle_events = [
        payload
        for topic, payload in payloads
        if topic == TOPIC_EVENT and payload.get("event") == "quote_cycle"
    ]
    assert len(quote_cycle_events) == 1
    assert quote_cycle_events[0]["instrument_id"] == str(strategy.config.maker_instrument_id)
    assert quote_cycle_events[0]["quote_cycle_seq"] == 1
    assert quote_cycle_events[0]["trigger_source"] == "maker_bbo_update"
    assert quote_cycle_events[0]["trigger_md_ts_event_ns"] == 111_111_111
    assert quote_cycle_events[0]["trigger_md_ts_init_ns"] == 222_222_222
    assert quote_cycle_events[0]["ts_cycle_start_ns"] == 1_000_000_000
    assert quote_cycle_events[0]["ts_cycle_end_ns"] == 1_000_000_000
    assert "decision_context_json" not in quote_cycle_events[0]


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

    payloads: list[tuple[str, dict[str, Any]]] = []
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

    payloads: list[tuple[str, dict[str, Any]]] = []
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
    assert quote_cycle_events[0]["place_count"] == 0
    bounded_convergence = quote_cycle_events[0]["decision_context_json"]["bounded_convergence"]
    assert bounded_convergence["buy"]["executed_cancel_count"] == 1
    assert bounded_convergence["buy"]["executed_place_count"] == 0
    assert bounded_convergence["sell"]["executed_cancel_count"] == 1
    assert bounded_convergence["sell"]["executed_place_count"] == 0


def test_quote_cycle_completed_event_exports_cycle_timing(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000, 1_000_500_000])
    strategy._publish_event = MakerV3Strategy._publish_event.__get__(strategy, MakerV3Strategy)
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_450_000
    strategy._managed_orders = list
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0

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

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy.on_order_book_deltas(
        SimpleNamespace(
            instrument_id=strategy.config.maker_instrument_id,
            ts_event=333_333_333,
            ts_init=444_444_444,
        ),
    )

    quote_cycle_events = [
        payload
        for topic, payload in payloads
        if topic == TOPIC_EVENT and payload.get("event") == "quote_cycle"
    ]
    assert len(quote_cycle_events) == 1
    assert quote_cycle_events[0]["quote_cycle_event"] == QUOTE_CYCLE_EVENT_COMPLETED
    assert quote_cycle_events[0]["reason_code"] == REASON_COMPLETED_NO_ACTIONS
    assert quote_cycle_events[0]["quote_cycle_seq"] == 1
    assert quote_cycle_events[0]["trigger_source"] == "maker_bbo_update"
    assert quote_cycle_events[0]["trigger_md_ts_event_ns"] == 333_333_333
    assert quote_cycle_events[0]["trigger_md_ts_init_ns"] == 444_444_444
    assert quote_cycle_events[0]["ts_cycle_start_ns"] <= quote_cycle_events[0]["ts_cycle_end_ns"]
    decision_context = _json_mapping(quote_cycle_events[0]["decision_context_json"])
    assert decision_context["pricing"]["maker_top_bid"] == "100"


def test_on_order_filled_releases_cached_place_intent_after_trade_publish(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._publish_portfolio_inventory_component = lambda *_args, **_kwargs: None

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    client_order_id = "CLIENT-1"
    strategy._latest_place_intent_by_client_order_id[client_order_id] = {
        "run_id": "run-telemetry-001",
        "quote_cycle_id": "run-telemetry-001:11",
        "reason_code": "place_missing_level",
        "level_index": 2,
    }

    strategy.on_order_filled(
        SimpleNamespace(
            client_order_id=client_order_id,
            trade_id="TRADE-1",
            instrument_id=strategy.config.maker_instrument_id,
            order_side=OrderSide.BUY,
            last_qty=Decimal("1"),
            last_px=Decimal("100.25"),
            ts_event=1_000_123_000,
        ),
    )

    trade_payloads = [payload for topic, payload in payloads if topic == TOPIC_TRADE]
    assert len(trade_payloads) == 1
    assert trade_payloads[0]["run_id"] == "run-telemetry-001"
    assert trade_payloads[0]["quote_cycle_id"] == "run-telemetry-001:11"
    assert trade_payloads[0]["reason_code"] == "place_missing_level"
    assert client_order_id not in strategy._latest_place_intent_by_client_order_id


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

    payloads: list[tuple[str, dict[str, Any]]] = []
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


def test_publish_state_preserves_quote_status_when_bot_off_with_cached_managed_orders(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._managed_orders = lambda: [
        SimpleNamespace(side=OrderSide.BUY),
        SimpleNamespace(side=OrderSide.SELL),
    ]

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_state("bot_off")

    state_payload = next(payload for topic, payload in payloads if topic == TOPIC_STATE)
    assert state_payload["managed_orders"] == 2
    assert state_payload["maker_quote_status"] == {
        "bid_open": 1,
        "ask_open": 1,
        "bid_depth": 5,
        "ask_depth": 5,
        "bid_blocked": 4,
        "ask_blocked": 4,
    }


def test_publish_state_uses_cache_truth_not_tracked_managed_order_ids(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._managed_orders = lambda: []
    strategy._managed_client_order_ids = {"A", "B"}

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._runtime_params["n_orders1"] = 5
    strategy._publish_state("quotes_replaced")

    state_payload = next(payload for topic, payload in payloads if topic == TOPIC_STATE)
    assert state_payload["managed_orders"] == 0
    assert state_payload["maker_quote_status"] == {
        "bid_open": 0,
        "ask_open": 0,
        "bid_depth": 5,
        "ask_depth": 5,
        "bid_blocked": 5,
        "ask_blocked": 5,
    }


def test_on_order_pending_cancel_republishes_current_state_with_cache_truth(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000, 1_000_000_000])
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._managed_orders = lambda: []
    strategy._last_state_name = "quotes_replaced"
    strategy._runtime_params["n_orders1"] = 5

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy.on_order_pending_cancel(SimpleNamespace(client_order_id="A"))

    state_payload = next(payload for topic, payload in payloads if topic == TOPIC_STATE)
    assert state_payload["state"] == "quotes_replaced"
    assert state_payload["managed_orders"] == 0
    assert state_payload["maker_quote_status"] == {
        "bid_open": 0,
        "ask_open": 0,
        "bid_depth": 5,
        "ask_depth": 5,
        "bid_blocked": 5,
        "ask_blocked": 5,
    }


def test_on_order_pending_cancel_state_exports_quote_blocker_metadata(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._managed_orders = lambda: []
    strategy._last_state_name = "running"
    strategy._runtime_params["n_orders1"] = 5

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy.on_order_pending_cancel(SimpleNamespace(client_order_id="A", ts_event=1_000_000_000))

    state_payload = next(payload for topic, payload in payloads if topic == TOPIC_STATE)
    assert state_payload["quote_progress"]["pending_cancel_count"] == 1
    assert state_payload["quote_progress"]["last_order_event_ts_ms"] == 1_000
    assert state_payload["quote_progress"]["oldest_pending_cancel_age_ms"] == 0
    assert state_payload["quote_blockers"][0]["reason_code"] == "pending_cancel_in_flight"
    assert state_payload["quote_blockers"][0]["oldest_pending_cancel_age_ms"] == 0


def test_publish_state_exports_last_completed_quote_progress(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._managed_orders = lambda: []
    strategy._last_completed_quote_ns = 1_234_000_000

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_state("running")

    state_payload = next(payload for topic, payload in payloads if topic == TOPIC_STATE)
    assert state_payload["quote_progress"]["last_completed_quote_ts_ms"] == 1_234


def test_quote_progress_payload_snapshots_pending_cancel_set_before_iterating(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._last_state_ns = 1_000_000_000
    strategy._last_completed_quote_ns = 900_000_000
    strategy._pending_cancel_client_order_ids = {"RESTING-1", "RESTING-2"}
    strategy._pending_cancel_first_seen_ns_by_client_order_id = _MutatingPendingCancelFirstSeen(
        strategy=strategy,
        mutate_client_order_id="RESTING-2",
        values={
            "RESTING-1": 900_000_000,
            "RESTING-2": 950_000_000,
        },
    )

    payload = strategy._quote_progress_payload()

    assert payload == {
        "last_completed_quote_ts_ms": 900,
        "pending_cancel_count": 2,
        "oldest_pending_cancel_age_ms": 100,
    }


def test_venue_protection_path_still_publishes_blocked_state_when_pending_cancel_sets_mutate(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._managed_orders = lambda: []
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_actionable_alert = lambda **_kwargs: True
    strategy._cancel_managed_quotes = lambda *_args, **_kwargs: None
    strategy.request_immediate_stop = lambda *_args, **_kwargs: None
    strategy.stop_immediately = lambda: None
    strategy._pending_cancel_client_order_ids = {"RESTING-1", "RESTING-2"}
    strategy._pending_cancel_first_seen_ns_by_client_order_id = _MutatingPendingCancelFirstSeen(
        strategy=strategy,
        mutate_client_order_id="RESTING-2",
        values={
            "RESTING-1": 900_000_000,
            "RESTING-2": 950_000_000,
        },
    )

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    failures_mod.handle_venue_protection(
        strategy,
        now_ns=1_000_000_000,
        reason="Too many visits. Exceeded the API Rate Limit.",
        source_event="order_cancel_rejected",
        client_order_id="RESTING-1",
    )

    state_payload = next(payload for topic, payload in payloads if topic == TOPIC_STATE)
    assert state_payload["state"] == "blocked_venue_protection"
    assert state_payload["quote_progress"]["pending_cancel_count"] == 2


def test_canonical_strategy_exports_match_root_surface() -> None:
    assert MakerV3StrategyFromRoot is MakerV3Strategy
    assert MakerV3StrategyConfigFromRoot is MakerV3StrategyConfig


def test_flux_strategy_registry_exposes_canonical_makerv3_binding() -> None:
    from nautilus_trader.flux.strategies.registry import get_strategy_spec

    spec = get_strategy_spec("makerv3")

    assert spec.param_set == "makerv3"
    assert spec.strategy_cls is MakerV3Strategy
    assert spec.config_cls is MakerV3StrategyConfig
    assert spec.strategy_family == "maker_v3"
    assert spec.strategy_version == "v3"


def test_flux_strategy_registry_rejects_unknown_param_sets() -> None:
    from nautilus_trader.flux.strategies.registry import get_strategy_spec

    with pytest.raises(ValueError, match="Unsupported flux strategy param set"):
        get_strategy_spec("makerv5")


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


def test_publish_balances_filters_fallback_positions_to_maker_base_asset(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-PERP.BYBIT")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE")
    other_base_id = InstrumentId.from_str("BTCUSDT.BYBIT")
    strategy = clocked_strategy_factory(
        [1_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
    )
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        other_base_id: SimpleNamespace(
            id=other_base_id,
            base_currency=SimpleNamespace(code="BTC"),
        ),
    }
    positions = [
        SimpleNamespace(instrument_id=maker_instrument_id, signed_qty=Decimal(2)),
        SimpleNamespace(instrument_id=other_base_id, signed_qty=Decimal(9)),
    ]
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        accounts=list,
        positions_open=lambda instrument_id=None: positions if instrument_id is None else [],
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_balances()

    balances_payload = next(payload for topic, payload in payloads if topic == TOPIC_BALANCES)
    positions_payload = balances_payload["positions"]

    assert len(positions_payload) == 1
    assert positions_payload[0]["instrument_id"] == str(maker_instrument_id)

    rows = build_balances_rows(
        raw_snapshot=balances_payload["positions"],
        strategy_id=strategy._external_strategy_id,
    )
    position_rows = [row for row in rows if str(row.get("kind")).lower() == "position"]
    assert len(position_rows) == 1
    assert position_rows[0]["instrument_id"] == str(maker_instrument_id)


def test_publish_balances_does_not_fallback_to_same_base_position_from_other_venue(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE")
    other_venue_instrument_id = InstrumentId.from_str("PLUME-USDT-SWAP.OKX")
    strategy = clocked_strategy_factory(
        [1_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
    )
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        other_venue_instrument_id: SimpleNamespace(
            id=other_venue_instrument_id,
            base_currency=SimpleNamespace(code="PLUME"),
        ),
    }
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        accounts=list,
        positions_open=lambda instrument_id=None: (
            []
            if instrument_id == maker_instrument_id
            else [SimpleNamespace(instrument_id=other_venue_instrument_id, signed_qty=Decimal(9))]
        ),
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_balances()

    balances_payload = next(payload for topic, payload in payloads if topic == TOPIC_BALANCES)
    assert balances_payload["positions"] == []


def test_publish_balances_does_not_fallback_to_same_base_position_from_other_instrument_same_venue(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    reference_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE")
    other_instrument_id = InstrumentId.from_str("PLUMEUSDT-SPOT.BYBIT")
    strategy = clocked_strategy_factory(
        [1_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )
    strategy._maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
    )
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        other_instrument_id: SimpleNamespace(
            id=other_instrument_id,
            base_currency=SimpleNamespace(code="PLUME"),
        ),
    }
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        accounts=list,
        positions_open=lambda instrument_id=None: (
            []
            if instrument_id == maker_instrument_id
            else [SimpleNamespace(instrument_id=other_instrument_id, signed_qty=Decimal(9))]
        ),
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_balances()

    balances_payload = next(payload for topic, payload in payloads if topic == TOPIC_BALANCES)
    assert balances_payload["positions"] == []


def test_publish_balances_skips_portfolio_lookup_when_no_cached_accounts(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        accounts=lambda: [],
        positions_open=lambda instrument_id=None: [],
        instrument=lambda instrument_id: None,
    )
    account_calls: list[object] = []
    strategy._portfolio = SimpleNamespace(
        account=lambda **kwargs: account_calls.append(kwargs),
    )

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_balances()

    balances_payload = next(payload for topic, payload in payloads if topic == TOPIC_BALANCES)
    assert balances_payload["accounts"] == []
    assert account_calls == []


def test_publish_balances_uses_fresh_venue_position_report_for_maker_instrument(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    strategy = clocked_strategy_factory(
        [1_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE"),
    )
    strategy._maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
        quote_currency=SimpleNamespace(code="USDT"),
        settlement_currency=SimpleNamespace(code="USDT"),
        is_inverse=False,
        multiplier=SimpleNamespace(as_decimal=lambda: Decimal("10")),
        info={"base_exposure_mode": "exact_multiplier"},
        make_qty=lambda value: SimpleNamespace(as_decimal=lambda: Decimal(str(value))),
        make_price=lambda value: SimpleNamespace(as_decimal=lambda: Decimal(str(value))),
        calculate_base_exposure_qty=lambda qty, _price=None: qty.as_decimal() * Decimal("10"),
    )
    stale_positions = [
        SimpleNamespace(
            instrument_id=maker_instrument_id,
            signed_qty=Decimal("371135"),
            avg_px_open=Decimal("0.01101"),
        ),
        SimpleNamespace(
            instrument_id=maker_instrument_id,
            signed_qty=Decimal("-173371"),
            avg_px_open=Decimal("0.01078"),
        ),
    ]
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        accounts=list,
        positions_open=lambda instrument_id=None: (
            stale_positions
            if instrument_id is None or instrument_id == maker_instrument_id
            else []
        ),
        instrument=lambda instrument_id: (
            strategy._maker_instrument if instrument_id == maker_instrument_id else None
        ),
    )
    strategy._last_maker_position_activity_ns = 100
    report = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_decimal_qty=Decimal("99382"),
        avg_px_open=Decimal("0.0109378"),
        ts_last=200,
        ts_init=210,
        venue_position_id=None,
    )
    strategy._handle_execution_report_message(
        SimpleNamespace(position_reports={maker_instrument_id: [report]}, ts_init=210),
    )

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_balances()

    balances_payload = next(payload for topic, payload in payloads if topic == TOPIC_BALANCES)
    assert len(balances_payload["positions"]) == 1
    assert balances_payload["positions"][0]["instrument_id"] == str(maker_instrument_id)
    assert balances_payload["positions"][0]["signed_qty"] == "99382"
    assert balances_payload["positions"][0]["side"] == "LONG"
    assert balances_payload["positions"][0]["signed_qty_venue"] == "99382"
    assert balances_payload["positions"][0]["quantity_venue"] == "99382"
    assert balances_payload["positions"][0]["signed_qty_base"] == "993820"
    assert balances_payload["positions"][0]["quantity_base"] == "993820"
    assert balances_payload["positions"][0]["qty_conversion_status"] == "exact_multiplier"
    assert (
        balances_payload["positions"][0]["qty_conversion_source"]
        == "instrument.info:base_exposure_mode=exact_multiplier"
    )


def test_publish_balances_keeps_fresh_venue_position_report_when_position_events_are_newer(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    strategy = clocked_strategy_factory(
        [1_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE"),
    )
    strategy._maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
        quote_currency=SimpleNamespace(code="USDT"),
        settlement_currency=SimpleNamespace(code="USDT"),
        is_inverse=False,
        multiplier=SimpleNamespace(as_decimal=lambda: Decimal("1")),
        info={"base_exposure_mode": "identity"},
        make_qty=lambda value: SimpleNamespace(as_decimal=lambda: Decimal(str(value))),
        make_price=lambda value: SimpleNamespace(as_decimal=lambda: Decimal(str(value))),
        calculate_base_exposure_qty=lambda qty, _price=None: qty.as_decimal(),
    )
    stale_positions = [
        SimpleNamespace(
            instrument_id=maker_instrument_id,
            signed_qty=Decimal("371135"),
            avg_px_open=Decimal("0.01101"),
        ),
        SimpleNamespace(
            instrument_id=maker_instrument_id,
            signed_qty=Decimal("-173371"),
            avg_px_open=Decimal("0.01078"),
        ),
    ]
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        accounts=list,
        positions_open=lambda instrument_id=None: (
            stale_positions
            if instrument_id is None or instrument_id == maker_instrument_id
            else []
        ),
        instrument=lambda instrument_id: (
            strategy._maker_instrument if instrument_id == maker_instrument_id else None
        ),
    )
    strategy._last_maker_position_activity_ns = 100
    report = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_decimal_qty=Decimal("99382"),
        avg_px_open=Decimal("0.0109378"),
        ts_last=200,
        ts_init=210,
        venue_position_id=None,
    )
    strategy._handle_execution_report_message(
        SimpleNamespace(position_reports={maker_instrument_id: [report]}, ts_init=210),
    )
    strategy.on_position_changed(
        SimpleNamespace(
            instrument_id=maker_instrument_id,
            ts_event=300,
        ),
    )

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_balances()

    balances_payload = next(payload for topic, payload in payloads if topic == TOPIC_BALANCES)
    assert len(balances_payload["positions"]) == 1
    assert balances_payload["positions"][0]["instrument_id"] == str(maker_instrument_id)
    assert balances_payload["positions"][0]["signed_qty"] == "99382"
    assert balances_payload["positions"][0]["signed_qty_venue"] == "99382"
    assert balances_payload["positions"][0]["signed_qty_base"] == "99382"
    assert balances_payload["positions"][0]["qty_conversion_status"] == "identity"
    assert (
        balances_payload["positions"][0]["qty_conversion_source"]
        == "instrument.info:base_exposure_mode=identity"
    )


def test_publish_balances_preserves_precomputed_dual_unit_snapshot_fields(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    strategy = clocked_strategy_factory(
        [1_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE"),
    )
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        accounts=list,
        positions_open=lambda instrument_id=None: [],
        instrument=lambda instrument_id: None,
    )
    strategy._fresh_maker_position_report_snapshot = lambda: {
        "instrument_id": maker_instrument_id,
        "signed_qty": Decimal("99382"),
        "signed_qty_venue": Decimal("99382"),
        "quantity_venue": Decimal("99382"),
        "signed_qty_base": Decimal("993820"),
        "quantity_base": Decimal("993820"),
        "qty_conversion_status": "exact_multiplier",
        "qty_conversion_source": "instrument.info:base_exposure_mode=exact_multiplier",
        "avg_px_open": Decimal("0.0109378"),
    }

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_balances()

    balances_payload = next(payload for topic, payload in payloads if topic == TOPIC_BALANCES)
    position_payload = balances_payload["positions"][0]
    assert position_payload["signed_qty"] == "99382"
    assert position_payload["quantity"] == "99382"
    assert position_payload["signed_qty_venue"] == "99382"
    assert position_payload["quantity_venue"] == "99382"
    assert position_payload["signed_qty_base"] == "993820"
    assert position_payload["quantity_base"] == "993820"
    assert position_payload["qty_conversion_status"] == "exact_multiplier"
    assert (
        position_payload["qty_conversion_source"]
        == "instrument.info:base_exposure_mode=exact_multiplier"
    )


def test_publish_balances_does_not_fallback_to_cache_when_fresh_flat_position_report_is_zero(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    strategy = clocked_strategy_factory(
        [1_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE"),
    )
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        accounts=list,
        positions_open=lambda instrument_id=None: [
            SimpleNamespace(
                instrument_id=maker_instrument_id,
                signed_qty=Decimal("371135"),
                avg_px_open=Decimal("0.01101"),
            ),
        ],
        instrument=lambda instrument_id: None,
    )
    strategy._fresh_maker_position_report_snapshot = lambda: {
        "instrument_id": maker_instrument_id,
        "signed_qty": Decimal("0"),
        "signed_qty_venue": Decimal("0"),
        "quantity_venue": Decimal("0"),
        "signed_qty_base": Decimal("0"),
        "quantity_base": Decimal("0"),
        "qty_conversion_status": "exact_multiplier",
        "qty_conversion_source": "instrument.info:base_exposure_mode=exact_multiplier",
        "avg_px_open": Decimal("0.0109378"),
    }

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_balances()

    balances_payload = next(payload for topic, payload in payloads if topic == TOPIC_BALANCES)
    assert balances_payload["positions"] == []


def test_publish_balances_cache_fallback_ignores_stale_external_reconciliation_artifact(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-PERP.BITGET")
    strategy = clocked_strategy_factory(
        [1_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE"),
    )
    strategy._maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
        quote_currency=SimpleNamespace(code="USDT"),
        settlement_currency=SimpleNamespace(code="USDT"),
        is_inverse=False,
        multiplier=Quantity.from_str("1"),
        info={"base_exposure_mode": "identity"},
        make_qty=lambda value: Quantity.from_str(str(value)),
        make_price=lambda value: value,
        calculate_base_exposure_qty=lambda qty, _price=None: qty.as_decimal(),
    )
    owned_position = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_qty=Decimal("-250030"),
        avg_px_open=Decimal("0.0109378"),
        strategy_id="plumeusdt_bitget_perp_makerv3",
        position_id="P-OWNED",
        to_dict=lambda: {
            "kind": "position",
            "instrument_id": str(maker_instrument_id),
            "signed_qty": "-250030",
            "quantity": "250030",
            "side": "SHORT",
            "strategy_id": "plumeusdt_bitget_perp_makerv3",
            "position_id": "P-OWNED",
        },
    )
    stale_external_position = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_qty=Decimal("-250030"),
        avg_px_open=Decimal("0.0109378"),
        strategy_id="EXTERNAL",
        position_id="P-EXTERNAL",
        to_dict=lambda: {
            "kind": "position",
            "instrument_id": str(maker_instrument_id),
            "signed_qty": "-250030",
            "quantity": "250030",
            "side": "SHORT",
            "strategy_id": "EXTERNAL",
            "position_id": "P-EXTERNAL",
        },
    )
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        accounts=list,
        positions_open=lambda instrument_id=None: (
            [owned_position, stale_external_position]
            if instrument_id is None or instrument_id == maker_instrument_id
            else []
        ),
        orders_for_position=lambda _position_id: [],
        instrument=lambda instrument_id: (
            strategy._maker_instrument if instrument_id == maker_instrument_id else None
        ),
    )

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_balances()

    balances_payload = next(payload for topic, payload in payloads if topic == TOPIC_BALANCES)
    assert len(balances_payload["positions"]) == 1
    assert balances_payload["positions"][0]["position_id"] == "P-OWNED"
    assert balances_payload["positions"][0]["signed_qty"] == "-250030"

    rows = build_balances_rows(raw_snapshot=balances_payload, strategy_id=strategy._external_strategy_id)
    position_rows = [row for row in rows if str(row.get("kind")).lower() == "position"]
    assert len(position_rows) == 1
    assert position_rows[0]["signed_qty"] == "-250030"


def test_publish_balances_converts_okx_venue_report_to_base_once(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUME-USDT-SWAP.OKX")
    strategy = clocked_strategy_factory(
        [1_000_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE"),
    )
    maker_instrument = SimpleNamespace(
        id=maker_instrument_id,
        base_currency=SimpleNamespace(code="PLUME"),
        multiplier=Quantity.from_str("10"),
        info={
            "okx_ct_val": "10",
            "okx_ct_val_ccy": "PLUME",
            "okx_ct_type": "linear",
            "okx_lot_sz": "1",
            "base_exposure_mode": "exact_multiplier",
        },
        make_qty=lambda value: Quantity.from_str(str(value)),
        make_price=lambda value: Decimal(str(value)),
        calculate_base_exposure_qty=lambda qty, _price=None: qty.as_decimal() * Decimal("10"),
    )
    strategy._maker_instrument = maker_instrument
    strategy._cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        accounts=list,
        positions_open=lambda instrument_id=None: [],
        instrument=lambda instrument_id: (
            maker_instrument if instrument_id == maker_instrument_id else None
        ),
    )
    strategy._last_maker_position_activity_ns = 100
    report = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_decimal_qty=Decimal("-657"),
        avg_px_open=Decimal("0.0109378"),
        ts_last=200,
        ts_init=210,
        venue_position_id=None,
    )
    strategy._handle_execution_report_message(
        SimpleNamespace(position_reports={maker_instrument_id: [report]}, ts_init=210),
    )

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_balances()

    balances_payload = next(payload for topic, payload in payloads if topic == TOPIC_BALANCES)
    position_payload = balances_payload["positions"][0]
    assert position_payload["signed_qty"] == "-657"
    assert position_payload["quantity"] == "657"
    assert position_payload["signed_qty_venue"] == "-657"
    assert position_payload["quantity_venue"] == "657"
    assert position_payload["signed_qty_base"] == "-6570"
    assert position_payload["quantity_base"] == "6570"

    rows = build_balances_rows(
        raw_snapshot=[balances_payload],
        strategy_id=strategy._external_strategy_id,
    )
    position_rows = [row for row in rows if row.get("kind") == "position"]
    assert len(position_rows) == 1
    assert position_rows[0]["signed_qty"] == "-6570"
    assert position_rows[0]["signed_qty_venue"] == "-657"
    assert position_rows[0]["signed_qty_base"] == "-6570"


def test_publish_state_backfills_inventory_skew_when_quote_cycle_has_not_run(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._managed_orders = list
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._quote_runtime_params_snapshot = lambda: {
        "des_qty_global": Decimal(0),
        "max_qty_global": Decimal(50000),
        "max_skew_bps_global": Decimal(20),
        "des_qty_local": Decimal(0),
        "max_qty_local": Decimal(100000),
        "max_skew_bps_local": Decimal(0),
        "linear_offset_bps": Decimal(0),
    }
    strategy._compute_inventory_skew = lambda **_kwargs: {
        "inventory_qty_base": Decimal("33317.3519"),
        "inventory_qty": Decimal("33317.3519"),
        "inventory_source": "portfolio_component_sum",
        "base_currency": "PLUME",
        "position_qty_base": None,
        "position_qty_venue": None,
        "position_qty": None,
        "spot_qty": None,
        "global_position_qty_base": None,
        "global_position_qty_venue": None,
        "global_position_qty": None,
        "global_spot_qty": None,
        "global_inventory_qty_base": Decimal("33317.3519"),
        "global_inventory_qty": Decimal("33317.3519"),
        "global_inventory_source": "portfolio_component_sum",
        "local_position_qty_base": Decimal("-98060"),
        "local_position_qty_venue": Decimal(-9806),
        "local_position_qty": Decimal(-9806),
        "local_spot_qty": None,
        "local_inventory_qty_base": Decimal("-98060"),
        "local_inventory_qty": Decimal(-9806),
        "local_inventory_source": "positions",
        "local_position_qty_conversion_status": "exact_multiplier",
        "local_position_qty_conversion_source": "instrument.info:base_exposure_mode=exact_multiplier",
        "des_qty_global": Decimal(0),
        "max_qty_global": Decimal(50000),
        "max_skew_bps_global": Decimal(20),
        "des_qty_local": Decimal(0),
        "max_qty_local": Decimal(100000),
        "max_skew_bps_local": Decimal(0),
        "linear_offset_bps": Decimal(0),
        "global_ratio": Decimal("0.666347038"),
        "global_skew_bps": Decimal("13.32694076"),
        "local_ratio": Decimal("-0.09806"),
        "local_skew_bps": Decimal(0),
        "total_skew_bps": Decimal("13.32694076"),
    }

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_state("bot_off")

    state_payload = next(payload for topic, payload in payloads if topic == TOPIC_STATE)
    assert state_payload["pricing_debug"]["skew"]["inventory_qty_base"] == "33317.3519"
    assert state_payload["pricing_debug"]["skew"]["global_inventory_qty"] == "33317.3519"
    assert state_payload["pricing_debug"]["skew"]["global_inventory_qty_base"] == "33317.3519"
    assert (
        state_payload["pricing_debug"]["skew"]["global_inventory_source"]
        == "portfolio_component_sum"
    )
    assert state_payload["pricing_debug"]["skew"]["local_position_qty_base"] == "-98060"
    assert state_payload["pricing_debug"]["skew"]["local_position_qty_venue"] == "-9806"
    assert (
        state_payload["pricing_debug"]["skew"]["local_position_qty_conversion_status"]
        == "exact_multiplier"
    )
    assert state_payload["pricing_debug"]["skew"]["local_inventory_qty"] == "-9806"
    assert state_payload["pricing_debug"]["skew"]["local_inventory_qty_base"] == "-98060"
    assert state_payload["pricing_debug"]["skew"]["local_inventory_source"] == "positions"


def test_publish_state_refreshes_skew_while_preserving_cached_pricing_debug(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_000])
    strategy._managed_orders = list
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._last_pricing_debug = {
        "pricing": {
            "place_bid": "100.00",
        },
        "skew": {
            "global_inventory_qty": "33317.3519",
            "global_inventory_source": "portfolio_component_partial_sum",
        },
    }
    strategy._quote_runtime_params_snapshot = lambda: {"max_qty_global": Decimal(50000)}

    calls = {"count": 0}

    def _compute_inventory_skew(**_kwargs: Any) -> dict[str, Any]:
        calls["count"] += 1
        return {
            "inventory_qty": Decimal("1"),
            "inventory_source": "positions",
            "base_currency": "PLUME",
            "position_qty": Decimal("1"),
            "spot_qty": Decimal("0"),
            "global_position_qty": Decimal("1"),
            "global_spot_qty": Decimal("0"),
            "global_inventory_qty": Decimal("1"),
            "global_inventory_source": "positions",
            "local_position_qty": Decimal("1"),
            "local_spot_qty": Decimal("0"),
            "local_inventory_qty": Decimal("1"),
            "local_inventory_source": "positions",
            "des_qty_global": Decimal("0"),
            "max_qty_global": Decimal("50000"),
            "max_skew_bps_global": Decimal("20"),
            "des_qty_local": Decimal("0"),
            "max_qty_local": Decimal("100000"),
            "max_skew_bps_local": Decimal("0"),
            "linear_offset_bps": Decimal("0"),
            "global_ratio": Decimal("0.00002"),
            "global_skew_bps": Decimal("0.0004"),
            "local_ratio": Decimal("0.00001"),
            "local_skew_bps": Decimal("0"),
            "total_skew_bps": Decimal("0.0004"),
        }

    strategy._compute_inventory_skew = _compute_inventory_skew

    payloads: list[tuple[str, dict[str, Any]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._publish_state("running")

    state_payload = next(payload for topic, payload in payloads if topic == TOPIC_STATE)
    assert state_payload["pricing_debug"]["pricing"]["place_bid"] == "100.00"
    assert state_payload["pricing_debug"]["skew"]["global_inventory_qty"] == "1"
    assert state_payload["pricing_debug"]["skew"]["global_inventory_source"] == "positions"
    assert calls["count"] == 1
