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


def _require_finite_decimal(value: Decimal, name: str) -> None:
    if not value.is_finite():
        raise ValueError(f"{name} must be finite")


@dataclass(frozen=True, slots=True)
class CancelAction:
    index: int
    reason_code: str


def plan_side_rebalance_details(  # noqa: C901
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
    side_norm = str(side).lower()
    if side_norm not in {"buy", "sell"}:
        raise ValueError(f"Unsupported side: {side!r}")
    if len(active_prices) != len(active_stale):
        raise ValueError("active_prices and active_stale length mismatch")

    for idx, price in enumerate(active_prices):
        _require_finite_decimal(price, f"active_prices[{idx}]")
    for idx, (target_px, cancel_px, match_tol) in enumerate(desired_levels):
        _require_finite_decimal(target_px, f"desired_levels[{idx}].target_px")
        _require_finite_decimal(cancel_px, f"desired_levels[{idx}].cancel_px")
        _require_finite_decimal(match_tol, f"desired_levels[{idx}].match_tol")

    max_levels = len(desired_levels)
    cancel_reasons: dict[int, str] = {}

    for index in range(max_levels, len(active_prices)):
        cancel_reasons[index] = REASON_CANCEL_EXCESS_LEVEL

    for index in range(min(len(active_prices), max_levels)):
        if index in cancel_reasons:
            continue
        current_px = active_prices[index]
        _, cancel_px, _ = desired_levels[index]
        too_aggressive = (side_norm == "buy" and current_px > cancel_px) or (
            side_norm == "sell" and current_px < cancel_px
        )
        if too_aggressive:
            cancel_reasons[index] = REASON_CANCEL_TOO_AGGRESSIVE

    stale_budget = max(0, int(stale_cancel_budget))
    if stale_budget > 0:
        stale_candidates = [
            idx
            for idx, is_stale in enumerate(active_stale)
            if is_stale and idx not in cancel_reasons
        ]
        for idx in sorted(stale_candidates, reverse=True)[:stale_budget]:
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


def plan_side_rebalance_actions(  # noqa: C901
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


__all__ = ["CancelAction", "plan_side_rebalance_actions", "plan_side_rebalance_details"]
