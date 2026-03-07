import sys

from .strategy_set import EQUITIES_DESCRIPTOR
from .strategy_set import TOKENMM_DESCRIPTOR
from .strategy_set import StrategySetDescriptor
from .strategy_set import build_profile_strategy_maps
from .strategy_set import build_profile_summary
from .strategy_set import get_strategy_set_descriptor
from .strategy_set import get_strategy_set_descriptors
from .strategy_set import normalize_profile
from .strategy_set import supported_profile_ids
from .bootstrap import strategy_startup_lock
from .portfolio_runner import parse_required_strategy_ids
from .portfolio_runner import parse_strategy_ids

if __name__ == "flux.runners.shared":
    sys.modules.setdefault("nautilus_trader.flux.runners.shared", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.runners.shared":
    sys.modules.setdefault("flux.runners.shared", sys.modules[__name__])

__all__ = [
    "EQUITIES_DESCRIPTOR",
    "TOKENMM_DESCRIPTOR",
    "StrategySetDescriptor",
    "build_profile_strategy_maps",
    "build_profile_summary",
    "get_strategy_set_descriptor",
    "get_strategy_set_descriptors",
    "normalize_profile",
    "parse_required_strategy_ids",
    "parse_strategy_ids",
    "strategy_startup_lock",
    "supported_profile_ids",
]
