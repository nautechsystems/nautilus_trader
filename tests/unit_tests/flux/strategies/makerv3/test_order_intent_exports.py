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

    strategy._refresh_quotes(now_ns=1_000_000_000, quote_cycle_id="RUN-42:7")

    place_payloads = _collect_order_intents(payloads, intent_type="PLACE")
    assert place_payloads
    place_payload = place_payloads[0]
    assert place_payload["strategy_id"] == strategy.runtime_strategy_id
    assert place_payload["external_strategy_id"] == strategy._external_strategy_id
    assert place_payload["run_id"]
    assert place_payload["quote_cycle_id"] == "RUN-42:7"
    assert place_payload["reason_code"] == "place_missing_level"
    assert place_payload["level_index"] == 0
    assert place_payload["target_px"]
    assert place_payload["cancel_px"]
    assert place_payload["match_tol"]
    assert place_payload["ts_decision_ns"] == 1_000_000_000
    assert place_payload["ts_submit_local_ns"] >= place_payload["ts_decision_ns"]
    assert place_payload["decision_context_json"]["pricing"]["place_bid"] == place_payload["target_px"]


def test_refresh_quotes_emits_blocked_cancel_intents_with_runtime_strategy_id(
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
    strategy.cancel_order = lambda _order: None
    strategy._best_bid_ask = lambda instrument_id: (
        None
        if instrument_id == strategy.config.maker_instrument_id
        else (Decimal(100), Decimal(101))
    )

    payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._refresh_quotes(now_ns=1_000_000_000, quote_cycle_id="RUN-42:8")

    cancel_payloads = _collect_order_intents(payloads, intent_type="CANCEL")
    assert cancel_payloads
    cancel_payload = cancel_payloads[0]
    assert cancel_payload["strategy_id"] == strategy.runtime_strategy_id
    assert cancel_payload["external_strategy_id"] == strategy._external_strategy_id
    assert cancel_payload["reason_code"] == "cancel_maker_book_unavailable"
    assert cancel_payload["quote_cycle_id"] == "RUN-42:8"
    assert cancel_payload["ts_cancel_request_local_ns"] == 1_000_000_000


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

    payloads: list[tuple[str, dict[str, Any] | list[dict[str, Any]]]] = []
    strategy._publish_json = lambda topic, payload: payloads.append((topic, payload))

    strategy._enforce_stale_market_data(now_ns=1_000_000_000)

    cancel_payloads = _collect_order_intents(payloads, intent_type="CANCEL")
    assert cancel_payloads
    cancel_payload = cancel_payloads[0]
    assert cancel_payload["strategy_id"] == strategy.runtime_strategy_id
    assert cancel_payload["external_strategy_id"] == strategy._external_strategy_id
    assert cancel_payload["reason_code"] == "cancel_maker_book_unavailable"
    assert cancel_payload["decision_context_json"]["maker_quote_status"]["bid_open"] == 1


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
            "cancel_excess_level",
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
            "cancel_too_aggressive",
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
            "cancel_free_slot_for_missing_level",
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
