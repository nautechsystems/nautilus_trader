"""Plan deterministic quote-side rebalancing actions."""

from __future__ import annotations

from decimal import Decimal


def plan_side_rebalance_actions(
    *,
    side: str,
    active_prices: list[Decimal],
    active_stale: list[bool],
    desired_levels: list[tuple[Decimal, Decimal, Decimal]],
    stale_cancel_budget: int = 1,
) -> tuple[list[int], list[int]]:
    """Return active cancel indices and missing desired level indices."""
    side_norm = str(side).lower()
    if side_norm not in {"buy", "sell"}:
        raise ValueError(f"Unsupported side: {side!r}")
    if len(active_prices) != len(active_stale):
        raise ValueError("active_prices and active_stale length mismatch")

    max_levels = len(desired_levels)
    cancels: set[int] = set()

    for index in range(max_levels, len(active_prices)):
        cancels.add(index)

    for index in range(min(len(active_prices), max_levels)):
        if index in cancels:
            continue
        current_px = active_prices[index]
        _, cancel_px, _ = desired_levels[index]
        too_aggressive = (
            (side_norm == "buy" and current_px > cancel_px)
            or (side_norm == "sell" and current_px < cancel_px)
        )
        if too_aggressive:
            cancels.add(index)

    stale_budget = max(0, int(stale_cancel_budget))
    if stale_budget > 0:
        stale_candidates = [
            idx
            for idx, is_stale in enumerate(active_stale)
            if is_stale and idx not in cancels
        ]
        for idx in sorted(stale_candidates, reverse=True)[:stale_budget]:
            cancels.add(idx)

    def survivor_indices() -> list[int]:
        return [idx for idx in range(len(active_prices)) if idx not in cancels]

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
        if len(missing) <= free_slots:
            break
        if not survivors:
            break
        idx_to_cancel = survivors[-1]
        if idx_to_cancel in cancels:
            break
        cancels.add(idx_to_cancel)
        survivors = survivor_indices()
        missing = missing_level_indices(survivors)

    return sorted(cancels), missing


__all__ = ["plan_side_rebalance_actions"]
