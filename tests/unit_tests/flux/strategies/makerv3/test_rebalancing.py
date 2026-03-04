from __future__ import annotations

from decimal import Decimal

import pytest

from nautilus_trader.flux.strategies.makerv3.rebalancing import plan_side_rebalance_actions


def test_plan_side_rebalance_actions_cancels_overflow_and_too_aggressive_orders() -> None:
    cancel_indices, missing_indices = plan_side_rebalance_actions(
        side="buy",
        active_prices=[Decimal("101"), Decimal("100"), Decimal("99"), Decimal("98")],
        active_stale=[False, False, False, False],
        desired_levels=[
            (Decimal("100"), Decimal("100.5"), Decimal("0")),
            (Decimal("99"), Decimal("99.5"), Decimal("0")),
        ],
    )

    assert cancel_indices == [0, 1, 2, 3]
    assert missing_indices == [0, 1]


def test_plan_side_rebalance_actions_uses_stale_cancel_budget_from_tail() -> None:
    cancel_indices, missing_indices = plan_side_rebalance_actions(
        side="sell",
        active_prices=[Decimal("10"), Decimal("11"), Decimal("12")],
        active_stale=[True, True, True],
        desired_levels=[
            (Decimal("10"), Decimal("9"), Decimal("0")),
            (Decimal("11"), Decimal("10"), Decimal("0")),
            (Decimal("12"), Decimal("11"), Decimal("0")),
        ],
        stale_cancel_budget=2,
    )

    assert cancel_indices == [1, 2]
    assert missing_indices == [1, 2]


def test_plan_side_rebalance_actions_frees_one_slot_for_more_aggressive_missing_level() -> None:
    cancel_indices, missing_indices = plan_side_rebalance_actions(
        side="buy",
        active_prices=[Decimal("100"), Decimal("99")],
        active_stale=[False, False],
        desired_levels=[
            (Decimal("101"), Decimal("101"), Decimal("0")),
            (Decimal("100"), Decimal("100"), Decimal("0")),
        ],
        stale_cancel_budget=0,
    )

    assert cancel_indices == [1]
    assert missing_indices == [0]


def test_plan_side_rebalance_actions_rejects_invalid_inputs() -> None:
    with pytest.raises(ValueError, match="Unsupported side"):
        plan_side_rebalance_actions(
            side="hold",
            active_prices=[],
            active_stale=[],
            desired_levels=[],
        )

    with pytest.raises(ValueError, match="length mismatch"):
        plan_side_rebalance_actions(
            side="buy",
            active_prices=[Decimal("1")],
            active_stale=[],
            desired_levels=[],
        )
