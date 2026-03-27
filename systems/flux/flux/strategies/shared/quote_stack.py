from __future__ import annotations

import sys
from collections.abc import Sequence
from dataclasses import dataclass
from decimal import Decimal
from typing import Literal


Side = Literal["buy", "sell"]
StackActionKind = Literal[
    "cancel_back",
    "cancel_front",
    "cancel_repair",
    "place_back",
    "place_front",
    "place_missing",
]
StackActionMode = Literal[
    "cancel_back",
    "cancel_front",
    "cancel_front_place_back",
    "no_op",
    "place_front_cancel_back",
    "place_missing",
    "repair_hole",
]


@dataclass(frozen=True, slots=True)
class ActiveStackLevel:
    active_index: int
    price: Decimal


@dataclass(frozen=True, slots=True)
class DesiredStackLevel:
    level_index: int
    place_price: Decimal
    cancel_price: Decimal
    match_tolerance: Decimal = Decimal(0)


@dataclass(frozen=True, slots=True)
class StackAction:
    kind: StackActionKind
    active_index: int | None = None
    level_index: int | None = None


@dataclass(frozen=True, slots=True)
class StackPlanDiagnostics:
    stack_action_mode: StackActionMode
    depth_before: int
    depth_after: int
    temporary_oversize_depth: int
    missing_level_count: int = 0
    interior_hole_count: int = 0
    front_changed: bool = False
    back_changed: bool = False


@dataclass(frozen=True, slots=True)
class StackPlan:
    actions: tuple[StackAction, ...]
    diagnostics: StackPlanDiagnostics


@dataclass(frozen=True, slots=True)
class _Alignment:
    missing_levels: tuple[DesiredStackLevel, ...]
    missing_positions: tuple[int, ...]
    unmatched_levels: tuple[ActiveStackLevel, ...]


def _normalize_side(side: str) -> Side:
    normalized = str(side).lower()
    if normalized not in {"buy", "sell"}:
        raise ValueError(f"unsupported quote stack side: {side!r}")
    return normalized


def _as_decimal(value: Decimal | float | str) -> Decimal:
    if isinstance(value, Decimal):
        return value
    return Decimal(str(value))


def _sort_best_to_worst(
    *,
    side: Side,
    levels: Sequence[ActiveStackLevel] | Sequence[DesiredStackLevel],
    price_getter,
) -> tuple[ActiveStackLevel, ...] | tuple[DesiredStackLevel, ...]:
    return tuple(
        sorted(
            levels,
            key=price_getter,
            reverse=side == "buy",
        ),
    )


def _normalize_active_levels(
    *,
    side: Side,
    active_prices: Sequence[Decimal | int | float | str | ActiveStackLevel],
) -> tuple[ActiveStackLevel, ...]:
    normalized = [
        ActiveStackLevel(
            active_index=int(price.active_index),
            price=_as_decimal(price.price),
        )
        if isinstance(price, ActiveStackLevel)
        else ActiveStackLevel(idx, _as_decimal(price))
        for idx, price in enumerate(active_prices)
    ]
    return _sort_best_to_worst(
        side=side,
        levels=normalized,
        price_getter=lambda level: level.price,
    )


def _normalize_desired_levels(
    *,
    side: Side,
    desired_levels: Sequence[
        DesiredStackLevel
        | tuple[
            Decimal | int | float | str,
            Decimal | int | float | str,
            Decimal | int | float | str,
        ]
    ],
) -> tuple[DesiredStackLevel, ...]:
    normalized: list[DesiredStackLevel] = []
    for idx, level in enumerate(desired_levels):
        if isinstance(level, DesiredStackLevel):
            normalized.append(
                DesiredStackLevel(
                    level_index=int(level.level_index),
                    place_price=_as_decimal(level.place_price),
                    cancel_price=_as_decimal(level.cancel_price),
                    match_tolerance=_as_decimal(level.match_tolerance),
                ),
            )
            continue

        place_price, cancel_price, match_tolerance = level
        normalized.append(
            DesiredStackLevel(
                level_index=idx,
                place_price=_as_decimal(place_price),
                cancel_price=_as_decimal(cancel_price),
                match_tolerance=_as_decimal(match_tolerance),
            ),
        )

    return _sort_best_to_worst(
        side=side,
        levels=normalized,
        price_getter=lambda level: level.place_price,
    )


