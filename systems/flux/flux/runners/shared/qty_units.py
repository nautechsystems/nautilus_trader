from __future__ import annotations

import logging
from typing import Any

from flux.common.quantity_units import normalize_order_qty_unit


def resolve_runner_qty_unit(
    strategy_cfg: dict[str, Any],
    *,
    strategy_id: str,
    logger: logging.Logger,
) -> str:
    raw_qty_unit = strategy_cfg.get("qty_unit")
    if raw_qty_unit is None:
        logger.warning(
            "Strategy config qty_unit missing for strategy_id=%s; defaulting to 'venue'. Set qty_unit explicitly.",
            strategy_id,
        )
        return "venue"

    return normalize_order_qty_unit(raw_qty_unit, context=f"strategy_id={strategy_id}")


__all__ = ["resolve_runner_qty_unit"]
