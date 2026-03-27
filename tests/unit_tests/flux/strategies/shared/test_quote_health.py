from __future__ import annotations

from decimal import Decimal

from nautilus_trader.flux.strategies.shared.quote_health import evaluate_quote_health


def test_evaluate_quote_health_marks_old_quote_as_old_not_feed_down() -> None:
    health = evaluate_quote_health(
        leg_role="maker",
        bid=Decimal("100"),
        ask=Decimal("101"),
        quote_age_ms=12_000,
        max_quote_age_ms=10_000,
        transport_connected=True,
        subscription_healthy=True,
    )

    assert health.feed_state == "ok"
    assert health.quote_state == "old"
    assert health.usable_for_pricing is False
    assert health.reason_code == "maker_quote_old"


def test_evaluate_quote_health_marks_missing_quote_without_transport_failure() -> None:
    health = evaluate_quote_health(
        leg_role="reference",
        bid=None,
        ask=None,
        quote_age_ms=None,
        max_quote_age_ms=1_000,
        transport_connected=True,
        subscription_healthy=True,
    )

    assert health.feed_state == "ok"
    assert health.quote_state == "missing"
    assert health.usable_for_pricing is False
    assert health.usable_for_hedging is False
    assert health.reason_code == "reference_quote_missing"


def test_evaluate_quote_health_marks_transport_disconnect_as_feed_down() -> None:
    health = evaluate_quote_health(
        leg_role="reference",
        bid=Decimal("100"),
        ask=Decimal("101"),
        quote_age_ms=25,
        max_quote_age_ms=1_000,
        transport_connected=False,
        subscription_healthy=False,
    )

    assert health.feed_state == "down"
    assert health.quote_state == "fresh"
    assert health.usable_for_pricing is False
    assert health.usable_for_hedging is False
    assert health.reason_code == "reference_feed_down"


def test_evaluate_quote_health_marks_fresh_quote_as_usable() -> None:
    health = evaluate_quote_health(
        leg_role="maker",
        bid=Decimal("100"),
        ask=Decimal("101"),
        quote_age_ms=250,
        max_quote_age_ms=1_000,
        transport_connected=True,
        subscription_healthy=True,
    )

    assert health.feed_state == "ok"
    assert health.quote_state == "fresh"
    assert health.usable_for_pricing is True
    assert health.usable_for_hedging is True
    assert health.reason_code is None


def test_evaluate_quote_health_treats_locked_quote_as_present() -> None:
    health = evaluate_quote_health(
        leg_role="reference",
        bid=Decimal("145.10"),
        ask=Decimal("145.10"),
        quote_age_ms=0,
        max_quote_age_ms=1_000,
        transport_connected=True,
        subscription_healthy=True,
    )

    assert health.feed_state == "ok"
    assert health.quote_state == "fresh"
    assert health.usable_for_pricing is True
    assert health.usable_for_hedging is True
    assert health.reason_code is None
