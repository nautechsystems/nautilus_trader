from __future__ import annotations

import sys

from flux.strategies.shared.equities_arb.core import EquitiesArbFeeRules
from flux.strategies.shared.equities_arb.core import FeeAssumptions
from flux.strategies.shared.equities_arb.core import build_effective_ibkr_fee_bps
from flux.strategies.shared.equities_arb.core import build_fee_assumptions
from flux.strategies.shared.equities_arb.core import build_fee_aware_threshold_bps
from flux.strategies.shared.equities_arb.core import build_ibkr_ioc_limit
from flux.strategies.shared.equities_arb.core import build_maker_quote_price
from flux.strategies.shared.equities_arb.core import build_take_take_limit_price
from flux.strategies.shared.equities_arb.core import effective_venue_resolution_config
from flux.strategies.shared.equities_arb.core import resolve_fee_rules
from flux.strategies.shared.equities_arb.core import resolve_runtime_params_module
from flux.strategies.shared.equities_arb.core import runtime_params_module_for_strategy
from flux.strategies.shared.equities_arb.core import strategy_allowed_instrument_ids
from flux.strategies.shared.equities_arb.core import strategy_supports_immediate_hedge
from flux.strategies.shared.equities_arb.core import strategy_uses_profile_account_projection
from flux.strategies.shared.equities_arb.core import supports_immediate_hedge
from flux.strategies.shared.equities_arb.core import uses_profile_account_projection
from flux.strategies.shared.equities_arb.core import validate_ibkr_quote
from flux.strategies.shared.equities_arb.hedging import HedgeBacklogState
from flux.strategies.shared.equities_arb.hedging import HedgeOrderIntent
from flux.strategies.shared.equities_arb.hedging import PendingHedgeState
from flux.strategies.shared.equities_arb.hedging import build_hedge_backlog_payload
from flux.strategies.shared.equities_arb.hedging import build_hedge_policy_payload
from flux.strategies.shared.equities_arb.hedging import build_pending_hedge_payload
from flux.strategies.shared.equities_arb.instruments import hyperliquid_perp_to_ibkr_instrument_id
from flux.strategies.shared.equities_arb.instruments import translate_hyperliquid_fill_to_ibkr_shares
from flux.strategies.shared.equities_arb.observability import build_fee_assumptions_payload
from flux.strategies.shared.equities_arb.observability import build_quote_snapshot_payload
from flux.strategies.shared.equities_arb.reference_balances import (
    IbkrReferenceBalanceSnapshotProvider,
)
from flux.strategies.shared.equities_arb.reference_balances import (
    IbkrReferenceBalanceSnapshotProviderConfig,
)
from flux.strategies.shared.equities_arb.reference_balances import (
    get_cached_ibkr_reference_balance_provider,
)

_CURRENT_MODULE = sys.modules[__name__]

if __name__ == "flux.strategies.shared.equities_arb":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.equities_arb",
        _CURRENT_MODULE,
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.equities_arb":
    sys.modules.setdefault("flux.strategies.shared.equities_arb", _CURRENT_MODULE)


__all__ = [
    "EquitiesArbFeeRules",
    "FeeAssumptions",
    "HedgeBacklogState",
    "HedgeOrderIntent",
    "PendingHedgeState",
    "build_effective_ibkr_fee_bps",
    "build_fee_assumptions",
    "build_fee_assumptions_payload",
    "build_fee_aware_threshold_bps",
    "build_hedge_backlog_payload",
    "build_hedge_policy_payload",
    "build_ibkr_ioc_limit",
    "build_maker_quote_price",
    "build_pending_hedge_payload",
    "build_quote_snapshot_payload",
    "build_take_take_limit_price",
    "effective_venue_resolution_config",
    "hyperliquid_perp_to_ibkr_instrument_id",
    "IbkrReferenceBalanceSnapshotProvider",
    "IbkrReferenceBalanceSnapshotProviderConfig",
    "get_cached_ibkr_reference_balance_provider",
    "resolve_fee_rules",
    "resolve_runtime_params_module",
    "runtime_params_module_for_strategy",
    "strategy_allowed_instrument_ids",
    "strategy_supports_immediate_hedge",
    "strategy_uses_profile_account_projection",
    "supports_immediate_hedge",
    "translate_hyperliquid_fill_to_ibkr_shares",
    "uses_profile_account_projection",
    "validate_ibkr_quote",
]
