from __future__ import annotations

from decimal import Decimal
from typing import Any

import pytest

from nautilus_trader.flux.strategies.makerv3 import rebalancing as rebalancing_mod
from nautilus_trader.flux.strategies.makerv3.constants import REASON_CANCEL_EXCESS_LEVEL
from nautilus_trader.flux.strategies.makerv3.constants import REASON_CANCEL_STALE_ORDER
from nautilus_trader.flux.strategies.makerv3.constants import REASON_CANCEL_TOO_AGGRESSIVE
from nautilus_trader.flux.strategies.makerv3.rebalancing import plan_side_rebalance_details
from nautilus_trader.flux.strategies.makerv3.rebalancing import plan_side_rebalance_actions


def _desired_levels(*prices: str) -> list[tuple[Decimal, Decimal, Decimal]]:
    return [
        (Decimal(price), Decimal(price), Decimal(0))
        for price in prices
    ]


def _bounded_side_plan(**kwargs: Any) -> Any:
    planner = getattr(rebalancing_mod, "plan_side_bounded_convergence", None)
    assert callable(planner), "bounded convergence planner surface missing"
    return planner(**kwargs)


def _result_field(result: Any, name: str) -> Any:
    if hasattr(result, name):
        return getattr(result, name)
    return result[name]


def _cancel_pairs(cancel_actions: list[Any]) -> list[tuple[int, str]]:
    return [
        (
            int(action.index) if hasattr(action, "index") else int(action["index"]),
            str(action.reason_code)
            if hasattr(action, "reason_code")
            else str(action["reason_code"]),
        )
        for action in cancel_actions
    ]


def test_plan_side_rebalance_actions_cancels_overflow_and_too_aggressive_orders() -> None:
    cancel_indices, missing_indices = plan_side_rebalance_actions(
        side="buy",
        active_prices=[Decimal(101), Decimal(100), Decimal(99), Decimal(98)],
        active_stale=[False, False, False, False],
        desired_levels=[
            (Decimal(100), Decimal("100.5"), Decimal(0)),
            (Decimal(99), Decimal("99.5"), Decimal(0)),
        ],
    )

    assert cancel_indices == [0, 1, 2, 3]
    assert missing_indices == [0, 1]


def test_plan_side_rebalance_actions_uses_stale_cancel_budget_from_tail() -> None:
    cancel_indices, missing_indices = plan_side_rebalance_actions(
        side="sell",
        active_prices=[Decimal(10), Decimal(11), Decimal(12)],
        active_stale=[True, True, True],
        desired_levels=[
            (Decimal(10), Decimal(9), Decimal(0)),
            (Decimal(11), Decimal(10), Decimal(0)),
            (Decimal(12), Decimal(11), Decimal(0)),
        ],
        stale_cancel_budget=2,
    )

    assert cancel_indices == [1, 2]
    assert missing_indices == [1, 2]


def test_plan_side_rebalance_actions_frees_one_slot_for_more_aggressive_missing_level() -> None:
    cancel_indices, missing_indices = plan_side_rebalance_actions(
        side="buy",
        active_prices=[Decimal(100), Decimal(99)],
        active_stale=[False, False],
        desired_levels=[
            (Decimal(101), Decimal(101), Decimal(0)),
            (Decimal(100), Decimal(100), Decimal(0)),
        ],
        stale_cancel_budget=0,
    )

    assert cancel_indices == [1]
    assert missing_indices == [0]


def test_plan_side_rebalance_actions_does_not_cancel_whole_side_on_one_step_widening() -> None:
    cancel_actions, missing_indices = plan_side_rebalance_details(
        side="buy",
        active_prices=[
            Decimal("105"),
            Decimal("104"),
            Decimal("103"),
            Decimal("102"),
            Decimal("101"),
        ],
        active_stale=[False, False, False, False, False],
        desired_levels=_desired_levels("104", "103", "102", "101", "100"),
        stale_cancel_budget=0,
    )

    assert _cancel_pairs(cancel_actions) == [(0, REASON_CANCEL_TOO_AGGRESSIVE)]
    assert missing_indices == [4]


def test_bounded_side_planner_peels_passive_tail_incrementally_for_single_missing_top_level() -> None:
    result = _bounded_side_plan(
        side="buy",
        active_prices=[
            Decimal("106"),
            Decimal("105"),
            Decimal("104"),
            Decimal("103"),
            Decimal("102"),
            Decimal("101"),
        ],
        active_stale=[False, False, False, False, False, False],
        desired_levels=_desired_levels("107", "106", "105", "104", "103", "102"),
        stale_cancel_budget=0,
        max_reprice_cancel_actions=1,
        max_place_actions=1,
        max_total_actions=2,
        backlog_mode="normal",
    )

    assert _cancel_pairs(_result_field(result, "cancel_actions")) == [
        (5, REASON_CANCEL_EXCESS_LEVEL),
    ]
    assert list(_result_field(result, "place_level_indices")) == [0]


