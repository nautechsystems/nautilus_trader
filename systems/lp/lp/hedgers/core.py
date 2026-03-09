from __future__ import annotations

import json
import logging
import time
from dataclasses import dataclass
from decimal import ROUND_DOWN
from decimal import Decimal
from typing import Any
from typing import Protocol

from lp.config import LpHedgerConfig
from lp.execution.bybit import MarketOrderRequest
from lp.execution.bybit import PerpExecutionClient
from lp.market.rooster import compute_amounts_from_liquidity
from lp.market.rooster import compute_liquidity_from_initial_deposit
from lp.market.rooster import price_to_sqrt_price
from lp.market.rooster import resolve_pool_price_token1_per_token0


logger = logging.getLogger(__name__)
TOKEN1_MIN_NOTIONAL_USD = Decimal(10)


@dataclass(frozen=True, slots=True)
class LpGeometry:
    initial_token0: Decimal
    initial_token1: Decimal
    price_lower: Decimal
    price_upper: Decimal

    @property
    def initial_eth(self) -> Decimal:
        return self.initial_token0

    @property
    def initial_plume(self) -> Decimal:
        return self.initial_token1


@dataclass(frozen=True, slots=True)
class RuntimeState:
    price: Decimal
    price_move_pct: Decimal
    price_source: str
    pool_price: Decimal
    pool_price_source: str
    lp_token0: Decimal
    lp_token1: Decimal
    perp_token0: Decimal
    perp_token1: Decimal
    net_token0: Decimal
    net_token1: Decimal
    target_net_token0: Decimal
    target_net_token1: Decimal
    token0_error: Decimal
    token1_error: Decimal
    token0_mark: Decimal
    token1_mark: Decimal
    token0_usd_error: Decimal
    token1_usd_error: Decimal
    last_hedge_price: Decimal
    last_net_token0: Decimal
    last_net_token1: Decimal
    base_geometry: LpGeometry
    effective_geometry: LpGeometry
    base_token0_threshold: Decimal
    base_token1_threshold: Decimal
    effective_token0_threshold: Decimal
    effective_token1_threshold: Decimal
    base_price_move_pct: Decimal
    effective_price_move_pct: Decimal
    hedger_enabled: bool
    token0_price_triggered: bool
    token1_price_triggered: bool
    token0_usd_triggered: bool
    token1_usd_triggered: bool


class RedisClient(Protocol):
    def get(self, key: str) -> Any: ...

    def set(self, key: str, value: str) -> None: ...

    def lpush(self, key: str, value: str) -> int: ...

    def ltrim(self, key: str, start: int, end: int) -> None: ...


def _as_decimal(value: Any) -> Decimal:
    return Decimal(str(value))


def _decode_json(raw: Any) -> dict[str, Any] | None:
    if raw is None:
        return None
    if isinstance(raw, bytes):
        raw = raw.decode("utf-8")
    try:
        parsed = json.loads(raw)
    except Exception:
        logger.warning("Invalid LP hedger JSON payload", exc_info=True)
        return None
    return parsed if isinstance(parsed, dict) else None


