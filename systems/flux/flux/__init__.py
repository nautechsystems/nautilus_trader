"""
The `flux` package contains production modules for MakerV3 Flux integration.
"""

import sys

if __name__ == "flux":
    sys.modules.setdefault("nautilus_trader.flux", sys.modules[__name__])
    nautilus_pkg = sys.modules.get("nautilus_trader")
    if nautilus_pkg is not None:
        setattr(nautilus_pkg, "flux", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux":
    sys.modules.setdefault("flux", sys.modules[__name__])
    nautilus_pkg = sys.modules.get("nautilus_trader")
    if nautilus_pkg is not None:
        setattr(nautilus_pkg, "flux", sys.modules[__name__])

__all__: list[str] = []