def test_bounded_side_planner_spends_stale_cleanup_budget_outside_aggressive_reprice_budget() -> None:
    result = _bounded_side_plan(
        side="buy",
        active_prices=[
            Decimal("105"),
            Decimal("104"),
            Decimal("103"),
            Decimal("102"),
            Decimal("101"),
        ],
        active_stale=[False, False, False, False, True],
        desired_levels=_desired_levels("104", "103", "102", "101", "100"),
        stale_cancel_budget=1,
        max_reprice_cancel_actions=1,
        max_place_actions=1,
        max_total_actions=3,
        backlog_mode="normal",
    )

    cancel_actions = _cancel_pairs(_result_field(result, "cancel_actions"))
    aggressive_reprice_cancels = [
        cancel_action
        for cancel_action in cancel_actions
        if cancel_action[1] == REASON_CANCEL_TOO_AGGRESSIVE
    ]
    stale_cleanup_cancels = [
        cancel_action
        for cancel_action in cancel_actions
        if cancel_action[1] == REASON_CANCEL_STALE_ORDER
    ]

    assert aggressive_reprice_cancels == [(0, REASON_CANCEL_TOO_AGGRESSIVE)]
    assert stale_cleanup_cancels == [(4, REASON_CANCEL_STALE_ORDER)]
    assert len(cancel_actions) == 2
    assert list(_result_field(result, "place_level_indices")) == [3]


def test_bounded_side_planner_reports_frontier_and_stale_replacement_diagnostics() -> None:
    result = _bounded_side_plan(
        side="buy",
        active_prices=[
            Decimal("105"),
            Decimal("104"),
            Decimal("103"),
            Decimal("102"),
            Decimal("101"),
        ],
        active_stale=[False, False, False, False, True],
        desired_levels=_desired_levels("104", "103", "102", "101", "100"),
        stale_cancel_budget=1,
        max_reprice_cancel_actions=1,
        max_place_actions=1,
        max_total_actions=3,
        backlog_mode="normal",
    )

    diagnostics = _result_field(result, "diagnostics")

    assert _result_field(diagnostics, "frontier_missing_level_count") == 1
    assert _result_field(diagnostics, "planned_stale_replacement_count") == 1
    assert _result_field(diagnostics, "total_missing_level_count") == 1
    assert _result_field(diagnostics, "backlog_mode") == "normal"


def test_bounded_side_planner_never_returns_duplicate_cancel_actions() -> None:
    result = _bounded_side_plan(
        side="buy",
        active_prices=[
            Decimal("106"),
            Decimal("105"),
            Decimal("104"),
            Decimal("103"),
            Decimal("102"),
            Decimal("101"),
        ],
        active_stale=[False, False, False, True, False, False],
        desired_levels=_desired_levels("107", "106", "105", "104", "103", "102"),
        stale_cancel_budget=1,
        max_reprice_cancel_actions=3,
        max_place_actions=2,
        max_total_actions=4,
        backlog_mode="normal",
    )

    cancel_indices = [
        index
        for index, _reason in _cancel_pairs(_result_field(result, "cancel_actions"))
    ]
    assert cancel_indices == list(dict.fromkeys(cancel_indices))


def test_bounded_side_planner_respects_cancel_place_and_total_budgets() -> None:
    result = _bounded_side_plan(
        side="buy",
        active_prices=[
            Decimal("106"),
            Decimal("105"),
            Decimal("104"),
            Decimal("103"),
            Decimal("102"),
            Decimal("101"),
        ],
        active_stale=[False, False, False, True, True, True],
        desired_levels=_desired_levels("107", "106", "105", "104", "103", "102"),
        stale_cancel_budget=2,
        max_reprice_cancel_actions=2,
        max_place_actions=1,
        max_total_actions=2,
        backlog_mode="normal",
    )

    cancel_actions = list(_result_field(result, "cancel_actions"))
    place_level_indices = list(_result_field(result, "place_level_indices"))

    assert len(cancel_actions) <= 2
    assert len(place_level_indices) <= 1
    assert len(cancel_actions) + len(place_level_indices) <= 2


def test_bounded_side_planner_caps_places_to_available_slots_when_frontier_and_replacements_compete() -> None:
    result = _bounded_side_plan(
        side="sell",
        active_prices=[
            Decimal("97"),
            Decimal("98"),
            Decimal("99"),
            Decimal("100"),
            Decimal("101"),
        ],
        active_stale=[False, False, False, True, False],
        desired_levels=_desired_levels("99", "100", "101", "102", "103"),
        stale_cancel_budget=1,
        max_reprice_cancel_actions=0,
        max_place_actions=3,
        max_total_actions=4,
        backlog_mode="normal",
    )

    place_level_indices = list(_result_field(result, "place_level_indices"))

    assert len(place_level_indices) == 1


