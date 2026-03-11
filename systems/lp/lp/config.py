from __future__ import annotations

import configparser
from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path
from typing import Any

from lp.hedgers.registry import get_hedger_meta


def _as_decimal(value: Any) -> Decimal:
    return Decimal(str(value).strip())


def _coalesce(*values: str | None) -> str | None:
    for value in values:
        if value is None:
            continue
        stripped = value.strip()
        if stripped:
            return stripped
    return None


def _bool_from_config(
    section: configparser.SectionProxy | None,
    key: str,
    *,
    default: bool,
) -> bool:
    if section is None or key not in section:
        return default

    value = str(section.get(key, "")).strip().lower()
    if value in {"1", "true", "yes", "y", "on"}:
        return True
    if value in {"0", "false", "no", "n", "off"}:
        return False
    return default


def _mask_api_key(key: str) -> str:
    if not key:
        return "(unset)"
    if len(key) <= 8:
        return key
    return f"{key[:4]}...{key[-4:]}"


@dataclass(frozen=True, slots=True)
class LpHedgerConfig:
    hedger_id: str
    label: str
    job_id: str
    state_key: str
    lp_mode: str
    chain: str
    amm: str
    pool_address: str
    token0_symbol: str
    token1_symbol: str
    token0_decimals: int
    token1_decimals: int
    initial_token0: Decimal
    initial_token1: Decimal
    price_lower: Decimal
    price_upper: Decimal
    target_net_token0: Decimal
    target_net_token1: Decimal
    perp_symbol_token0: str
    perp_symbol_token1: str
    order_qty_step_token0: Decimal
    order_qty_step_token1: Decimal
    max_slippage_bps: Decimal
    price_move_pct: Decimal
    token0_exposure_usd_threshold: Decimal
    token1_exposure_usd_threshold: Decimal
    min_order_qty_token0: Decimal
    min_order_qty_token1: Decimal
    poll_interval_sec: int
    hedge_token0: bool
    hedge_token1: bool
    bybit_api_key: str
    bybit_api_secret: str

    @property
    def initial_eth(self) -> Decimal:
        return self.initial_token0

    @property
    def initial_plume(self) -> Decimal:
        return self.initial_token1

    @property
    def target_net_eth(self) -> Decimal:
        return self.target_net_token0

    @property
    def target_net_plume(self) -> Decimal:
        return self.target_net_token1

    @property
    def eth_symbol(self) -> str:
        return self.perp_symbol_token0

    @property
    def plume_symbol(self) -> str:
        return self.perp_symbol_token1

    @property
    def eth_qty_step(self) -> Decimal:
        return self.order_qty_step_token0

    @property
    def plume_qty_step(self) -> Decimal:
        return self.order_qty_step_token1

    @property
    def eth_exposure_usd_threshold(self) -> Decimal:
        return self.token0_exposure_usd_threshold

    @property
    def plume_exposure_usd_threshold(self) -> Decimal:
        return self.token1_exposure_usd_threshold

    @property
    def min_order_qty_eth(self) -> Decimal:
        return self.min_order_qty_token0

    @property
    def min_order_qty_plume(self) -> Decimal:
        return self.min_order_qty_token1

    @property
    def api_key_hint(self) -> str:
        return _mask_api_key(self.bybit_api_key)

    def summary(self) -> dict[str, str]:
        return {
            "id": self.hedger_id,
            "label": self.label,
            "job_id": self.job_id,
            "state_key": self.state_key,
            "token0_symbol": self.token0_symbol,
            "token1_symbol": self.token1_symbol,
            "api_key_hint": self.api_key_hint,
        }


