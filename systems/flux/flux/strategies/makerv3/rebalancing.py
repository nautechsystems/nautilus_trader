"""
Plan deterministic quote-side rebalancing actions.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal

from flux.strategies.makerv3.constants import REASON_CANCEL_EXCESS_LEVEL
from flux.strategies.makerv3.constants import REASON_CANCEL_FREE_SLOT_FOR_MISSING_LEVEL
from flux.strategies.makerv3.constants import REASON_CANCEL_STALE_ORDER
from flux.strategies.makerv3.constants import REASON_CANCEL_TOO_AGGRESSIVE


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
    backlog_mode: str
    matched_level_count: int
    keep_level_count: int
    frontier_missing_level_count: int
    planned_stale_replacement_count: int
    total_missing_level_count: int
    excess_cancel_candidate_count: int
    aggressive_cancel_candidate_count: int
    stale_cancel_candidate_count: int
    room_cancel_candidate_count: int
    budget_limited: bool
    backlog_limited: bool


@dataclass(frozen=True, slots=True)
class BoundedConvergencePlan:
    cancel_actions: tuple[CancelAction, ...]
    place_level_indices: tuple[int, ...]
    diagnostics: ConvergenceDiagnostics


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
    Return a pure side-local bounded convergence plan with diagnostics.
    """
    side_norm = _validate_rebalance_inputs(
        side=side,
        active_prices=active_prices,
        active_stale=active_stale,
        desired_levels=desired_levels,
    )
    stale_budget = _normalize_non_negative_int(stale_cancel_budget, "stale_cancel_budget")
    reprice_budget = _normalize_non_negative_int(
        max_reprice_cancel_actions,
        "max_reprice_cancel_actions",
    )
    place_budget = _normalize_non_negative_int(max_place_actions, "max_place_actions")
    total_budget = _normalize_non_negative_int(max_total_actions, "max_total_actions")
    backlog_mode_norm = _normalize_backlog_mode(backlog_mode)

    alignment = _align_active_to_desired(
        side=side_norm,
        active_prices=active_prices,
        desired_levels=desired_levels,
    )

    cancel_actions: list[CancelAction] = []
    selected_cancel_indices: set[int] = set()

    def add_cancel(index: int, reason_code: str) -> bool:
        nonlocal reprice_budget, stale_budget, total_budget
        if index in selected_cancel_indices or total_budget <= 0:
            return False
        if reason_code == REASON_CANCEL_STALE_ORDER:
            if stale_budget <= 0:
                return False
            stale_budget -= 1
        else:
            if reprice_budget <= 0:
                return False
            reprice_budget -= 1
        total_budget -= 1
        selected_cancel_indices.add(index)
        cancel_actions.append(CancelAction(index=index, reason_code=reason_code))
        return True

    passive_tail_cancel_candidates = tuple(reversed(alignment.passive_tail_candidates))
    aggressive_cancel_candidates = alignment.aggressive_cancel_candidates

    for index in passive_tail_cancel_candidates:
        if not add_cancel(index, REASON_CANCEL_EXCESS_LEVEL):
            break

    if backlog_mode_norm == "normal":
        for index in aggressive_cancel_candidates:
            if not add_cancel(index, REASON_CANCEL_TOO_AGGRESSIVE):
                break

    stale_cancel_candidates = tuple(
        idx
        for idx in range(len(active_prices) - 1, -1, -1)
        if active_stale[idx] and idx not in selected_cancel_indices
    )
    for index in stale_cancel_candidates:
        if not add_cancel(index, REASON_CANCEL_STALE_ORDER):
            break

    room_cancel_candidates = tuple(
        idx
        for idx in range(len(active_prices) - 1, -1, -1)
        if idx not in selected_cancel_indices and alignment.matched_level_for_active[idx] is not None
    )

    room_created = 0
    if backlog_mode_norm == "normal":
        survivors_after_initial_cancels = len(active_prices) - len(selected_cancel_indices)
        free_slots = max(0, len(desired_levels) - survivors_after_initial_cancels)
        _frontier_place_target, room_needed = _max_frontier_places(
            frontier_missing_count=len(alignment.frontier_missing_levels),
            free_slots=free_slots,
            place_budget=place_budget,
            reprice_budget=reprice_budget,
            total_budget=total_budget,
        )

        for index in room_cancel_candidates:
            if room_created >= room_needed:
                break
            if add_cancel(index, REASON_CANCEL_FREE_SLOT_FOR_MISSING_LEVEL):
                room_created += 1
            else:
                break

    planned_stale_replacements = tuple(
        level_index
        for index in stale_cancel_candidates
        if index in selected_cancel_indices
        for level_index in (alignment.matched_level_for_active[index],)
        if level_index is not None
    )
    planned_room_replacements = tuple(
        level_index
        for index in room_cancel_candidates
        if index in selected_cancel_indices
        for level_index in (alignment.matched_level_for_active[index],)
        if level_index is not None
    )

    place_level_indices: list[int] = []
    if backlog_mode_norm == "normal":
        survivors_after_all_cancels = len(active_prices) - len(selected_cancel_indices)
        available_slots = max(0, len(desired_levels) - survivors_after_all_cancels)
        frontier_place_count = min(len(alignment.frontier_missing_levels), available_slots)
        frontier_place_levels = list(alignment.frontier_missing_levels[:frontier_place_count])
        replacement_place_levels = list(planned_room_replacements) + list(planned_stale_replacements)
        place_candidates = frontier_place_levels + replacement_place_levels

        remaining_place_budget = place_budget
        remaining_total_budget = total_budget
        for level_index in place_candidates:
            if remaining_place_budget <= 0 or remaining_total_budget <= 0:
                break
            place_level_indices.append(level_index)
            remaining_place_budget -= 1
            remaining_total_budget -= 1
    else:
        place_candidates = []

    matched_level_count = sum(
        1 for level_index in alignment.matched_level_for_active if level_index is not None
    )
    keep_level_count = matched_level_count + len(alignment.keep_candidates)

    selected_excess_cancel_count = sum(
        1
        for action in cancel_actions
        if action.reason_code == REASON_CANCEL_EXCESS_LEVEL
    )
    selected_aggressive_cancel_count = sum(
        1
        for action in cancel_actions
        if action.reason_code == REASON_CANCEL_TOO_AGGRESSIVE
    )
    selected_stale_cancel_count = sum(
        1
        for action in cancel_actions
        if action.reason_code == REASON_CANCEL_STALE_ORDER
    )
    total_missing_before_places = (
        len(alignment.frontier_missing_levels)
        + len(planned_room_replacements)
        + len(planned_stale_replacements)
    )
    budget_limited = (
        len(passive_tail_cancel_candidates) > selected_excess_cancel_count
        or len(stale_cancel_candidates) > selected_stale_cancel_count
    )
    if backlog_mode_norm == "normal":
        budget_limited = budget_limited or (
            len(aggressive_cancel_candidates) > selected_aggressive_cancel_count
            or total_missing_before_places > len(place_level_indices)
        )
    backlog_limited = backlog_mode_norm != "normal" and (
        bool(aggressive_cancel_candidates)
        or bool(alignment.frontier_missing_levels)
        or bool(planned_stale_replacements)
        or bool(planned_room_replacements)
    )

    diagnostics = ConvergenceDiagnostics(
        backlog_mode=backlog_mode_norm,
        matched_level_count=matched_level_count,
        keep_level_count=keep_level_count,
        frontier_missing_level_count=len(alignment.frontier_missing_levels),
        planned_stale_replacement_count=len(planned_stale_replacements),
        total_missing_level_count=max(0, total_missing_before_places - len(place_level_indices)),
        excess_cancel_candidate_count=len(passive_tail_cancel_candidates),
        aggressive_cancel_candidate_count=len(aggressive_cancel_candidates),
        stale_cancel_candidate_count=len(stale_cancel_candidates),
        room_cancel_candidate_count=len(room_cancel_candidates),
        budget_limited=budget_limited,
        backlog_limited=backlog_limited,
    )
    return BoundedConvergencePlan(
        cancel_actions=tuple(cancel_actions),
        place_level_indices=tuple(place_level_indices),
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

    cancel_reasons: dict[int, str] = {
        index: REASON_CANCEL_TOO_AGGRESSIVE
        for index in alignment.aggressive_cancel_candidates
    }

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