def test_bounded_side_planner_prefers_passive_tail_before_more_aggressive_levels_for_capacity_only() -> None:
    result = _bounded_side_plan(
        side="buy",
        active_prices=[
            Decimal("106"),
            Decimal("105"),
            Decimal("104"),
            Decimal("103"),
            Decimal("102"),
            Decimal("101"),
        ],
        active_stale=[False, False, False, False, False, False],
        desired_levels=_desired_levels("107", "106", "105", "104", "103", "102"),
        stale_cancel_budget=0,
        max_reprice_cancel_actions=1,
        max_place_actions=1,
        max_total_actions=2,
        backlog_mode="normal",
    )

    assert _cancel_pairs(_result_field(result, "cancel_actions")) == [
        (5, REASON_CANCEL_EXCESS_LEVEL),
    ]


def test_bounded_side_planner_reports_zero_remaining_missing_after_planned_one_step_widening() -> None:
    result = _bounded_side_plan(
        side="buy",
        active_prices=[
            Decimal("105"),
            Decimal("104"),
            Decimal("103"),
            Decimal("102"),
            Decimal("101"),
        ],
        active_stale=[False, False, False, False, False],
        desired_levels=_desired_levels("104", "103", "102", "101", "100"),
        stale_cancel_budget=0,
        max_reprice_cancel_actions=1,
        max_place_actions=1,
        max_total_actions=2,
        backlog_mode="normal",
    )

    assert _cancel_pairs(_result_field(result, "cancel_actions")) == [
        (0, REASON_CANCEL_TOO_AGGRESSIVE),
    ]
    assert list(_result_field(result, "place_level_indices")) == [4]
    diagnostics = _result_field(result, "diagnostics")
    assert _result_field(diagnostics, "frontier_missing_level_count") == 1
    assert _result_field(diagnostics, "total_missing_level_count") == 0


def test_bounded_side_planner_places_more_aggressive_stale_replacement_before_frontier_gap() -> None:
    result = _bounded_side_plan(
        side="buy",
        active_prices=[Decimal("105")],
        active_stale=[True],
        desired_levels=_desired_levels("105", "104"),
        stale_cancel_budget=1,
        max_reprice_cancel_actions=0,
        max_place_actions=1,
        max_total_actions=2,
        backlog_mode="normal",
    )

    assert _cancel_pairs(_result_field(result, "cancel_actions")) == [
        (0, REASON_CANCEL_STALE_ORDER),
    ]
    assert list(_result_field(result, "place_level_indices")) == [0]


def test_bounded_side_planner_does_not_peel_keep_bucket_for_ordinary_widening_room() -> None:
    result = _bounded_side_plan(
        side="buy",
        active_prices=[Decimal("100")],
        active_stale=[False],
        desired_levels=[(Decimal("99"), Decimal("100.5"), Decimal("0"))],
        stale_cancel_budget=0,
        max_reprice_cancel_actions=1,
        max_place_actions=1,
        max_total_actions=2,
        backlog_mode="normal",
    )

    assert _cancel_pairs(_result_field(result, "cancel_actions")) == []
    assert list(_result_field(result, "place_level_indices")) == []
    diagnostics = _result_field(result, "diagnostics")
    assert _result_field(diagnostics, "keep_level_count") == 1
    assert _result_field(diagnostics, "budget_limited") is False
    assert _result_field(diagnostics, "total_missing_level_count") == 1


def test_bounded_side_planner_distinguishes_backlog_limiting_from_budget_limiting() -> None:
    result = _bounded_side_plan(
        side="buy",
        active_prices=[Decimal("105")],
        active_stale=[False],
        desired_levels=[(Decimal("104"), Decimal("104"), Decimal("0"))],
        stale_cancel_budget=0,
        max_reprice_cancel_actions=1,
        max_place_actions=1,
        max_total_actions=2,
        backlog_mode="soft_throttle",
    )

    assert _cancel_pairs(_result_field(result, "cancel_actions")) == []
    assert list(_result_field(result, "place_level_indices")) == []
    diagnostics = _result_field(result, "diagnostics")
    assert _result_field(diagnostics, "budget_limited") is False
    assert _result_field(diagnostics, "backlog_limited") is True


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
            active_prices=[Decimal(1)],
            active_stale=[],
            desired_levels=[],
        )


@pytest.mark.parametrize("bad", [Decimal("NaN"), Decimal("Infinity"), Decimal("-Infinity")])
def test_plan_side_rebalance_actions_rejects_non_finite_active_prices(bad: Decimal) -> None:
    with pytest.raises(ValueError, match="finite"):
        plan_side_rebalance_actions(
            side="buy",
            active_prices=[bad],
            active_stale=[False],
            desired_levels=[(Decimal(1), Decimal(2), Decimal(0))],
        )


@pytest.mark.parametrize("bad", [Decimal("NaN"), Decimal("Infinity"), Decimal("-Infinity")])
def test_plan_side_rebalance_actions_rejects_non_finite_desired_levels(bad: Decimal) -> None:
    with pytest.raises(ValueError, match="finite"):
        plan_side_rebalance_actions(
            side="buy",
            active_prices=[Decimal(1)],
            active_stale=[False],
            desired_levels=[(bad, Decimal(2), Decimal(0))],
        )
