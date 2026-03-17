from __future__ import annotations

from nautilus_trader.flux.strategies.shared.ibkr_order_policy import build_ibkr_hedge_order_policy


def test_build_ibkr_hedge_order_policy_keeps_ioc_during_regular_session() -> None:
    policy = build_ibkr_hedge_order_policy(
        configured_route="SMART",
        outside_rth_enabled=True,
        is_regular_session=True,
        hedge_mode="maker_hedge",
    )

    assert policy.route == "SMART"
    assert policy.time_in_force == "IOC"
    assert policy.outside_rth is True
    assert policy.include_overnight is False
    assert policy.cancel_after_ms is None


def test_build_ibkr_hedge_order_policy_uses_day_and_cancel_budget_outside_regular_session() -> None:
    policy = build_ibkr_hedge_order_policy(
        configured_route="BLUEOCEAN",
        outside_rth_enabled=True,
        is_regular_session=False,
        hedge_mode="maker_hedge",
    )

    assert policy.route == "SMART"
    assert policy.time_in_force == "DAY"
    assert policy.outside_rth is True
    assert policy.include_overnight is True
    assert policy.cancel_after_ms == 5_000


def test_build_ibkr_hedge_order_policy_accepts_take_take_mode_without_symbol_specific_inputs() -> None:
    policy = build_ibkr_hedge_order_policy(
        configured_route="SMART",
        outside_rth_enabled=False,
        is_regular_session=True,
        hedge_mode="take_take",
    )

    assert policy.route == "SMART"
    assert policy.time_in_force == "IOC"
    assert policy.include_overnight is False
