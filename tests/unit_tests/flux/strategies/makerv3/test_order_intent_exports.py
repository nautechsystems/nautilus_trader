from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace
from typing import Any

import pytest

from nautilus_trader.flux.strategies.makerv3 import quote_engine as quote_engine_mod
from nautilus_trader.model.enums import OrderSide


TOPIC_ORDER_INTENT = "flux.makerv3.order_intent"


def _collect_topic_payloads(
    payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]],
    topic: str,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for payload_topic, payload in payloads:
        if payload_topic != topic:
            continue
        if isinstance(payload, list):
            rows.extend(payload)
        else:
            rows.append(payload)
    return rows


def _collect_order_intents(
    payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]],
    *,
    intent_type: str | None = None,
) -> list[dict[str, Any]]:
    rows = _collect_topic_payloads(payloads, TOPIC_ORDER_INTENT)
    if intent_type is None:
        return rows
    return [row for row in rows if row.get("intent_type") == intent_type]


def _make_refresh_strategy(clocked_strategy_factory, monkeypatch):
    strategy = clocked_strategy_factory(
        [
            1_000_000_000,
            1_000_000_100,
            1_000_000_200,
            1_000_000_300,
            1_000_000_400,
        ],
    )
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._strategy_identity = "runtime_strategy_id"
    strategy._external_strategy_id = "external_strategy_id"
    strategy._publish_state = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_actionable_alert = lambda *_args, **_kwargs: None
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 990_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 990_000_000
    strategy._register_managed_order = lambda _order: None

    order_seq = {"value": 0}

    def _limit(**kwargs: Any) -> SimpleNamespace:
        order_seq["value"] += 1
        return SimpleNamespace(
            client_order_id=f"CLIENT-{order_seq['value']}",
            price=kwargs["price"],
            side=kwargs["order_side"],
            quantity=kwargs["quantity"],
            ts_init=0,
        )

    monkeypatch.setattr(
        type(strategy),
        "order_factory",
        property(lambda _self: SimpleNamespace(limit=_limit)),
        raising=False,
    )
    strategy.submit_order = lambda _order, **_kwargs: None
    strategy.cancel_all_orders = lambda _instrument_id: None
    return strategy


def test_refresh_quotes_emits_place_order_intent_payloads_with_runtime_strategy_id_and_context(
    clocked_strategy_factory,
    monkeypatch,
) -> None:
    strategy = _make_refresh_strategy(clocked_strategy_factory, monkeypatch)
    strategy._managed_orders = list

    payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    quote_cycle = strategy._quote_cycle_context_from_id(
        now_ns=1_000_000_000,
        quote_cycle_id="RUN-42:7",
        trigger_source="timer_guard",
        trigger_instrument_id=strategy.config.maker_instrument_id,
        trigger_md_ts_event_ns=111_111_111,
        trigger_md_ts_init_ns=222_222_222,
    )
    strategy._refresh_quotes(
        now_ns=1_000_000_000,
        quote_cycle_id=quote_cycle.quote_cycle_id,
        quote_cycle=quote_cycle,
    )

    place_payloads = _collect_order_intents(payloads, intent_type="PLACE")
    assert place_payloads
    place_payload = place_payloads[0]
    assert place_payload["strategy_id"] == strategy.runtime_strategy_id
    assert place_payload["external_strategy_id"] == strategy._external_strategy_id
    assert place_payload["run_id"]
    assert place_payload["quote_cycle_id"] == "RUN-42:7"
    assert place_payload["reason_code"] == "place_missing_hole_repair"
    assert place_payload["level_index"] == 0
    assert place_payload["target_px"]
    assert place_payload["cancel_px"]
    assert place_payload["match_tol"]
    assert place_payload["ts_market_data_event_ns"] == 111_111_111
    assert place_payload["ts_market_data_recv_ns"] == 222_222_222
    assert place_payload["ts_decision_ns"] == 1_000_000_000
    assert place_payload["ts_submit_local_ns"] >= place_payload["ts_decision_ns"]
    assert place_payload["decision_context_json"] is None


