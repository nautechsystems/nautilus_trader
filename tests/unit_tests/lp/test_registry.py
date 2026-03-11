from __future__ import annotations

from lp.hedgers import get_hedger_meta
from lp.hedgers import list_hedgers
from lp.hedgers import list_public_hedgers


def test_registry_preserves_chainsaw_env_var_names() -> None:
    meta = get_hedger_meta("eth_plume_lp")

    assert meta is not None
    assert meta.job_id == "service-eth-plume-lp-hedger"
    assert meta.state_key == "eth_plume_lp_hedger"
    assert meta.snapshot_key == "eth_plume_lp_hedger:snapshot"
    assert meta.events_key == "eth_plume_lp_hedger:events"
    assert meta.mode_key == "eth_plume_lp_hedger:mode"
    assert meta.geometry_overrides_key == "eth_plume_lp_hedger:geometry_overrides"
    assert meta.threshold_overrides_key == "eth_plume_lp_hedger:threshold_overrides"
    assert meta.config_env_var == "ETH_PLUME_LP_HEDGER_CONFIG"
    assert meta.config_default_path == "deploy/lp/hedgers/eth_plume_lp_hedger.ini"


def test_registry_marks_only_band1_and_band2_enabled_by_default() -> None:
    metas = {meta.id: meta for meta in list_hedgers()}

    assert list(metas) == [
        "eth_plume_lp",
        "eth_plume_lp_band2",
        "hype_usdt_lp",
        "plume_weth_lp",
        "third_lp",
    ]
    assert metas["eth_plume_lp"].default_enabled is True
    assert metas["eth_plume_lp_band2"].default_enabled is True
    assert metas["hype_usdt_lp"].default_enabled is False
    assert metas["plume_weth_lp"].default_enabled is False
    assert metas["third_lp"].default_enabled is False


def test_registry_exposes_staged_generic_instances_publicly_and_hides_third_stub() -> None:
    public_ids = [meta.id for meta in list_public_hedgers()]
    third = get_hedger_meta("third_lp")

    assert public_ids == [
        "eth_plume_lp",
        "eth_plume_lp_band2",
        "hype_usdt_lp",
        "plume_weth_lp",
    ]
    assert third is not None
    assert third.public_visible is False
