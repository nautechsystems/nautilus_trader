"""
Plan deterministic quote-side rebalancing actions.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from decimal import Decimal

from flux.strategies.makerv3.constants import REASON_CANCEL_BACK_EXCESS
from flux.strategies.makerv3.constants import REASON_CANCEL_EXCESS_LEVEL
from flux.strategies.makerv3.constants import REASON_CANCEL_FREE_SLOT_FOR_MISSING_LEVEL
from flux.strategies.makerv3.constants import REASON_CANCEL_FRONT_VIOLATION
from flux.strategies.makerv3.constants import REASON_CANCEL_STALE_ORDER
from flux.strategies.makerv3.constants import REASON_CANCEL_TOO_AGGRESSIVE
from flux.strategies.shared.quote_stack import ActiveStackLevel
from flux.strategies.shared.quote_stack import DesiredStackLevel
from flux.strategies.shared.quote_stack import StackAction
from flux.strategies.shared.quote_stack import StackActionMode
from flux.strategies.shared.quote_stack import StackPlan
from flux.strategies.shared.quote_stack import plan_side_deque_actions


_BACKLOG_MODES = frozenset({"normal", "soft_throttle", "hard_freeze", "blocked"})


def _require_finite_decimal(value: Decimal, name: str) -> None:
    if not value.is_finite():
        raise ValueError(f"{name} must be finite")


def _normalize_non_negative_int(value: int, name: str) -> int:
    normalized = int(value)
    if normalized < 0:
        raise ValueError(f"{name} must be >= 0")
    return normalized


def _normalize_side(side: str) -> str:
    side_norm = str(side).lower()
    if side_norm not in {"buy", "sell"}:
        raise ValueError(f"Unsupported side: {side!r}")
    return side_norm


def _normalize_backlog_mode(backlog_mode: str) -> str:
    mode = str(backlog_mode).lower()
    if mode not in _BACKLOG_MODES:
        raise ValueError(f"Unsupported backlog_mode: {backlog_mode!r}")
    return mode


def _is_more_aggressive(side: str, left: Decimal, right: Decimal) -> bool:
    return left > right if side == "buy" else left < right


def _is_too_aggressive(side: str, active_price: Decimal, cancel_price: Decimal) -> bool:
    return active_price > cancel_price if side == "buy" else active_price < cancel_price


@dataclass(frozen=True, slots=True)
class CancelAction:
    index: int
    reason_code: str


@dataclass(frozen=True, slots=True)
class ConvergenceDiagnostics:
    stack_action_mode: str
    backlog_mode: str
    matched_level_count: int
    keep_level_count: int
    missing_level_count: int
    frontier_missing_level_count: int
    interior_hole_count: int
    planned_stale_replacement_count: int
    total_missing_level_count: int
    excess_cancel_candidate_count: int
    aggressive_cancel_candidate_count: int
    stale_cancel_candidate_count: int
    room_cancel_candidate_count: int
    budget_limited: bool
    backlog_limited: bool
    depth_before: int
    depth_after: int
    temporary_oversize_depth: int
    front_changed: bool
    back_changed: bool


@dataclass(frozen=True, slots=True)
class BoundedConvergencePlan:
    cancel_actions: tuple[CancelAction, ...]
    place_level_indices: tuple[int, ...]
    diagnostics: ConvergenceDiagnostics


_CANCEL_ACTION_KINDS = frozenset({"cancel_back", "cancel_front", "cancel_repair"})
_PLACE_ACTION_KINDS = frozenset({"place_back", "place_front", "place_missing"})


def _reason_code_for_action(action: StackAction) -> str:
    if action.kind == "cancel_back":
        return REASON_CANCEL_BACK_EXCESS
    if action.kind == "cancel_front":
        return REASON_CANCEL_FRONT_VIOLATION
    if action.kind == "cancel_repair":
        return REASON_CANCEL_FREE_SLOT_FOR_MISSING_LEVEL
    raise ValueError(f"unsupported cancel action kind: {action.kind!r}")


def _mode_for_actions(actions: tuple[StackAction, ...]) -> StackActionMode:
    if not actions:
        return "no_op"
    kinds = tuple(action.kind for action in actions)
    if kinds == ("place_front", "cancel_back"):
        return "place_front_cancel_back"
    if kinds == ("cancel_front", "place_back"):
        return "cancel_front_place_back"
    if kinds == ("cancel_back",):
        return "cancel_back"
    if kinds == ("cancel_front",):
        return "cancel_front"
    if kinds == ("cancel_repair",):
        return "repair_hole"
    if kinds == ("place_missing",):
        return "place_missing"
    return "repair_hole"


def _bounded_depth_diagnostics(
    *,
    depth_before: int,
    actions: tuple[StackAction, ...],
    front_active_index: int | None = None,
    back_active_index: int | None = None,
) -> tuple[int, int, bool, bool]:
    current_depth = depth_before
    max_depth = depth_before
    front_changed = False
    back_changed = False

    for action in actions:
        if action.kind in _PLACE_ACTION_KINDS:
            current_depth += 1
        elif action.kind in _CANCEL_ACTION_KINDS:
            current_depth -= 1
        max_depth = max(max_depth, current_depth)

        if action.kind in {"cancel_front", "place_front"} or (
            action.kind == "cancel_repair" and action.active_index == front_active_index
        ) or (
            action.kind == "place_missing" and action.level_index == 0
        ):
            front_changed = True
        if action.kind in {"cancel_back", "place_back"} or (
            action.kind == "cancel_repair" and action.active_index == back_active_index
        ) or (
            action.kind == "place_missing" and action.level_index == depth_before
        ):
            back_changed = True

    return current_depth, max_depth, front_changed, back_changed


def _front_back_active_indexes(
    *,
    side: str,
    active_prices: Sequence[Decimal],
) -> tuple[int | None, int | None]:
    if not active_prices:
        return None, None
    ordered = sorted(
        enumerate(active_prices),
        key=lambda item: item[1],
        reverse=side == "buy",
    )
    return ordered[0][0], ordered[-1][0]


def _filtered_stack_plan(
    *,
    stack_plan: StackPlan,
    max_reprice_cancel_actions: int,
    max_place_actions: int,
    max_total_actions: int,
    backlog_mode: str,
) -> tuple[tuple[StackAction, ...], bool]:
    if backlog_mode != "normal":
        return (), False

    actions = tuple(stack_plan.actions)
    if not actions:
        return (), False

    remaining_cancel = max(0, int(max_reprice_cancel_actions))
    remaining_place = max(0, int(max_place_actions))
    remaining_total = max(0, int(max_total_actions))

    if stack_plan.diagnostics.stack_action_mode == "place_front_cancel_back":
        if remaining_total < 2 or remaining_place < 1 or remaining_cancel < 1:
            return (), True
        return actions, False

    allowed: list[StackAction] = []
    budget_limited = False
    for action in actions:
        if remaining_total <= 0:
            budget_limited = True
            break
        if action.kind in _CANCEL_ACTION_KINDS:
            if remaining_cancel <= 0:
                budget_limited = True
                break
            remaining_cancel -= 1
        else:
            if remaining_place <= 0:
                budget_limited = True
                break
            remaining_place -= 1
        remaining_total -= 1
        allowed.append(action)

    return tuple(allowed), budget_limited


@dataclass(frozen=True, slots=True)
class _AlignmentResult:
    matched_level_for_active: tuple[int | None, ...]
    active_for_matched_level: tuple[int | None, ...]
    aggressive_cancel_candidates: tuple[int, ...]
    passive_tail_candidates: tuple[int, ...]
    keep_candidates: tuple[int, ...]
    frontier_missing_levels: tuple[int, ...]


def _validate_rebalance_inputs(
    *,
    side: str,
    active_prices: list[Decimal],
    active_stale: list[bool],
    desired_levels: list[tuple[Decimal, Decimal, Decimal]],
) -> str:
    side_norm = _normalize_side(side)
    if len(active_prices) != len(active_stale):
        raise ValueError("active_prices and active_stale length mismatch")

    for idx, price in enumerate(active_prices):
        _require_finite_decimal(price, f"active_prices[{idx}]")
    for idx, (target_px, cancel_px, match_tol) in enumerate(desired_levels):
        _require_finite_decimal(target_px, f"desired_levels[{idx}].target_px")
        _require_finite_decimal(cancel_px, f"desired_levels[{idx}].cancel_px")
        _require_finite_decimal(match_tol, f"desired_levels[{idx}].match_tol")

    return side_norm


def _align_active_to_desired(
    *,
    side: str,
    active_prices: list[Decimal],
    desired_levels: list[tuple[Decimal, Decimal, Decimal]],
) -> _AlignmentResult:
    matched_level_for_active: list[int | None] = [None] * len(active_prices)
    active_for_matched_level: list[int | None] = [None] * len(desired_levels)
    aggressive_cancel_candidates: list[int] = []
    keep_candidates: list[int] = []
    frontier_missing_levels: list[int] = []

    active_index = 0
    desired_index = 0
    while active_index < len(active_prices) and desired_index < len(desired_levels):
        active_price = active_prices[active_index]
        target_price, cancel_price, match_tol = desired_levels[desired_index]

        if abs(active_price - target_price) <= match_tol:
            matched_level_for_active[active_index] = desired_index
            active_for_matched_level[desired_index] = active_index
            active_index += 1
            desired_index += 1
            continue

        if _is_more_aggressive(side, active_price, target_price):
            if _is_too_aggressive(side, active_price, cancel_price):
                aggressive_cancel_candidates.append(active_index)
            else:
                keep_candidates.append(active_index)
            active_index += 1
            continue

        frontier_missing_levels.append(desired_index)
        desired_index += 1

    while desired_index < len(desired_levels):
        frontier_missing_levels.append(desired_index)
        desired_index += 1

    passive_tail_candidates = list(range(active_index, len(active_prices)))

    return _AlignmentResult(
        matched_level_for_active=tuple(matched_level_for_active),
        active_for_matched_level=tuple(active_for_matched_level),
        aggressive_cancel_candidates=tuple(aggressive_cancel_candidates),
        passive_tail_candidates=tuple(passive_tail_candidates),
        keep_candidates=tuple(keep_candidates),
        frontier_missing_levels=tuple(frontier_missing_levels),
    )


def _missing_level_indices_for_active_prices(
    *,
    side: str,
    active_prices: list[Decimal],
    desired_levels: list[tuple[Decimal, Decimal, Decimal]],
) -> list[int]:
    alignment = _align_active_to_desired(
        side=side,
        active_prices=active_prices,
        desired_levels=desired_levels,
    )
    return list(alignment.frontier_missing_levels)


def _max_frontier_places(
    *,
    frontier_missing_count: int,
    free_slots: int,
    place_budget: int,
    reprice_budget: int,
    total_budget: int,
) -> tuple[int, int]:
    if frontier_missing_count <= 0 or place_budget <= 0 or total_budget <= 0:
        return 0, 0

    max_places = min(frontier_missing_count, place_budget, total_budget, free_slots + reprice_budget)
    if max_places > free_slots:
        max_places = min(max_places, (total_budget + free_slots) // 2)

    room_needed = max(0, max_places - free_slots)
    return max_places, room_needed


def _legacy_index_rebalance_details(
    *,
    side: str,
    active_prices: list[Decimal],
    active_stale: list[bool],
    desired_levels: list[tuple[Decimal, Decimal, Decimal]],
    stale_cancel_budget: int,
) -> tuple[list[CancelAction], list[int]]:
    max_levels = len(desired_levels)
    cancel_reasons: dict[int, str] = {}

    for index in range(max_levels, len(active_prices)):
        cancel_reasons[index] = REASON_CANCEL_EXCESS_LEVEL

    for index in range(min(len(active_prices), max_levels)):
        if index in cancel_reasons:
            continue
        current_px = active_prices[index]
        _, cancel_px, _ = desired_levels[index]
        if _is_too_aggressive(side, current_px, cancel_px):
            cancel_reasons[index] = REASON_CANCEL_TOO_AGGRESSIVE

    if stale_cancel_budget > 0:
        stale_candidates = [
            idx
            for idx, is_stale in enumerate(active_stale)
            if is_stale and idx not in cancel_reasons
        ]
        for idx in sorted(stale_candidates, reverse=True)[:stale_cancel_budget]:
            cancel_reasons[idx] = REASON_CANCEL_STALE_ORDER

    def survivor_indices() -> list[int]:
        return [idx for idx in range(len(active_prices)) if idx not in cancel_reasons]

    def missing_level_indices(survivors: list[int]) -> list[int]:
        survivor_prices = [active_prices[idx] for idx in survivors]
        missing: list[int] = []
        for level_idx, (target_px, _, match_tol) in enumerate(desired_levels):
            if not any(abs(px - target_px) <= match_tol for px in survivor_prices):
                missing.append(level_idx)
        return missing

    survivors = survivor_indices()
    missing = missing_level_indices(survivors)

    while True:
        free_slots = max(0, max_levels - len(survivors))
        if len(missing) <= free_slots or not survivors:
            break
        idx_to_cancel = survivors[-1]
        if idx_to_cancel in cancel_reasons:
            break
        cancel_reasons[idx_to_cancel] = REASON_CANCEL_FREE_SLOT_FOR_MISSING_LEVEL
        survivors = survivor_indices()
        missing = missing_level_indices(survivors)

    cancel_actions = [
        CancelAction(index=index, reason_code=cancel_reasons[index])
        for index in sorted(cancel_reasons)
    ]
    return cancel_actions, missing


def plan_side_bounded_convergence(
    *,
    side: str,
    active_prices: list[Decimal],
    active_stale: list[bool],
    desired_levels: list[tuple[Decimal, Decimal, Decimal]],
    stale_cancel_budget: int = 1,
    max_reprice_cancel_actions: int,
    max_place_actions: int,
    max_total_actions: int,
    backlog_mode: str,
) -> BoundedConvergencePlan:
    """
    Return a deque-style side-local plan with compatibility diagnostics.
    """
    side_norm = _validate_rebalance_inputs(
        side=side,
        active_prices=active_prices,
        active_stale=active_stale,
        desired_levels=desired_levels,
    )
    _normalize_non_negative_int(stale_cancel_budget, "stale_cancel_budget")
    reprice_budget = _normalize_non_negative_int(
        max_reprice_cancel_actions,
        "max_reprice_cancel_actions",
    )
    place_budget = _normalize_non_negative_int(max_place_actions, "max_place_actions")
    total_budget = _normalize_non_negative_int(max_total_actions, "max_total_actions")
    backlog_mode_norm = _normalize_backlog_mode(backlog_mode)
    stack_plan = plan_side_deque_actions(
        side=side_norm,
        active_prices=[
            ActiveStackLevel(active_index=index, price=price)
            for index, price in enumerate(active_prices)
        ],
        desired_levels=[
            DesiredStackLevel(
                level_index=index,
                place_price=target_px,
                cancel_price=cancel_px,
                match_tolerance=match_tol,
            )
            for index, (target_px, cancel_px, match_tol) in enumerate(desired_levels)
        ],
    )
    allowed_actions, budget_cut = _filtered_stack_plan(
        stack_plan=stack_plan,
        max_reprice_cancel_actions=reprice_budget,
        max_place_actions=place_budget,
        max_total_actions=total_budget,
        backlog_mode=backlog_mode_norm,
    )
    suppressed_keep_bucket = (
        stack_plan.diagnostics.stack_action_mode == "place_front_cancel_back"
        and active_prices
        and desired_levels
        and not _is_more_aggressive(side_norm, desired_levels[0][0], active_prices[0])
    )
    if suppressed_keep_bucket:
        allowed_actions = ()

    cancel_actions = tuple(
        CancelAction(
            index=int(action.active_index),
            reason_code=_reason_code_for_action(action),
        )
        for action in allowed_actions
        if action.kind in _CANCEL_ACTION_KINDS and action.active_index is not None
    )
    place_level_indices = tuple(
        int(action.level_index)
        for action in allowed_actions
        if action.kind in _PLACE_ACTION_KINDS and action.level_index is not None
    )
    planned_place_count = sum(1 for action in stack_plan.actions if action.kind in _PLACE_ACTION_KINDS)
    budget_limited = budget_cut or (
        backlog_mode_norm == "normal"
        and not suppressed_keep_bucket
        and planned_place_count > 0
        and stack_plan.diagnostics.missing_level_count > len(place_level_indices)
    )
    front_active_index, back_active_index = _front_back_active_indexes(
        side=side_norm,
        active_prices=active_prices,
    )
    depth_after, temporary_oversize_depth, front_changed, back_changed = _bounded_depth_diagnostics(
        depth_before=stack_plan.diagnostics.depth_before,
        actions=allowed_actions,
        front_active_index=front_active_index,
        back_active_index=back_active_index,
    )

    diagnostics = ConvergenceDiagnostics(
        stack_action_mode=_mode_for_actions(allowed_actions),
        backlog_mode=backlog_mode_norm,
        matched_level_count=max(
            0,
            len(active_prices) - stack_plan.diagnostics.missing_level_count - len(cancel_actions),
        ),
        keep_level_count=max(0, len(active_prices) - len(cancel_actions)),
        missing_level_count=stack_plan.diagnostics.missing_level_count,
        frontier_missing_level_count=max(
            0,
            stack_plan.diagnostics.missing_level_count - stack_plan.diagnostics.interior_hole_count,
        ),
        interior_hole_count=stack_plan.diagnostics.interior_hole_count,
        planned_stale_replacement_count=0,
        total_missing_level_count=max(
            0,
            stack_plan.diagnostics.missing_level_count - len(place_level_indices),
        ),
        excess_cancel_candidate_count=sum(
            1
            for action in allowed_actions
            if action.kind == "cancel_back"
        ),
        aggressive_cancel_candidate_count=sum(
            1
            for action in allowed_actions
            if action.kind == "cancel_front"
        ),
        stale_cancel_candidate_count=sum(1 for is_stale in active_stale if is_stale),
        room_cancel_candidate_count=0,
        budget_limited=budget_limited,
        backlog_limited=backlog_mode_norm != "normal",
        depth_before=stack_plan.diagnostics.depth_before,
        depth_after=depth_after,
        temporary_oversize_depth=temporary_oversize_depth,
        front_changed=front_changed,
        back_changed=back_changed,
    )
    return BoundedConvergencePlan(
        cancel_actions=cancel_actions,
        place_level_indices=place_level_indices,
        diagnostics=diagnostics,
    )


def plan_side_rebalance_details(
    *,
    side: str,
    active_prices: list[Decimal],
    active_stale: list[bool],
    desired_levels: list[tuple[Decimal, Decimal, Decimal]],
    stale_cancel_budget: int = 1,
) -> tuple[list[CancelAction], list[int]]:
    """
    Return structured active cancel actions and missing desired level indices.
    """
    side_norm = _validate_rebalance_inputs(
        side=side,
        active_prices=active_prices,
        active_stale=active_stale,
        desired_levels=desired_levels,
    )
    stale_budget = _normalize_non_negative_int(stale_cancel_budget, "stale_cancel_budget")

    if len(active_prices) > len(desired_levels):
        return _legacy_index_rebalance_details(
            side=side_norm,
            active_prices=active_prices,
            active_stale=active_stale,
            desired_levels=desired_levels,
            stale_cancel_budget=stale_budget,
        )

    alignment = _align_active_to_desired(
        side=side_norm,
        active_prices=active_prices,
        desired_levels=desired_levels,
    )

    cancel_reasons: dict[int, str] = dict.fromkeys(alignment.aggressive_cancel_candidates, REASON_CANCEL_TOO_AGGRESSIVE)

    if stale_budget > 0:
        stale_candidates = [
            idx
            for idx in range(len(active_prices) - 1, -1, -1)
            if active_stale[idx] and idx not in cancel_reasons
        ]
        for idx in stale_candidates[:stale_budget]:
            cancel_reasons[idx] = REASON_CANCEL_STALE_ORDER

    passive_tail_candidates = [
        idx
        for idx in reversed(alignment.passive_tail_candidates)
        if idx not in cancel_reasons
    ]

    while True:
        survivors = [
            idx
            for idx in range(len(active_prices))
            if idx not in cancel_reasons
        ]
        survivor_prices = [active_prices[idx] for idx in survivors]
        missing = _missing_level_indices_for_active_prices(
            side=side_norm,
            active_prices=survivor_prices,
            desired_levels=desired_levels,
        )
        free_slots = max(0, len(desired_levels) - len(survivors))
        if len(missing) <= free_slots or not survivors:
            break

        if passive_tail_candidates:
            cancel_reasons[passive_tail_candidates.pop(0)] = REASON_CANCEL_EXCESS_LEVEL
            continue

        cancel_reasons[survivors[-1]] = REASON_CANCEL_FREE_SLOT_FOR_MISSING_LEVEL

    cancel_actions = [
        CancelAction(index=index, reason_code=cancel_reasons[index])
        for index in sorted(cancel_reasons)
    ]
    return cancel_actions, missing


def plan_side_rebalance_actions(
    *,
    side: str,
    active_prices: list[Decimal],
    active_stale: list[bool],
    desired_levels: list[tuple[Decimal, Decimal, Decimal]],
    stale_cancel_budget: int = 1,
) -> tuple[list[int], list[int]]:
    """
    Return active cancel indices and missing desired level indices.
    """
    cancel_actions, missing = plan_side_rebalance_details(
        side=side,
        active_prices=active_prices,
        active_stale=active_stale,
        desired_levels=desired_levels,
        stale_cancel_budget=stale_cancel_budget,
    )
    return [action.index for action in cancel_actions], missing


__all__ = [
    "BoundedConvergencePlan",
    "CancelAction",
    "ConvergenceDiagnostics",
    "plan_side_bounded_convergence",
    "plan_side_rebalance_actions",
    "plan_side_rebalance_details",
]
