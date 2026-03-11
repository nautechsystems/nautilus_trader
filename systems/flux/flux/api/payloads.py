from __future__ import annotations

"""Public compatibility facade for Flux API payload helpers."""

import sys
from collections.abc import Mapping
from collections.abc import Sequence
from typing import Any

from ._payloads_balances import build_balances_rows
from ._payloads_balances import collapse_balance_display_rows
from ._payloads_balances import enrich_balances_rows
from ._payloads_balances import filter_balance_rows_for_contract_scope
from ._payloads_balances import merge_portfolio_balances_rows
from ._payloads_common import ContractCatalogEntry
from ._payloads_common import StrategyMetadata
from ._payloads_common import as_list
from ._payloads_common import build_alerts_rows
from ._payloads_common import build_envelope
from ._payloads_common import build_error
from ._payloads_common import build_params_payload
from ._payloads_common import build_trades_rows
from ._payloads_common import canonical_naming_fields
from ._payloads_common import coerce_ts_ms
from ._payloads_common import contract_id_for_leg
from ._payloads_common import decode_text
from ._payloads_common import enrich_row_with_canonical_naming
from ._payloads_common import extract_stream_rows
from ._payloads_common import load_json
from ._payloads_common import normalize_symbol_parts
from ._payloads_common import now_ms
from ._payloads_common import safe_bool
from ._payloads_common import safe_float
from ._payloads_common import safe_int
from ._payloads_common import select_latest_strategy_row
from ._payloads_common import strategy_id_from_row
from ._payloads_signals import build_legs_payload_impl
from ._payloads_signals import build_signals_payload_impl

if __name__ == "flux.api.payloads":
    sys.modules.setdefault("nautilus_trader.flux.api.payloads", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.api.payloads":
    sys.modules.setdefault("flux.api.payloads", sys.modules[__name__])


__all__ = [
    "ContractCatalogEntry",
    "StrategyMetadata",
    "as_list",
    "build_alerts_rows",
    "build_balances_rows",
    "build_envelope",
    "build_error",
    "build_legs_payload",
    "build_params_payload",
    "build_signals_payload",
    "build_trades_rows",
    "canonical_naming_fields",
    "coerce_ts_ms",
    "collapse_balance_display_rows",
    "contract_id_for_leg",
    "decode_text",
    "enrich_balances_rows",
    "enrich_row_with_canonical_naming",
    "extract_stream_rows",
    "filter_balance_rows_for_contract_scope",
    "load_json",
    "merge_portfolio_balances_rows",
    "normalize_symbol_parts",
    "now_ms",
    "safe_bool",
    "safe_float",
    "safe_int",
    "select_latest_strategy_row",
    "strategy_id_from_row",
]


def build_legs_payload(
    *,
    contracts: Sequence[ContractCatalogEntry],
    market_rows: Mapping[str, dict[str, Any]],
    now_ms_value: int | None = None,
) -> dict[str, Any]:
    """Build the normalized leg payload keyed by stable contract IDs."""

    current_ts_ms = now_ms() if now_ms_value is None else int(now_ms_value)
    return build_legs_payload_impl(
        contracts=contracts,
        market_rows=market_rows,
        current_ts_ms=current_ts_ms,
    )


def build_signals_payload(
    *,
    strategy_id: str,
    metadata: StrategyMetadata,
    state: dict[str, Any],
    fv_row: dict[str, Any],
    params: dict[str, Any],
    balances: list[dict[str, Any]],
    legs: dict[str, Any],
) -> dict[str, Any]:
    """Build the API signal payload for a strategy from state, params, balances, and legs."""

    return build_signals_payload_impl(
        strategy_id=strategy_id,
        metadata=metadata,
        state=state,
        fv_row=fv_row,
        params=params,
        balances=balances,
        legs=legs,
        now_ms_fn=now_ms,
    )