class LpHedger:
    def __init__(
        self,
        *,
        config: LpHedgerConfig,
        price_helper: object,
        bybit_client: PerpExecutionClient,
        redis_client: RedisClient,
    ) -> None:
        self.config = config
        self._price_helper = price_helper
        self._bybit = bybit_client
        self._redis = redis_client
        self._base_geometry = LpGeometry(
            initial_token0=config.initial_token0,
            initial_token1=config.initial_token1,
            price_lower=config.price_lower,
            price_upper=config.price_upper,
        )
        self._current_geometry = self._base_geometry
        self._sqrt_lower = price_to_sqrt_price(self._base_geometry.price_lower)
        self._sqrt_upper = price_to_sqrt_price(self._base_geometry.price_upper)
        self._liquidity = compute_liquidity_from_initial_deposit(
            self._base_geometry.initial_token0,
            self._base_geometry.initial_token1,
            self._sqrt_lower,
            self._sqrt_upper,
        )
        self._geometry_overrides: dict[str, Decimal] = {}
        self._threshold_overrides: dict[str, Decimal] = {}
        self._target_net_token0 = config.target_net_token0
        self._target_net_token1 = config.target_net_token1
        self._base_token0_exposure_usd_threshold = config.token0_exposure_usd_threshold
        self._base_token1_exposure_usd_threshold = config.token1_exposure_usd_threshold
        self._base_price_move_pct = config.price_move_pct
        self._token0_exposure_usd_threshold = self._base_token0_exposure_usd_threshold
        self._token1_exposure_usd_threshold = self._base_token1_exposure_usd_threshold
        self._price_move_pct = self._base_price_move_pct
        self._hedger_enabled = False

    @property
    def target_net_token0(self) -> Decimal:
        return self._target_net_token0

    @property
    def target_net_token1(self) -> Decimal:
        return self._target_net_token1

    def tick(self) -> dict[str, Any]:
        try:
            state = self._collect_runtime_state(persist_initial_state=True)
        except Exception:
            logger.warning("LP hedger tick failed", exc_info=True)
            return self._load_saved_snapshot()
        snapshot = self._snapshot_from_state(state)
        self._save_snapshot(snapshot)

        should_hedge_token0 = (
            self.config.hedge_token0
            and state.token0_price_triggered
            and abs(state.token0_error) >= self.config.min_order_qty_token0
        )
        should_hedge_token1 = (
            self.config.hedge_token1
            and state.token1_price_triggered
            and abs(state.token1_error) >= self.config.min_order_qty_token1
            and state.token1_usd_error >= TOKEN1_MIN_NOTIONAL_USD
        )
        if not state.hedger_enabled or not (should_hedge_token0 or should_hedge_token1):
            return snapshot

        new_perp_token0 = state.perp_token0
        new_perp_token1 = state.perp_token1
        new_net_token0 = state.net_token0
        new_net_token1 = state.net_token1
        hedge_executed = False

        if should_hedge_token0:
            result = self._maybe_execute_hedge(
                asset=self.config.token0_symbol,
                symbol=self.config.perp_symbol_token0,
                qty_step=self.config.order_qty_step_token0,
                incremental_change=-state.token0_error,
                price_source=state.price_source,
                mark_price=state.token0_mark,
                price_triggered=state.token0_price_triggered,
                usd_triggered=state.token0_usd_triggered,
                token0_usd_error_before=state.token0_usd_error,
                token1_usd_error_before=state.token1_usd_error,
                lp_position=state.lp_token0,
                current_perp=new_perp_token0,
            )
            if result is not None:
                new_perp_token0, new_net_token0 = result
                hedge_executed = True

        if should_hedge_token1 and self.config.perp_symbol_token1:
            result = self._maybe_execute_hedge(
                asset=self.config.token1_symbol,
                symbol=self.config.perp_symbol_token1,
                qty_step=self.config.order_qty_step_token1,
                incremental_change=-state.token1_error,
                price_source=state.price_source,
                mark_price=state.token1_mark,
                price_triggered=state.token1_price_triggered,
                usd_triggered=state.token1_usd_triggered,
                token0_usd_error_before=state.token0_usd_error,
                token1_usd_error_before=state.token1_usd_error,
                lp_position=state.lp_token1,
                current_perp=new_perp_token1,
            )
            if result is not None:
                new_perp_token1, new_net_token1 = result
                hedge_executed = True

        if hedge_executed:
            self._save_state_best_effort(state.price, new_net_token0, new_net_token1)
        return snapshot

    def build_market_order(
        self,
        *,
        symbol: str,
        incremental_change: Decimal,
        qty_step: Decimal,
    ) -> MarketOrderRequest | None:
        qty = self._round_to_step(incremental_change, qty_step)
        if qty == 0:
            return None
        return MarketOrderRequest(
            symbol=symbol,
            side="buy" if qty > 0 else "sell",
            qty=abs(qty),
            max_slippage_bps=self.config.max_slippage_bps,
        )

    def build_snapshot(self) -> dict[str, Any]:
        state = self._collect_runtime_state()
        return self._snapshot_from_state(state)

    def _collect_runtime_state(self, *, persist_initial_state: bool = False) -> RuntimeState:
        self._refresh_geometry()
        self._refresh_thresholds()
        self._refresh_enabled_flag()
        perp_token0, token0_mark, perp_token1, token1_mark = self._load_perp_state()
        cross_price = token0_mark / token1_mark
        pool_price, pool_price_source = self._resolve_pool_price(cross_price)
        lp_token0, lp_token1 = self._compute_lp_balances(pool_price)
        net_token0 = lp_token0 + perp_token0
        net_token1 = lp_token1 + perp_token1
        last_hedge_price, last_net_token0, last_net_token1 = self._load_or_seed_state(
            cross_price=cross_price,
            net_token0=net_token0,
            net_token1=net_token1,
            persist_initial_state=persist_initial_state,
        )
        token0_error = net_token0 - self._target_net_token0
        token1_error = net_token1 - self._target_net_token1
        if last_hedge_price == 0:
            price_move_pct = Decimal(0)
        else:
            price_move_pct = abs((cross_price / last_hedge_price) - 1) * Decimal(100)
        token0_usd_error = abs(token0_error) * token0_mark
        token1_usd_error = abs(token1_error) * token1_mark
        return RuntimeState(
            price=cross_price,
            price_move_pct=price_move_pct,
            price_source="perp_cross",
            pool_price=pool_price,
            pool_price_source=pool_price_source,
            lp_token0=lp_token0,
            lp_token1=lp_token1,
            perp_token0=perp_token0,
            perp_token1=perp_token1,
            net_token0=net_token0,
            net_token1=net_token1,
            target_net_token0=self._target_net_token0,
            target_net_token1=self._target_net_token1,
            token0_error=token0_error,
            token1_error=token1_error,
            token0_mark=token0_mark,
            token1_mark=token1_mark,
            token0_usd_error=token0_usd_error,
            token1_usd_error=token1_usd_error,
            last_hedge_price=last_hedge_price,
            last_net_token0=last_net_token0,
            last_net_token1=last_net_token1,
            base_geometry=self._base_geometry,
            effective_geometry=self._current_geometry,
            base_token0_threshold=self._base_token0_exposure_usd_threshold,
            base_token1_threshold=self._base_token1_exposure_usd_threshold,
            effective_token0_threshold=self._token0_exposure_usd_threshold,
            effective_token1_threshold=self._token1_exposure_usd_threshold,
            base_price_move_pct=self._base_price_move_pct,
            effective_price_move_pct=self._price_move_pct,
            hedger_enabled=self._hedger_enabled,
            token0_price_triggered=price_move_pct >= self._price_move_pct,
            token1_price_triggered=price_move_pct >= self._price_move_pct,
            token0_usd_triggered=token0_usd_error >= self._token0_exposure_usd_threshold,
            token1_usd_triggered=token1_usd_error >= self._token1_exposure_usd_threshold,
        )

    def _load_perp_state(self) -> tuple[Decimal, Decimal, Decimal, Decimal]:
        try:
            perp_token0 = _as_decimal(self._bybit.get_position_size(self.config.perp_symbol_token0))
            token0_mark = _as_decimal(self._bybit.get_mark_price(self.config.perp_symbol_token0))
        except Exception:
            logger.warning("Failed to fetch token0 perp state", exc_info=True)
            raise
        try:
            if self.config.perp_symbol_token1:
                perp_token1 = _as_decimal(self._bybit.get_position_size(self.config.perp_symbol_token1))
                token1_mark = _as_decimal(self._bybit.get_mark_price(self.config.perp_symbol_token1))
            else:
                perp_token1 = Decimal(0)
                token1_mark = Decimal(1)
        except Exception:
            logger.warning("Failed to fetch token1 perp state", exc_info=True)
            raise

        if token0_mark <= 0 or token1_mark <= 0:
            raise ValueError("mark prices must be positive")
        return perp_token0, token0_mark, perp_token1, token1_mark

    def _resolve_pool_price(self, cross_price: Decimal) -> tuple[Decimal, str]:
        if self.config.lp_mode == "onchain":
            try:
                pool_price = resolve_pool_price_token1_per_token0(
                    self._price_helper,
                    pool_address=self.config.pool_address,
                )
                pool_price_source = str(getattr(self._price_helper, "last_source", "onchain"))
            except Exception:
                logger.warning(
                    "Failed to resolve LP pool price; continuing with perp cross price",
                    exc_info=True,
                )
                pool_price = cross_price
                pool_price_source = "perp_cross"
        else:
            pool_price = cross_price
            pool_price_source = "perp_mark"
        return pool_price, pool_price_source

    def _compute_lp_balances(self, pool_price: Decimal) -> tuple[Decimal, Decimal]:
        try:
            sqrt_price = price_to_sqrt_price(pool_price)
            return compute_amounts_from_liquidity(
                self._liquidity,
                sqrt_price,
                self._sqrt_lower,
                self._sqrt_upper,
            )
        except Exception:
            logger.warning("Failed to compute LP exposures", exc_info=True)
            raise

    def _load_or_seed_state(
        self,
        *,
        cross_price: Decimal,
        net_token0: Decimal,
        net_token1: Decimal,
        persist_initial_state: bool,
    ) -> tuple[Decimal, Decimal, Decimal]:
        last_hedge_price, last_net_token0, last_net_token1 = self._load_state()
        if last_hedge_price is None or last_net_token0 is None or last_net_token1 is None:
            last_hedge_price = cross_price
            last_net_token0 = net_token0
            last_net_token1 = net_token1
            if persist_initial_state:
                self._save_state(last_hedge_price, last_net_token0, last_net_token1)
        return last_hedge_price, last_net_token0, last_net_token1

    def _snapshot_from_state(self, state: RuntimeState) -> dict[str, Any]:
        snapshot = {
            "timestamp": int(time.time()),
            "price_plume_per_eth": str(state.price),
            "price_token1_per_token0": str(state.price),
            "price_move_pct": str(state.price_move_pct),
            "price_source": state.price_source,
            "lp_mode": self.config.lp_mode,
            "token0_symbol": self.config.token0_symbol,
            "token1_symbol": self.config.token1_symbol,
            "perp_symbol_token0": self.config.perp_symbol_token0,
            "perp_symbol_token1": self.config.perp_symbol_token1,
            "lp_eth": str(state.lp_token0),
            "lp_plume": str(state.lp_token1),
            "lp_token0": str(state.lp_token0),
            "lp_token1": str(state.lp_token1),
            "perp_eth": str(state.perp_token0),
            "perp_plume": str(state.perp_token1),
            "perp_token0": str(state.perp_token0),
            "perp_token1": str(state.perp_token1),
            "net_eth": str(state.net_token0),
            "net_plume": str(state.net_token1),
            "net_token0": str(state.net_token0),
            "net_token1": str(state.net_token1),
            "target_net_eth": str(state.target_net_token0),
            "target_net_plume": str(state.target_net_token1),
            "target_net_token0": str(state.target_net_token0),
            "target_net_token1": str(state.target_net_token1),
            "eth_error": str(state.token0_error),
            "plume_error": str(state.token1_error),
            "token0_error": str(state.token0_error),
            "token1_error": str(state.token1_error),
            "eth_mark": str(state.token0_mark),
            "plume_mark": str(state.token1_mark),
            "token0_mark": str(state.token0_mark),
            "token1_mark": str(state.token1_mark),
            "eth_usd_error": str(state.token0_usd_error),
            "plume_usd_error": str(state.token1_usd_error),
            "token0_usd_error": str(state.token0_usd_error),
            "token1_usd_error": str(state.token1_usd_error),
            "last_hedge_price": str(state.last_hedge_price),
            "last_net_eth": str(state.last_net_token0),
            "last_net_plume": str(state.last_net_token1),
            "last_net_token0": str(state.last_net_token0),
            "last_net_token1": str(state.last_net_token1),
            "initial_eth_base": str(state.base_geometry.initial_token0),
            "initial_plume_base": str(state.base_geometry.initial_token1),
            "initial_token0_base": str(state.base_geometry.initial_token0),
            "initial_token1_base": str(state.base_geometry.initial_token1),
            "price_lower_base": str(state.base_geometry.price_lower),
            "price_upper_base": str(state.base_geometry.price_upper),
            "initial_eth_effective": str(state.effective_geometry.initial_token0),
            "initial_plume_effective": str(state.effective_geometry.initial_token1),
            "initial_token0_effective": str(state.effective_geometry.initial_token0),
            "initial_token1_effective": str(state.effective_geometry.initial_token1),
            "price_lower_effective": str(state.effective_geometry.price_lower),
            "price_upper_effective": str(state.effective_geometry.price_upper),
            "eth_exposure_usd_threshold_base": str(state.base_token0_threshold),
            "plume_exposure_usd_threshold_base": str(state.base_token1_threshold),
            "eth_exposure_usd_threshold_effective": str(state.effective_token0_threshold),
            "plume_exposure_usd_threshold_effective": str(state.effective_token1_threshold),
            "price_move_pct_base": str(state.base_price_move_pct),
            "price_move_pct_effective": str(state.effective_price_move_pct),
            "hedger_enabled": bool(state.hedger_enabled),
            "hedge_token0": bool(self.config.hedge_token0),
            "hedge_token1": bool(self.config.hedge_token1),
            "pool_price_plume_per_eth": str(state.pool_price),
            "pool_price_token1_per_token0": str(state.pool_price),
            "pool_price_source": state.pool_price_source,
        }
        return snapshot

    def _load_state(self) -> tuple[Decimal | None, Decimal | None, Decimal | None]:
        payload = _decode_json(self._redis.get(self._state_key()))
        if not payload:
            return None, None, None
        try:
            return (
                _as_decimal(payload["last_hedge_price"]),
                _as_decimal(payload.get("last_net_token0", payload.get("last_net_eth", "0"))),
                _as_decimal(payload.get("last_net_token1", payload.get("last_net_plume", "0"))),
            )
        except Exception:
            logger.warning("Invalid LP hedger state payload", exc_info=True)
            return None, None, None

    def _save_state(
        self,
        last_price: Decimal,
        last_net_token0: Decimal,
        last_net_token1: Decimal,
    ) -> None:
        payload = {
            "last_hedge_price": str(last_price),
            "last_net_eth": str(last_net_token0),
            "last_net_plume": str(last_net_token1),
            "last_net_token0": str(last_net_token0),
            "last_net_token1": str(last_net_token1),
        }
        self._redis.set(self._state_key(), json.dumps(payload))

    def _save_state_best_effort(
        self,
        last_price: Decimal,
        last_net_token0: Decimal,
        last_net_token1: Decimal,
    ) -> None:
        try:
            self._save_state(last_price, last_net_token0, last_net_token1)
        except Exception:
            logger.warning("Failed to write LP hedger state", exc_info=True)

    def _load_saved_snapshot(self) -> dict[str, Any]:
        return _decode_json(self._redis.get(self._snapshot_key())) or {}

    def _save_snapshot(self, snapshot: dict[str, Any]) -> None:
        try:
            self._redis.set(self._snapshot_key(), json.dumps(snapshot))
        except Exception:
            logger.warning("Failed to write LP hedger snapshot", exc_info=True)

    def _load_geometry_overrides(self) -> dict[str, Decimal]:
        payload = _decode_json(self._redis.get(self._geometry_overrides_key())) or {}
        overrides: dict[str, Decimal] = {}
        field_map = {
            "initial_token0": "initial_token0",
            "initial_eth": "initial_token0",
            "initial_token1": "initial_token1",
            "initial_plume": "initial_token1",
            "price_lower": "price_lower",
            "price_upper": "price_upper",
        }
        for key, target_key in field_map.items():
            if key not in payload:
                continue
            overrides[target_key] = _as_decimal(payload[key])
        return overrides

    def _refresh_geometry(self) -> None:
        try:
            overrides = self._load_geometry_overrides()
        except Exception:
            logger.warning("Failed to load LP geometry overrides", exc_info=True)
            overrides = {}
        if overrides == self._geometry_overrides:
            return
        geometry = self._base_geometry
        if overrides:
            geometry = LpGeometry(
                initial_token0=overrides.get("initial_token0", self._base_geometry.initial_token0),
                initial_token1=overrides.get("initial_token1", self._base_geometry.initial_token1),
                price_lower=overrides.get("price_lower", self._base_geometry.price_lower),
                price_upper=overrides.get("price_upper", self._base_geometry.price_upper),
            )
            try:
                self._validate_geometry(geometry)
            except ValueError:
                logger.warning("Invalid LP geometry overrides; falling back to base", exc_info=True)
                overrides = {}
                geometry = self._base_geometry
        self._geometry_overrides = overrides
        self._current_geometry = geometry
        self._sqrt_lower = price_to_sqrt_price(geometry.price_lower)
        self._sqrt_upper = price_to_sqrt_price(geometry.price_upper)
        self._liquidity = compute_liquidity_from_initial_deposit(
            geometry.initial_token0,
            geometry.initial_token1,
            self._sqrt_lower,
            self._sqrt_upper,
        )

    def _load_threshold_overrides(self) -> dict[str, Decimal]:
        payload = _decode_json(self._redis.get(self._threshold_overrides_key())) or {}
        overrides: dict[str, Decimal] = {}
        field_map = {
            "token0_exposure_usd_threshold": "token0_exposure_usd_threshold",
            "eth_exposure_usd_threshold": "token0_exposure_usd_threshold",
            "token1_exposure_usd_threshold": "token1_exposure_usd_threshold",
            "plume_exposure_usd_threshold": "token1_exposure_usd_threshold",
            "price_move_pct": "price_move_pct",
        }
        for key, target_key in field_map.items():
            if key not in payload:
                continue
            try:
                value = _as_decimal(payload[key])
            except Exception:
                logger.warning("Invalid LP threshold override for %s", key, exc_info=True)
                continue
            if value <= 0:
                logger.warning("Ignoring non-positive LP threshold override for %s", key)
                continue
            overrides[target_key] = value
        return overrides

    def _refresh_thresholds(self) -> None:
        overrides = self._load_threshold_overrides()
        if overrides == self._threshold_overrides:
            return
        self._threshold_overrides = overrides
        self._token0_exposure_usd_threshold = overrides.get(
            "token0_exposure_usd_threshold",
            self._base_token0_exposure_usd_threshold,
        )
        self._token1_exposure_usd_threshold = overrides.get(
            "token1_exposure_usd_threshold",
            self._base_token1_exposure_usd_threshold,
        )
        self._price_move_pct = overrides.get("price_move_pct", self._base_price_move_pct)

    def _refresh_enabled_flag(self) -> None:
        payload = _decode_json(self._redis.get(self._mode_key())) or {}
        self._hedger_enabled = bool(payload.get("enabled", False))

    def _maybe_execute_hedge(
        self,
        *,
        asset: str,
        symbol: str,
        qty_step: Decimal,
        incremental_change: Decimal,
        price_source: str,
        mark_price: Decimal,
        price_triggered: bool,
        usd_triggered: bool,
        token0_usd_error_before: Decimal,
        token1_usd_error_before: Decimal,
        lp_position: Decimal,
        current_perp: Decimal,
    ) -> tuple[Decimal, Decimal] | None:
        order = self.build_market_order(
            symbol=symbol,
            incremental_change=incremental_change,
            qty_step=qty_step,
        )
        if order is None:
            return None
        try:
            if not self._bybit.create_market_order(order):
                return None
        except Exception:
            logger.warning("Failed to submit LP hedge order", exc_info=True)
            return None

        signed_qty = order.qty if order.side == "buy" else -order.qty
        new_perp = current_perp + signed_qty
        new_net = lp_position + new_perp
        self._append_event(
            asset=asset,
            order=order,
            price_source=price_source,
            mark_price=mark_price,
            price_triggered=price_triggered,
            usd_triggered=usd_triggered,
            token0_usd_error_before=token0_usd_error_before,
            token1_usd_error_before=token1_usd_error_before,
            net_after=new_net,
        )
        return new_perp, new_net

    def _append_event(
        self,
        *,
        asset: str,
        order: MarketOrderRequest,
        price_source: str,
        mark_price: Decimal,
        price_triggered: bool,
        usd_triggered: bool,
        token0_usd_error_before: Decimal,
        token1_usd_error_before: Decimal,
        net_after: Decimal,
    ) -> None:
        if price_triggered and usd_triggered:
            trigger_reason = "price|usd_diag"
        elif price_triggered:
            trigger_reason = "price"
        elif usd_triggered:
            trigger_reason = "usd_diag_only"
        else:
            trigger_reason = "size_only"
        event = {
            "timestamp": int(time.time()),
            "asset": asset,
            "symbol": order.symbol,
            "side": order.side,
            "qty": str(order.qty),
            "max_slippage_bps": str(order.max_slippage_bps),
            "price_source": price_source,
            "mark_price": str(mark_price),
            "usd_notional": str(order.qty * mark_price),
            "trigger_reason": trigger_reason,
            "token0_usd_error_before": str(token0_usd_error_before),
            "token1_usd_error_before": str(token1_usd_error_before),
            "net_after": str(net_after),
        }
        key = self._events_key()
        try:
            self._redis.lpush(key, json.dumps(event))
            self._redis.ltrim(key, 0, 199)
        except Exception:
            logger.warning("Failed to append LP hedge event", exc_info=True)

    @staticmethod
    def _round_to_step(value: Decimal, step: Decimal) -> Decimal:
        if step <= 0:
            return value
        if value == 0:
            return Decimal(0)
        increments = (abs(value) / step).to_integral_value(rounding=ROUND_DOWN)
        rounded = increments * step
        return rounded if value > 0 else -rounded

    @staticmethod
    def _validate_geometry(geometry: LpGeometry) -> None:
        if geometry.initial_token0 <= 0 or geometry.initial_token1 <= 0:
            raise ValueError("initial deposits must be positive")
        if geometry.price_lower <= 0 or geometry.price_upper <= 0:
            raise ValueError("price bounds must be positive")
        if geometry.price_lower >= geometry.price_upper:
            raise ValueError("price_lower must be less than price_upper")

    def _state_key(self) -> str:
        return f"{self.config.state_key}:state"

    def _snapshot_key(self) -> str:
        return f"{self.config.state_key}:snapshot"

    def _events_key(self) -> str:
        return f"{self.config.state_key}:events"

    def _mode_key(self) -> str:
        return f"{self.config.state_key}:mode"

    def _geometry_overrides_key(self) -> str:
        return f"{self.config.state_key}:geometry_overrides"

    def _threshold_overrides_key(self) -> str:
        return f"{self.config.state_key}:threshold_overrides"


__all__ = ["LpGeometry", "LpHedger"]