def _matches(level: ActiveStackLevel, desired: DesiredStackLevel) -> bool:
    tolerance = max(desired.match_tolerance, Decimal(0))
    return abs(level.price - desired.place_price) <= tolerance


def _is_more_aggressive(
    side: Side,
    lhs: Decimal,
    rhs: Decimal,
    tolerance: Decimal = Decimal(0),
) -> bool:
    tolerance = max(tolerance, Decimal(0))
    if side == "buy":
        return lhs > rhs + tolerance
    return lhs < rhs - tolerance


def _has_front_cancel_violation(
    *,
    side: Side,
    active_levels: Sequence[ActiveStackLevel],
    desired_levels: Sequence[DesiredStackLevel],
) -> bool:
    if not active_levels or not desired_levels:
        return False

    active_front = active_levels[0]
    desired_front = desired_levels[0]
    return _is_more_aggressive(
        side,
        active_front.price,
        desired_front.cancel_price,
    )


def _align_levels(
    *,
    side: Side,
    active_levels: Sequence[ActiveStackLevel],
    desired_levels: Sequence[DesiredStackLevel],
) -> _Alignment:
    missing_levels: list[DesiredStackLevel] = []
    missing_positions: list[int] = []
    unmatched_levels: list[ActiveStackLevel] = []
    active_cursor = 0

    for desired_position, desired_level in enumerate(desired_levels):
        while active_cursor < len(active_levels):
            active_level = active_levels[active_cursor]
            if _matches(active_level, desired_level):
                break
            if not _is_more_aggressive(
                side,
                active_level.price,
                desired_level.place_price,
                desired_level.match_tolerance,
            ):
                break
            unmatched_levels.append(active_level)
            active_cursor += 1

        if active_cursor < len(active_levels) and _matches(active_levels[active_cursor], desired_level):
            active_cursor += 1
            continue

        missing_levels.append(desired_level)
        missing_positions.append(desired_position)

    unmatched_levels.extend(active_levels[active_cursor:])
    return _Alignment(
        missing_levels=tuple(missing_levels),
        missing_positions=tuple(missing_positions),
        unmatched_levels=tuple(unmatched_levels),
    )


def _interior_hole_count(
    *,
    desired_levels: Sequence[DesiredStackLevel],
    missing_positions: Sequence[int],
) -> int:
    if len(desired_levels) < 3 or not missing_positions:
        return 0
    missing_position_set = set(missing_positions)
    matched_positions = [
        position
        for position in range(len(desired_levels))
        if position not in missing_position_set
    ]
    if len(matched_positions) < 2:
        return 0
    leftmost_matched = matched_positions[0]
    rightmost_matched = matched_positions[-1]
    return sum(
        1
        for position in missing_positions
        if leftmost_matched < position < rightmost_matched
    )


def _is_simple_inward_move(
    *,
    side: Side,
    active_levels: Sequence[ActiveStackLevel],
    desired_levels: Sequence[DesiredStackLevel],
    interior_hole_count: int,
) -> bool:
    if len(active_levels) != len(desired_levels) or not active_levels or not desired_levels:
        return False
    if interior_hole_count > 0:
        return False
    desired_front = desired_levels[0]
    return _is_more_aggressive(
        side,
        desired_front.place_price,
        active_levels[0].price,
        desired_front.match_tolerance,
    )


def _should_place_back_after_front_cancel(
    *,
    desired_levels: Sequence[DesiredStackLevel],
    remainder_alignment: _Alignment,
    remainder_depth: int,
) -> bool:
    if not desired_levels or remainder_depth <= 0:
        return False
    target_depth = len(desired_levels)
    if remainder_depth != target_depth - 1:
        return False
    if remainder_alignment.unmatched_levels:
        return False
    tail_position = len(desired_levels) - 1
    return remainder_alignment.missing_positions == (tail_position,)


def _is_keep_bucket_widen(
    *,
    side: Side,
    active_levels: Sequence[ActiveStackLevel],
    alignment: _Alignment,
    interior_hole_count: int,
) -> bool:
    if not active_levels or not alignment.unmatched_levels:
        return False
    if interior_hole_count > 0:
        return False
    unmatched_count = len(alignment.unmatched_levels)
    unmatched_indexes = tuple(level.active_index for level in alignment.unmatched_levels[:unmatched_count])
    front_prefix = tuple(level.active_index for level in active_levels[:unmatched_count])
    if unmatched_indexes != front_prefix or len(alignment.missing_levels) != unmatched_count:
        return False
    return all(
        _is_more_aggressive(
            side,
            active_level.price,
            missing_level.place_price,
            missing_level.match_tolerance,
        )
        for active_level, missing_level in zip(alignment.unmatched_levels, alignment.missing_levels)
    )