def test_refresh_quotes_emits_distinct_blocked_cancel_intents_with_runtime_strategy_id(
    clocked_strategy_factory,
    monkeypatch,
) -> None:
    unavailable_strategy = _make_refresh_strategy(clocked_strategy_factory, monkeypatch)
    unavailable_strategy._managed_orders = lambda: [
        SimpleNamespace(
            client_order_id="RESTING-BOOK-1",
            side=OrderSide.BUY,
            price=Decimal("100"),
            quantity=Decimal("1"),
            ts_init=1,
        ),
    ]
    unavailable_strategy.cancel_order = lambda _order: None
    unavailable_strategy._best_bid_ask = lambda instrument_id: (
        None
        if instrument_id == unavailable_strategy.config.maker_instrument_id
        else (Decimal(100), Decimal(101))
    )

    stale_strategy = _make_refresh_strategy(clocked_strategy_factory, monkeypatch)
    stale_strategy._managed_orders = lambda: [
        SimpleNamespace(
            client_order_id="RESTING-STALE-1",
            side=OrderSide.BUY,
            price=Decimal("100"),
            quantity=Decimal("1"),
            ts_init=1,
        ),
    ]
    stale_strategy.cancel_order = lambda _order: None
    stale_strategy._last_bbo_ts_ns[stale_strategy.config.maker_instrument_id] = 700_000_000

    unavailable_payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]] = []
    stale_payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]] = []
    unavailable_strategy._publish_json = lambda topic, payload: unavailable_payloads.append(
        (topic, payload),
    )
    stale_strategy._publish_json = lambda topic, payload: stale_payloads.append((topic, payload))

    unavailable_strategy._refresh_quotes(now_ns=1_000_000_000, quote_cycle_id="RUN-42:8")
    stale_strategy._refresh_quotes(now_ns=1_000_000_000, quote_cycle_id="RUN-42:9")

    unavailable_intents = _collect_order_intents(unavailable_payloads, intent_type="CANCEL")
    stale_intents = _collect_order_intents(stale_payloads, intent_type="CANCEL")

    assert unavailable_intents
    assert stale_intents
    assert unavailable_intents[0]["strategy_id"] == unavailable_strategy.runtime_strategy_id
    assert unavailable_intents[0]["external_strategy_id"] == unavailable_strategy._external_strategy_id
    assert unavailable_intents[0]["reason_code"] == "cancel_maker_book_unavailable"
    assert unavailable_intents[0]["quote_cycle_id"] == "RUN-42:8"
    assert unavailable_intents[0]["ts_cancel_request_local_ns"] == 1_000_000_000
    assert unavailable_intents[0]["decision_context_json"] is None
    assert stale_intents[0]["strategy_id"] == stale_strategy.runtime_strategy_id
    assert stale_intents[0]["external_strategy_id"] == stale_strategy._external_strategy_id
    assert stale_intents[0]["reason_code"] == "cancel_maker_md_stale"
    assert stale_intents[0]["quote_cycle_id"] == "RUN-42:9"
    assert stale_intents[0]["ts_cancel_request_local_ns"] == 1_000_000_000
    assert stale_intents[0]["decision_context_json"] is None


def test_enforce_stale_market_data_emits_book_unavailable_cancel_reason_taxonomy(
    clocked_strategy_factory,
    monkeypatch,
) -> None:
    strategy = _make_refresh_strategy(clocked_strategy_factory, monkeypatch)
    strategy._managed_orders = lambda: [
        SimpleNamespace(
            client_order_id="RESTING-BOOK-1",
            side=OrderSide.BUY,
            price=Decimal("100"),
            quantity=Decimal("1"),
            ts_init=1,
        ),
    ]
    strategy._tracked_managed_order_count = lambda: 1
    strategy.cancel_order = lambda _order: None
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 0
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 990_000_000
    strategy._last_bbo_event_ts_ns[strategy.config.maker_instrument_id] = 333_333_333
    strategy._last_bbo_init_ts_ns[strategy.config.maker_instrument_id] = 444_444_444

    payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._enforce_stale_market_data(now_ns=1_000_000_000)

    cancel_payloads = _collect_order_intents(payloads, intent_type="CANCEL")
    assert cancel_payloads
    cancel_payload = cancel_payloads[0]
    assert cancel_payload["strategy_id"] == strategy.runtime_strategy_id
    assert cancel_payload["external_strategy_id"] == strategy._external_strategy_id
    assert cancel_payload["reason_code"] == "cancel_maker_book_unavailable"
    assert cancel_payload["quote_cycle_id"]
    assert cancel_payload["ts_market_data_event_ns"] == 333_333_333
    assert cancel_payload["ts_market_data_recv_ns"] == 444_444_444
    assert cancel_payload["decision_context_json"] is None