def load_lp_hedger_config(path: str | Path) -> LpHedgerConfig:
    parser = configparser.ConfigParser()
    path_obj = Path(path)
    if not parser.read(path_obj):
        raise FileNotFoundError(f"Config file not found: {path_obj}")

    required_sections = ("lp_pool", "target")
    missing = [name for name in required_sections if not parser.has_section(name)]
    if missing:
        raise ValueError(f"Missing required sections in hedger config: {', '.join(missing)}")

    identity = parser["identity"] if parser.has_section("identity") else None
    lp_pool = parser["lp_pool"]
    bybit = parser["bybit"] if parser.has_section("bybit") else None
    rebalance = parser["rebalance"] if parser.has_section("rebalance") else None
    target = parser["target"]
    hedge = parser["hedge"] if parser.has_section("hedge") else None

    hedger_id = _coalesce(identity.get("id") if identity else None, "eth_plume_lp")
    assert hedger_id is not None
    token0_symbol = _coalesce(lp_pool.get("token0_symbol"), "WETH")
    token1_symbol = _coalesce(lp_pool.get("token1_symbol"), "WPLUME")
    assert token0_symbol is not None
    assert token1_symbol is not None
    registry_meta = get_hedger_meta(hedger_id)
    label = _coalesce(
        identity.get("label") if identity else None,
        f"{token0_symbol}/{token1_symbol} LP Hedger",
    )
    assert label is not None

    state_key = _coalesce(
        rebalance.get("state_key") if rebalance else None,
        identity.get("state_key") if identity else None,
        registry_meta.state_key if registry_meta else None,
        f"{hedger_id}_hedger",
    )
    job_id = _coalesce(
        identity.get("job_id") if identity else None,
        registry_meta.job_id if registry_meta else None,
        f"service-{hedger_id.replace('_', '-')}",
    )
    initial_token0 = _coalesce(lp_pool.get("initial_token0"), lp_pool.get("initial_eth"))
    initial_token1 = _coalesce(lp_pool.get("initial_token1"), lp_pool.get("initial_plume"))
    if initial_token0 is None or initial_token1 is None:
        raise ValueError(
            "initial_token0/initial_token1 or initial_eth/initial_plume must be set in [lp_pool]",
        )

    target_net_token0 = _coalesce(target.get("target_net_token0"), target.get("target_net_eth"), "0")
    target_net_token1 = _coalesce(
        target.get("target_net_token1"),
        target.get("target_net_plume"),
        "0",
    )
    perp_symbol_token0 = _coalesce(
        bybit.get("perp_symbol_token0") if bybit else None,
        bybit.get("eth_symbol") if bybit else None,
        bybit.get("symbol") if bybit else None,
        "ETHUSDT",
    )
    perp_symbol_token1 = _coalesce(
        bybit.get("perp_symbol_token1") if bybit else None,
        bybit.get("plume_symbol") if bybit else None,
    )
    order_qty_step_token0 = _coalesce(
        bybit.get("order_qty_step_token0") if bybit else None,
        bybit.get("eth_qty_step") if bybit else None,
        bybit.get("qty_step") if bybit else None,
        "0.001",
    )
    order_qty_step_token1 = _coalesce(
        bybit.get("order_qty_step_token1") if bybit else None,
        bybit.get("plume_qty_step") if bybit else None,
        bybit.get("qty_step") if bybit else None,
        "1",
    )
    token0_exposure_usd_threshold = _coalesce(
        rebalance.get("token0_exposure_usd_threshold") if rebalance else None,
        rebalance.get("eth_exposure_usd_threshold") if rebalance else None,
        rebalance.get("exposure_usd_threshold") if rebalance else None,
        "0",
    )
    token1_exposure_usd_threshold = _coalesce(
        rebalance.get("token1_exposure_usd_threshold") if rebalance else None,
        rebalance.get("plume_exposure_usd_threshold") if rebalance else None,
        rebalance.get("exposure_usd_threshold") if rebalance else None,
        "0",
    )
    min_order_qty_token0 = _coalesce(
        rebalance.get("min_order_qty_token0") if rebalance else None,
        rebalance.get("min_order_qty_eth") if rebalance else None,
        rebalance.get("min_order_qty") if rebalance else None,
        "0",
    )
    min_order_qty_token1 = _coalesce(
        rebalance.get("min_order_qty_token1") if rebalance else None,
        rebalance.get("min_order_qty_plume") if rebalance else None,
        "0",
    )
    lp_mode = _coalesce(lp_pool.get("mode"), "onchain")
    chain = _coalesce(lp_pool.get("chain"), "plume")
    amm = _coalesce(lp_pool.get("amm"), "rooster_v3")
    pool_address = _coalesce(lp_pool.get("pool_address")) or ""
    assert state_key is not None
    assert job_id is not None
    assert target_net_token0 is not None
    assert target_net_token1 is not None
    assert perp_symbol_token0 is not None
    assert order_qty_step_token0 is not None
    assert order_qty_step_token1 is not None
    assert token0_exposure_usd_threshold is not None
    assert token1_exposure_usd_threshold is not None
    assert min_order_qty_token0 is not None
    assert min_order_qty_token1 is not None
    assert lp_mode is not None
    assert chain is not None
    assert amm is not None

    return LpHedgerConfig(
        hedger_id=hedger_id,
        label=label,
        job_id=job_id,
        state_key=state_key,
        lp_mode="synthetic" if lp_mode.lower() == "synthetic" else "onchain",
        chain=chain,
        amm=amm,
        pool_address=pool_address,
        token0_symbol=token0_symbol,
        token1_symbol=token1_symbol,
        token0_decimals=lp_pool.getint("token0_decimals", fallback=18),
        token1_decimals=lp_pool.getint("token1_decimals", fallback=18),
        initial_token0=_as_decimal(initial_token0),
        initial_token1=_as_decimal(initial_token1),
        price_lower=_as_decimal(lp_pool.get("price_lower", "0")),
        price_upper=_as_decimal(lp_pool.get("price_upper", "0")),
        target_net_token0=_as_decimal(target_net_token0),
        target_net_token1=_as_decimal(target_net_token1),
        perp_symbol_token0=perp_symbol_token0,
        perp_symbol_token1=perp_symbol_token1 or "",
        order_qty_step_token0=_as_decimal(order_qty_step_token0),
        order_qty_step_token1=_as_decimal(order_qty_step_token1),
        max_slippage_bps=_as_decimal(bybit.get("max_slippage_bps", "0") if bybit else "0"),
        price_move_pct=_as_decimal(rebalance.get("price_move_pct", "0") if rebalance else "0"),
        token0_exposure_usd_threshold=_as_decimal(token0_exposure_usd_threshold),
        token1_exposure_usd_threshold=_as_decimal(token1_exposure_usd_threshold),
        min_order_qty_token0=_as_decimal(min_order_qty_token0),
        min_order_qty_token1=_as_decimal(min_order_qty_token1),
        poll_interval_sec=rebalance.getint("poll_interval_sec", fallback=3) if rebalance else 3,
        hedge_token0=_bool_from_config(hedge, "hedge_token0", default=True),
        hedge_token1=_bool_from_config(hedge, "hedge_token1", default=True),
        bybit_api_key=str(bybit.get("api_key", "")).strip() if bybit else "",
        bybit_api_secret=str(bybit.get("api_secret", "")).strip() if bybit else "",
    )


__all__ = ["LpHedgerConfig", "load_lp_hedger_config"]
