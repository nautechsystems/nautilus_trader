import sys

from flux.strategies.shared.publisher_common import build_role_map_payload
from flux.strategies.shared.quote_snapshot import build_quote_snapshot_payload

if __name__ == "flux.strategies.shared":
    sys.modules.setdefault("nautilus_trader.flux.strategies.shared", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.strategies.shared":
    sys.modules.setdefault("flux.strategies.shared", sys.modules[__name__])


__all__ = [
    "build_quote_snapshot_payload",
    "build_role_map_payload",
]
