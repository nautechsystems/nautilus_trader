from __future__ import annotations

from dataclasses import dataclass
import sys

if __name__ == "flux.strategies.shared.capabilities":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.capabilities",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.capabilities":
    sys.modules.setdefault("flux.strategies.shared.capabilities", sys.modules[__name__])


@dataclass(frozen=True, slots=True)
class FluxStrategyCapabilities:
    publishes_local_inventory: bool
    uses_profile_account_projection: bool
    supports_immediate_hedge: bool


__all__ = ["FluxStrategyCapabilities"]
