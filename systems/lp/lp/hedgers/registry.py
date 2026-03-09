from __future__ import annotations

from collections.abc import Iterable

from .models import LpHedgerMeta


def _build_meta(
    *,
    hedger_id: str,
    job_id: str,
    state_key: str,
    config_env_var: str,
    config_default_path: str,
    default_enabled: bool,
) -> LpHedgerMeta:
    return LpHedgerMeta(
        id=hedger_id,
        job_id=job_id,
        state_key=state_key,
        snapshot_key=f"{state_key}:snapshot",
        events_key=f"{state_key}:events",
        mode_key=f"{state_key}:mode",
        geometry_overrides_key=f"{state_key}:geometry_overrides",
        threshold_overrides_key=f"{state_key}:threshold_overrides",
        config_env_var=config_env_var,
        config_default_path=config_default_path,
        default_enabled=default_enabled,
    )


_DEFAULT_HEDGERS: tuple[LpHedgerMeta, ...] = (
    _build_meta(
        hedger_id="eth_plume_lp",
        job_id="service-eth-plume-lp-hedger",
        state_key="eth_plume_lp_hedger",
        config_env_var="ETH_PLUME_LP_HEDGER_CONFIG",
        config_default_path="deploy/lp/hedgers/eth_plume_lp_hedger.ini",
        default_enabled=True,
    ),
    _build_meta(
        hedger_id="eth_plume_lp_band2",
        job_id="service-eth-plume-lp-hedger-band2",
        state_key="eth_plume_lp_hedger_band2",
        config_env_var="ETH_PLUME_LP_HEDGER_BAND2_CONFIG",
        config_default_path="deploy/lp/hedgers/eth_plume_lp_hedger_band2.ini",
        default_enabled=True,
    ),
    _build_meta(
        hedger_id="hype_usdt_lp",
        job_id="service-hedger3",
        state_key="hype_usdt_lp_hedger",
        config_env_var="HYPE_USDT_LP_HEDGER_CONFIG",
        config_default_path="deploy/lp/hedgers/hype_usdt_lp_hedger.ini.disabled",
        default_enabled=False,
    ),
    _build_meta(
        hedger_id="plume_weth_lp",
        job_id="service-hedger4",
        state_key="plume_weth_lp_hedger",
        config_env_var="PLUME_WETH_LP_HEDGER_CONFIG",
        config_default_path="deploy/lp/hedgers/plume_weth_lp_hedger.ini.disabled",
        default_enabled=False,
    ),
    _build_meta(
        hedger_id="third_lp",
        job_id="service-hedger5",
        state_key="third_lp_hedger",
        config_env_var="THIRD_LP_HEDGER_CONFIG",
        config_default_path="deploy/lp/hedgers/third_lp_hedger.ini.disabled",
        default_enabled=False,
    ),
)

_REGISTRY = {meta.id: meta for meta in _DEFAULT_HEDGERS}


def list_hedgers() -> tuple[LpHedgerMeta, ...]:
    return tuple(_DEFAULT_HEDGERS)


def list_hedger_metas() -> tuple[LpHedgerMeta, ...]:
    return list_hedgers()


def iter_hedgers() -> Iterable[LpHedgerMeta]:
    return iter(_DEFAULT_HEDGERS)


def get_hedger_meta(hedger_id: str) -> LpHedgerMeta | None:
    return _REGISTRY.get(hedger_id)


__all__ = ["get_hedger_meta", "iter_hedgers", "list_hedger_metas", "list_hedgers"]
