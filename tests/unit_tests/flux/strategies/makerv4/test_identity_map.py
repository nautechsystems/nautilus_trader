from __future__ import annotations

from nautilus_trader.flux.strategies.registry import get_strategy_identity


def test_makerv4_identity_map_is_explicit() -> None:
    identity = get_strategy_identity("makerv4")

    assert identity.strategy_id == "makerv4"
    assert identity.strategy_family == "maker_v4"
    assert identity.strategy_version == "v4"
    assert identity.param_set == "makerv4"
    assert identity.profile_key == "maker_v4"