@pytest.mark.parametrize(
    ("active_orders", "bid_levels", "expected_reason_code"),
    [
        (
            [
                SimpleNamespace(
                    client_order_id="BUY-1",
                    side=OrderSide.BUY,
                    price=Decimal("99.9"),
                    quantity=Decimal("1"),
                    ts_init=1_000_000_000,
                ),
                SimpleNamespace(
                    client_order_id="BUY-2",
                    side=OrderSide.BUY,
                    price=Decimal("99.8"),
                    quantity=Decimal("1"),
                    ts_init=1_000_000_000,
                ),
            ],
            [(Decimal("99.9"), Decimal("100.5"))],
            "cancel_back_excess",
        ),
        (
            [
                SimpleNamespace(
                    client_order_id="BUY-TOO-AGGRO",
                    side=OrderSide.BUY,
                    price=Decimal("100.0"),
                    quantity=Decimal("1"),
                    ts_init=1_000_000_000,
                ),
            ],
            [(Decimal("99.9"), Decimal("99.95"))],
            "cancel_front_violation",
        ),
        (
            [
                SimpleNamespace(
                    client_order_id="BUY-FREE-SLOT-1",
                    side=OrderSide.BUY,
                    price=Decimal("100.0"),
                    quantity=Decimal("1"),
                    ts_init=1_000_000_000,
                ),
                SimpleNamespace(
                    client_order_id="BUY-FREE-SLOT-2",
                    side=OrderSide.BUY,
                    price=Decimal("99.0"),
                    quantity=Decimal("1"),
                    ts_init=1_000_000_000,
                ),
            ],
            [(Decimal("101.0"), Decimal("101.0")), (Decimal("100.0"), Decimal("100.0"))],
            "cancel_back_excess",
        ),
    ],
)
def test_refresh_quotes_rebalance_cancel_intents_emit_structured_reason_taxonomy(
    clocked_strategy_factory,
    monkeypatch,
    active_orders,
    bid_levels,
    expected_reason_code: str,
) -> None:
    strategy = _make_refresh_strategy(clocked_strategy_factory, monkeypatch)
    strategy._managed_orders = lambda: list(active_orders)
    strategy.cancel_order = lambda _order: None

    monkeypatch.setattr(
        quote_engine_mod,
        "build_ladder_place_cancel_levels_from_bps",
        lambda **_kwargs: (bid_levels, []),
    )

    payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._refresh_quotes(now_ns=1_000_000_000, quote_cycle_id="RUN-42:10")

    cancel_payloads = _collect_order_intents(payloads, intent_type="CANCEL")
    assert cancel_payloads
    assert any(payload["reason_code"] == expected_reason_code for payload in cancel_payloads)


def test_refresh_quotes_emits_place_front_then_cancel_back_intents_with_deque_reason_codes(
    clocked_strategy_factory,
    monkeypatch,
) -> None:
    strategy = _make_refresh_strategy(clocked_strategy_factory, monkeypatch)
    strategy._runtime_params["max_cancels_per_side_per_cycle"] = 1
    strategy._runtime_params["max_places_per_side_per_cycle"] = 1
    strategy._runtime_params["max_total_actions_per_cycle"] = 2
    strategy._managed_orders = lambda: [
        SimpleNamespace(
            client_order_id="BUY-1",
            side=OrderSide.BUY,
            price=Decimal("99.8"),
            quantity=Decimal("1"),
            ts_init=1_000_000_000,
        ),
        SimpleNamespace(
            client_order_id="BUY-2",
            side=OrderSide.BUY,
            price=Decimal("99.7"),
            quantity=Decimal("1"),
            ts_init=1_000_000_000,
        ),
    ]
    strategy.cancel_order = lambda _order: None

    monkeypatch.setattr(
        quote_engine_mod,
        "build_ladder_place_cancel_levels_from_bps",
        lambda **_kwargs: (
            [
                (Decimal("99.9"), Decimal("99.95")),
                (Decimal("99.8"), Decimal("99.85")),
            ],
            [],
        ),
    )

    payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._refresh_quotes(now_ns=1_000_000_000, quote_cycle_id="RUN-42:11")

    place_payloads = _collect_order_intents(payloads, intent_type="PLACE")
    cancel_payloads = _collect_order_intents(payloads, intent_type="CANCEL")

    assert any(payload["reason_code"] == "place_front_improve" for payload in place_payloads)
    assert any(payload["reason_code"] == "cancel_back_excess" for payload in cancel_payloads)


def test_refresh_quotes_emits_cancel_front_then_place_back_intents_with_deque_reason_codes(
    clocked_strategy_factory,
    monkeypatch,
) -> None:
    strategy = _make_refresh_strategy(clocked_strategy_factory, monkeypatch)
    strategy._runtime_params["max_cancels_per_side_per_cycle"] = 1
    strategy._runtime_params["max_places_per_side_per_cycle"] = 1
    strategy._runtime_params["max_total_actions_per_cycle"] = 2
    strategy._managed_orders = lambda: [
        SimpleNamespace(
            client_order_id="BUY-1",
            side=OrderSide.BUY,
            price=Decimal("100.0"),
            quantity=Decimal("1"),
            ts_init=1_000_000_000,
        ),
        SimpleNamespace(
            client_order_id="BUY-2",
            side=OrderSide.BUY,
            price=Decimal("99.9"),
            quantity=Decimal("1"),
            ts_init=1_000_000_000,
        ),
    ]
    strategy.cancel_order = lambda _order: None

    monkeypatch.setattr(
        quote_engine_mod,
        "build_ladder_place_cancel_levels_from_bps",
        lambda **_kwargs: (
            [
                (Decimal("99.9"), Decimal("99.95")),
                (Decimal("99.8"), Decimal("99.85")),
            ],
            [],
        ),
    )

    payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._refresh_quotes(now_ns=1_000_000_000, quote_cycle_id="RUN-42:12")

    place_payloads = _collect_order_intents(payloads, intent_type="PLACE")
    cancel_payloads = _collect_order_intents(payloads, intent_type="CANCEL")

    assert any(payload["reason_code"] == "cancel_front_violation" for payload in cancel_payloads)
    assert any(payload["reason_code"] == "place_back_backfill" for payload in place_payloads)
