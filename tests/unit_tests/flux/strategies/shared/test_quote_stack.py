from __future__ import annotations

from decimal import Decimal
import inspect


def _desired_levels(*prices: str) -> list[tuple[Decimal, Decimal, Decimal]]:
    return [
        (Decimal(price), Decimal(price), Decimal(0))
        for price in prices
    ]


def _quote_stack_module():
    from nautilus_trader.flux.strategies.shared import quote_stack as quote_stack_mod

    return quote_stack_mod


def _planner():
    quote_stack_mod = _quote_stack_module()
    for name in (
        "plan_side_deque_actions",
        "plan_quote_stack",
    ):
        planner = getattr(quote_stack_mod, name, None)
        if callable(planner):
            return planner
    raise AssertionError("shared quote stack planner surface missing")


def _plan(**kwargs):
    return _planner()(**kwargs)


def _action_tuples(actions) -> list[tuple[str, int | None, int | None]]:
    return [
        (
            str(action.kind),
            action.active_index,
            action.level_index,
        )
        for action in actions
    ]


def test_plan_quote_stack_returns_no_op_for_matching_stable_stack() -> None:
    result = _plan(
        side="buy",
        active_prices=[Decimal("100"), Decimal("99"), Decimal("98")],
        desired_levels=_desired_levels("100", "99", "98"),
    )

    assert result.diagnostics.stack_action_mode == "no_op"
    assert _action_tuples(result.actions) == []


def test_plan_quote_stack_normalizes_best_to_worst_order_before_planning() -> None:
    quote_stack_mod = _quote_stack_module()

    result = _plan(
        side="buy",
        active_prices=[Decimal("98"), Decimal("100"), Decimal("99")],
        desired_levels=[
            quote_stack_mod.DesiredStackLevel(2, Decimal("98"), Decimal("98"), Decimal(0)),
            quote_stack_mod.DesiredStackLevel(0, Decimal("100"), Decimal("100"), Decimal(0)),
            quote_stack_mod.DesiredStackLevel(1, Decimal("99"), Decimal("99"), Decimal(0)),
        ],
    )

    assert result.diagnostics.stack_action_mode == "no_op"
    assert _action_tuples(result.actions) == []


def test_plan_quote_stack_normalizes_dataclass_numeric_inputs_before_planning() -> None:
    quote_stack_mod = _quote_stack_module()

    result = _plan(
        side="buy",
        active_prices=[
            quote_stack_mod.ActiveStackLevel(0, "98"),
            quote_stack_mod.ActiveStackLevel(1, "100"),
            quote_stack_mod.ActiveStackLevel(2, "99"),
        ],
        desired_levels=[
            quote_stack_mod.DesiredStackLevel(2, "98", "98", "0"),
            quote_stack_mod.DesiredStackLevel(0, "100", "100", "0"),
            quote_stack_mod.DesiredStackLevel(1, "99", "99", "0"),
        ],
    )

    assert result.diagnostics.stack_action_mode == "no_op"
    assert _action_tuples(result.actions) == []


def test_plan_quote_stack_treats_exact_match_tolerance_boundary_as_a_match() -> None:
    quote_stack_mod = _quote_stack_module()

    result = _plan(
        side="buy",
        active_prices=[Decimal("100.004")],
        desired_levels=[
            quote_stack_mod.DesiredStackLevel(
                0,
                Decimal("100"),
                Decimal("100.010"),
                Decimal("0.004"),
            ),
        ],
    )

    assert result.diagnostics.stack_action_mode == "no_op"
    assert _action_tuples(result.actions) == []


def test_plan_quote_stack_represents_inward_move_as_place_front_then_cancel_back() -> None:
    result = _plan(
        side="buy",
        active_prices=[Decimal("100"), Decimal("99"), Decimal("98")],
        desired_levels=_desired_levels("101", "100", "99"),
    )

    assert result.diagnostics.stack_action_mode == "place_front_cancel_back"
    assert _action_tuples(result.actions) == [
        ("place_front", None, 0),
        ("cancel_back", 2, None),
    ]


def test_plan_quote_stack_represents_outward_move_as_cancel_front_then_place_back() -> None:
    result = _plan(
        side="buy",
        active_prices=[Decimal("100"), Decimal("99"), Decimal("98")],
        desired_levels=[
            (Decimal("99"), Decimal("99"), Decimal(0)),
            (Decimal("98"), Decimal("98"), Decimal(0)),
            (Decimal("97"), Decimal("97"), Decimal(0)),
        ],
    )

    assert result.diagnostics.stack_action_mode == "cancel_front_place_back"
    assert _action_tuples(result.actions) == [
        ("cancel_front", 0, None),
        ("place_back", None, 2),
    ]


def test_plan_quote_stack_does_not_backfill_outward_when_post_cancel_depth_is_not_short() -> None:
    result = _plan(
        side="buy",
        active_prices=[Decimal("101"), Decimal("100"), Decimal("99"), Decimal("97")],
        desired_levels=_desired_levels("100", "99", "98"),
    )

    assert result.diagnostics.stack_action_mode == "cancel_front"
    assert result.diagnostics.depth_after == 3
    assert _action_tuples(result.actions) == [
        ("cancel_front", 0, None),
    ]


