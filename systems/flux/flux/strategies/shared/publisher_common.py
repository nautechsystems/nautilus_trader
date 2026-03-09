from __future__ import annotations

import sys

if __name__ == "flux.strategies.shared.publisher_common":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.publisher_common",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.publisher_common":
    sys.modules.setdefault("flux.strategies.shared.publisher_common", sys.modules[__name__])


def build_role_map_payload(*, maker_leg: str, ref_leg: str, hedge_leg: str | None = None) -> dict[str, str]:
    payload: dict[str, str] = {}
    maker_text = str(maker_leg).strip()
    ref_text = str(ref_leg).strip()
    hedge_text = str(hedge_leg).strip() if hedge_leg is not None else ""
    if maker_text:
        payload["maker_leg"] = maker_text
    if ref_text:
        payload["ref_leg"] = ref_text
        payload["hedge_leg"] = hedge_text or ref_text
    return payload


__all__ = [
    "build_role_map_payload",
]