def _build_plan(
    *,
    mode: StackActionMode,
    actions: Sequence[StackAction],
    depth_before: int,
    missing_level_count: int,
    interior_hole_count: int,
    front_active_index: int | None = None,
    back_active_index: int | None = None,
) -> StackPlan:
    current_depth = depth_before
    max_depth = depth_before

    for action in actions:
        if action.kind in {"place_back", "place_front", "place_missing"}:
            current_depth += 1
        elif action.kind in {"cancel_back", "cancel_front", "cancel_repair"}:
            current_depth -= 1
        max_depth = max(max_depth, current_depth)

    diagnostics = StackPlanDiagnostics(
        stack_action_mode=mode,
        depth_before=depth_before,
        depth_after=current_depth,
        temporary_oversize_depth=max_depth,
        missing_level_count=missing_level_count,
        interior_hole_count=interior_hole_count,
        front_changed=any(
            action.kind in {"cancel_front", "place_front"}
            or (action.kind == "cancel_repair" and action.active_index == front_active_index)
            or (action.kind == "place_missing" and action.level_index == 0)
            for action in actions
        ),
        back_changed=any(
            action.kind in {"cancel_back", "place_back"}
            or (action.kind == "cancel_repair" and action.active_index == back_active_index)
            or (action.kind == "place_missing" and action.level_index == depth_before)
            for action in actions
        ),
    )
    return StackPlan(actions=tuple(actions), diagnostics=diagnostics)