def test_plan_quote_stack_cancels_back_when_depth_overflows_on_the_tail() -> None:
    result = _plan(
        side="buy",
        active_prices=[Decimal("100"), Decimal("99"), Decimal("98"), Decimal("97")],
        desired_levels=_desired_levels("100", "99", "98"),
    )

    assert result.diagnostics.stack_action_mode == "cancel_back"
    assert _action_tuples(result.actions) == [
        ("cancel_back", 3, None),
    ]


def test_plan_quote_stack_uses_cancel_price_for_front_violation_not_match_tolerance() -> None:
    quote_stack_mod = _quote_stack_module()

    result = _plan(
        side="buy",
        active_prices=[Decimal("100.007")],
        desired_levels=[
            quote_stack_mod.DesiredStackLevel(
                0,
                Decimal("100"),
                Decimal("100.005"),
                Decimal("0.004"),
            ),
        ],
    )

    assert result.diagnostics.stack_action_mode == "cancel_front"
    assert _action_tuples(result.actions) == [
        ("cancel_front", 0, None),
    ]


def test_plan_quote_stack_alias_has_explicit_matching_signature() -> None:
    quote_stack_mod = _quote_stack_module()

    assert inspect.signature(quote_stack_mod.plan_quote_stack) == inspect.signature(
        quote_stack_mod.plan_side_deque_actions,
    )


def test_plan_quote_stack_never_cancels_middle_levels_when_the_stack_is_otherwise_valid() -> None:
    result = _plan(
        side="buy",
        active_prices=[
            Decimal("105"),
            Decimal("104"),
            Decimal("103"),
            Decimal("102"),
            Decimal("101"),
        ],
        desired_levels=_desired_levels("106", "105", "104", "103", "102"),
    )

    assert _action_tuples(result.actions) == [
        ("place_front", None, 0),
        ("cancel_back", 4, None),
    ]


def test_plan_quote_stack_repairs_real_hole_without_canceling_another_level() -> None:
    result = _plan(
        side="buy",
        active_prices=[Decimal("100"), Decimal("98")],
        desired_levels=_desired_levels("100", "99", "98"),
    )

    assert result.diagnostics.stack_action_mode == "repair_hole"
    assert _action_tuples(result.actions) == [
        ("place_missing", None, 1),
    ]


def test_plan_quote_stack_treats_short_tail_underfill_as_place_missing_not_hole_repair() -> None:
    result = _plan(
        side="buy",
        active_prices=[Decimal("100")],
        desired_levels=_desired_levels("100", "99", "98"),
    )

    assert result.diagnostics.stack_action_mode == "place_missing"
    assert result.diagnostics.interior_hole_count == 0
    assert result.diagnostics.front_changed is False
    assert result.diagnostics.back_changed is True
    assert _action_tuples(result.actions) == [
        ("place_missing", None, 1),
    ]


def test_plan_quote_stack_treats_empty_stack_as_frontier_underfill_not_hole_repair() -> None:
    result = _plan(
        side="buy",
        active_prices=[],
        desired_levels=_desired_levels("100", "99", "98"),
    )

    assert result.diagnostics.stack_action_mode == "place_missing"
    assert result.diagnostics.interior_hole_count == 0
    assert result.diagnostics.front_changed is True
    assert result.diagnostics.back_changed is True
    assert _action_tuples(result.actions) == [
        ("place_missing", None, 0),
    ]


def test_plan_quote_stack_marks_front_changed_when_short_stack_is_missing_top_level() -> None:
    result = _plan(
        side="buy",
        active_prices=[Decimal("99"), Decimal("98")],
        desired_levels=_desired_levels("100", "99", "98"),
    )

    assert result.diagnostics.stack_action_mode == "place_missing"
    assert result.diagnostics.front_changed is True
    assert result.diagnostics.back_changed is False
    assert _action_tuples(result.actions) == [
        ("place_missing", None, 0),
    ]


def test_plan_quote_stack_cancels_tail_to_repair_full_depth_interior_gap() -> None:
    result = _plan(
        side="buy",
        active_prices=[Decimal("100"), Decimal("98"), Decimal("97")],
        desired_levels=_desired_levels("100", "99", "98"),
    )

    assert result.diagnostics.stack_action_mode == "cancel_back"
    assert result.diagnostics.interior_hole_count == 1
    assert result.diagnostics.depth_after == 2
    assert result.diagnostics.temporary_oversize_depth == 3
    assert result.diagnostics.front_changed is False
    assert result.diagnostics.back_changed is True
    assert _action_tuples(result.actions) == [
        ("cancel_back", 2, None),
    ]


def test_plan_quote_stack_represents_temporary_n_plus_one_explicitly_for_inward_moves() -> None:
    result = _plan(
        side="buy",
        active_prices=[Decimal("100"), Decimal("99"), Decimal("98")],
        desired_levels=_desired_levels("101", "100", "99"),
    )

    actions = _action_tuples(result.actions)

    assert actions[0][0] == "place_front"
    assert actions[1][0] == "cancel_back"
    assert result.diagnostics.depth_before == 3
    assert result.diagnostics.depth_after == 3
    assert result.diagnostics.temporary_oversize_depth == 4