def plan_side_deque_actions(
    *,
    side: str,
    active_prices: Sequence[Decimal | int | float | str | ActiveStackLevel],
    desired_levels: Sequence[
        DesiredStackLevel
        | tuple[
            Decimal | int | float | str,
            Decimal | int | float | str,
            Decimal | int | float | str,
        ]
    ],
) -> StackPlan:
    normalized_side = _normalize_side(side)
    normalized_active_levels = _normalize_active_levels(
        side=normalized_side,
        active_prices=active_prices,
    )
    normalized_desired_levels = _normalize_desired_levels(
        side=normalized_side,
        desired_levels=desired_levels,
    )

    alignment = _align_levels(
        side=normalized_side,
        active_levels=normalized_active_levels,
        desired_levels=normalized_desired_levels,
    )
    depth_before = len(normalized_active_levels)
    target_depth = len(normalized_desired_levels)
    missing_level_count = len(alignment.missing_levels)
    interior_hole_count = _interior_hole_count(
        desired_levels=normalized_desired_levels,
        missing_positions=alignment.missing_positions,
    )
    frontier_missing_count = max(0, missing_level_count - interior_hole_count)
    front_active_index = normalized_active_levels[0].active_index if normalized_active_levels else None
    back_active_index = normalized_active_levels[-1].active_index if normalized_active_levels else None

    if _has_front_cancel_violation(
        side=normalized_side,
        active_levels=normalized_active_levels,
        desired_levels=normalized_desired_levels,
    ):
        active_front = normalized_active_levels[0]
        actions = [
            StackAction(
                kind="cancel_front",
                active_index=active_front.active_index,
            ),
        ]
        remainder_alignment = _align_levels(
            side=normalized_side,
            active_levels=normalized_active_levels[1:],
            desired_levels=normalized_desired_levels,
        )
        if _should_place_back_after_front_cancel(
            desired_levels=normalized_desired_levels,
            remainder_alignment=remainder_alignment,
            remainder_depth=len(normalized_active_levels) - 1,
        ):
            actions.append(
                StackAction(
                    kind="place_back",
                    level_index=remainder_alignment.missing_levels[-1].level_index,
                ),
            )
            return _build_plan(
                mode="cancel_front_place_back",
                actions=actions,
                depth_before=depth_before,
                missing_level_count=missing_level_count,
                interior_hole_count=interior_hole_count,
                front_active_index=front_active_index,
                back_active_index=back_active_index,
            )

        return _build_plan(
            mode="cancel_front",
            actions=actions,
            depth_before=depth_before,
            missing_level_count=missing_level_count,
            interior_hole_count=interior_hole_count,
            front_active_index=front_active_index,
            back_active_index=back_active_index,
        )

    if len(normalized_active_levels) > len(normalized_desired_levels):
        return _build_plan(
            mode="cancel_back",
            actions=[
                StackAction(
                    kind="cancel_back",
                    active_index=normalized_active_levels[-1].active_index,
                ),
            ],
            depth_before=depth_before,
            missing_level_count=missing_level_count,
            interior_hole_count=interior_hole_count,
            front_active_index=front_active_index,
            back_active_index=back_active_index,
        )

    if _is_simple_inward_move(
        side=normalized_side,
        active_levels=normalized_active_levels,
        desired_levels=normalized_desired_levels,
        interior_hole_count=interior_hole_count,
    ):
        return _build_plan(
            mode="place_front_cancel_back",
            actions=[
                StackAction(kind="place_front", level_index=normalized_desired_levels[0].level_index),
                StackAction(
                    kind="cancel_back",
                    active_index=normalized_active_levels[-1].active_index,
                ),
            ],
            depth_before=depth_before,
            missing_level_count=missing_level_count,
            interior_hole_count=interior_hole_count,
            front_active_index=front_active_index,
            back_active_index=back_active_index,
        )

    if alignment.missing_levels:
        if depth_before >= target_depth:
            if _is_keep_bucket_widen(
                side=normalized_side,
                active_levels=normalized_active_levels,
                alignment=alignment,
                interior_hole_count=interior_hole_count,
            ) or not alignment.unmatched_levels:
                return _build_plan(
                    mode="no_op",
                    actions=[],
                    depth_before=depth_before,
                    missing_level_count=missing_level_count,
                    interior_hole_count=interior_hole_count,
                    front_active_index=front_active_index,
                    back_active_index=back_active_index,
                )
            cancel_candidate = alignment.unmatched_levels[0]
            cancel_kind: StackActionKind = (
                "cancel_back"
                if cancel_candidate.active_index == normalized_active_levels[-1].active_index
                else "cancel_repair"
            )
            return _build_plan(
                mode="cancel_back" if cancel_kind == "cancel_back" else "repair_hole",
                actions=[
                    StackAction(
                        kind=cancel_kind,
                        active_index=cancel_candidate.active_index,
                    ),
                ],
                depth_before=depth_before,
                missing_level_count=missing_level_count,
                interior_hole_count=interior_hole_count,
                front_active_index=front_active_index,
                back_active_index=back_active_index,
            )
        mode: StackActionMode = (
            "repair_hole"
            if interior_hole_count > 0 and frontier_missing_count == 0
            else "place_missing"
        )
        return _build_plan(
            mode=mode,
            actions=[
                StackAction(
                    kind="place_missing",
                    level_index=alignment.missing_levels[0].level_index,
                ),
            ],
            depth_before=depth_before,
            missing_level_count=missing_level_count,
            interior_hole_count=interior_hole_count,
            front_active_index=front_active_index,
            back_active_index=back_active_index,
        )

    return _build_plan(
        mode="no_op",
        actions=[],
        depth_before=depth_before,
        missing_level_count=missing_level_count,
        interior_hole_count=interior_hole_count,
        front_active_index=front_active_index,
        back_active_index=back_active_index,
    )


def plan_quote_stack(
    *,
    side: str,
    active_prices: Sequence[Decimal | int | float | str | ActiveStackLevel],
    desired_levels: Sequence[
        DesiredStackLevel
        | tuple[
            Decimal | int | float | str,
            Decimal | int | float | str,
            Decimal | int | float | str,
        ]
    ],
) -> StackPlan:
    return plan_side_deque_actions(
        side=side,
        active_prices=active_prices,
        desired_levels=desired_levels,
    )


__all__ = [
    "ActiveStackLevel",
    "DesiredStackLevel",
    "StackAction",
    "StackPlan",
    "StackPlanDiagnostics",
    "plan_quote_stack",
    "plan_side_deque_actions",
]


if __name__ == "flux.strategies.shared.quote_stack":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.quote_stack",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.quote_stack":
    sys.modules.setdefault("flux.strategies.shared.quote_stack", sys.modules[__name__])
