"""
Implement the first MakerV4 pure strategy core slice.
"""

from __future__ import annotations

from contextlib import suppress
from dataclasses import asdict
from decimal import Decimal
from typing import Any

from flux.strategies.makerv3 import publisher as makerv3_publisher_mod
from flux.strategies.makerv3.constants import TOPIC_STATE
from flux.strategies.makerv4 import fees as fees_mod
from flux.strategies.makerv4 import publisher as publisher_mod
from flux.strategies.makerv4 import runtime_params as runtime_params_mod
from flux.strategies.makerv4.instruments import translate_hyperliquid_fill_to_ibkr_shares
from flux.strategies.makerv4.managed_orders import HedgeOrderIntent
from flux.strategies.makerv4.managed_orders import PendingHedgeState
from flux.strategies.makerv4.market_data import IbkrQuoteSnapshot
from flux.strategies.makerv4.pricing import build_ibkr_ioc_limit
from flux.strategies.makerv4.pricing import validate_ibkr_quote
from flux.strategies.shared.publisher_common import build_role_map_payload
from flux.strategies.makerv4.wire import HedgeExecutionReport
from flux.strategies.makerv4.wire import MakerFill
from flux.strategies.makerv3.strategy import MakerV3StrategyConfig
from nautilus_trader.trading.strategy import Strategy


class MakerV4StrategyConfig(MakerV3StrategyConfig):
    """
    MakerV4 config surface for the immediate-hedge core.
    """

    ibkr_primary_exchange: str = "NASDAQ"
    hedge_price_tick_size: Decimal = Decimal("0.01")
    hedge_min_share_increment: Decimal = Decimal("1")
    max_ibkr_quote_age_ms: int = 1_000
    max_ibkr_spread_bps: Decimal = Decimal("25")
    outside_rth_hedge_enabled: bool = False


class MakerV4Strategy(Strategy):
    """
    MakerV4 hedge strategy core wrapped in the Nautilus Strategy lifecycle.
    """

    BALANCES_PUBLISH_INTERVAL_MS = 10_000

    def __init__(self, config: MakerV4StrategyConfig) -> None:
        super().__init__(config)
        strategy_id = getattr(config, "strategy_id", None)
        external_strategy_id = getattr(config, "external_strategy_id", None)
        self.runtime_strategy_id = str(external_strategy_id or strategy_id or self.id)
        self._strategy_identity = self.runtime_strategy_id
        self._external_strategy_id = self._strategy_identity
        self._params_manager_factory = None
        self._params_manager = None
        self._portfolio_inventory_feed = None
        self._runtime_params = dict(runtime_params_mod.MAKERV4_RUNTIME_PARAM_DEFAULTS)
        self._maker_instrument = None
        self._instruments: dict[Any, Any] = {}
        self._latest_quotes: dict[Any, dict[str, Any]] = {}
        self._last_market_bbo_publish_ns: dict[Any, int] = {}
        self._last_balances_ns = 0
        self._last_state_ns = 0
        self._last_state_name: str | None = None
        self._state_is_blocked = False
        self._last_stale_cancel_ns = 0
        self._last_pricing_debug: dict[str, Any] = {}
        self.tradeable = True
        self.hedge_disabled_reason: str | None = None
        self._pending_hedge: PendingHedgeState | None = None
        self._hedge_requests: list[HedgeOrderIntent] = []
        self._seen_fill_ids: set[str] = set()
        self._fill_ids_head: list[str] = []

    def set_params_manager_factory(self, factory) -> None:
        self._params_manager_factory = factory

    def configure_portfolio_inventory_feed(self, **kwargs) -> None:
        self._portfolio_inventory_feed = kwargs

    @property
    def hedge_request_count(self) -> int:
        return len(self._hedge_requests)

    @property
    def pending_hedge_qty(self) -> Decimal:
        if self._pending_hedge is None:
            return Decimal("0")
        return self._pending_hedge.remaining_qty

    def _strategy_cache(self) -> Any | None:
        cache = getattr(self, "_cache", None)
        if cache is None:
            cache = getattr(self, "cache", None)
        return cache

    def _resolve_instrument(self, instrument_id: Any) -> Any | None:
        instrument = self._instruments.get(instrument_id)
        if instrument is not None:
            return instrument

        cache = self._strategy_cache()
        instrument_lookup = getattr(cache, "instrument", None)
        if callable(instrument_lookup):
            with suppress(Exception):
                return instrument_lookup(instrument_id)
        return None

    def _load_runtime_params(self) -> None:
        if self._params_manager is None and callable(self._params_manager_factory):
            self._params_manager = self._params_manager_factory(self)
        load = getattr(self._params_manager, "load", None)
        if callable(load):
            loaded = load()
            if isinstance(loaded, dict):
                self._runtime_params.update(loaded)

    def _effective_bot_on(self) -> bool:
        bot_on = self._runtime_params.get("bot_on", getattr(self.config, "bot_on", False))
        return bool(bot_on)

    def _managed_orders(self) -> list[Any]:
        return []

    def _tracked_managed_order_count(self) -> int:
        return 0

    def _quote_runtime_params_snapshot(self) -> dict[str, Any]:
        return dict(self._runtime_params)

    @staticmethod
    def _decimal_or_none(value: Any) -> Decimal | None:
        if value is None:
            return None
        as_decimal = getattr(value, "as_decimal", None)
        if callable(as_decimal):
            with suppress(Exception):
                parsed = as_decimal()
                if parsed is None or parsed <= 0:
                    return None
                return parsed
        try:
            parsed = Decimal(str(value))
        except Exception:
            return None
        if parsed <= 0:
            return None
        return parsed

    @staticmethod
    def _quote_ts_ns(value: Any) -> int:
        try:
            return int(value)
        except Exception:
            return 0

    def _update_quote_snapshot(
        self,
        *,
        instrument_id: Any,
        bid: Decimal | None,
        ask: Decimal | None,
        ts_ns: int,
    ) -> None:
        self._latest_quotes[instrument_id] = {
            "bid": bid,
            "ask": ask,
            "ts_ns": max(0, int(ts_ns)),
        }

    def _prime_cached_quote(self, instrument_id: Any) -> None:
        cache = self._strategy_cache()
        quote_lookup = getattr(cache, "quote_tick", None)
        if not callable(quote_lookup):
            return
        tick = None
        with suppress(Exception):
            tick = quote_lookup(instrument_id)
        if tick is None:
            return
        self._update_quote_snapshot(
            instrument_id=instrument_id,
            bid=self._decimal_or_none(getattr(tick, "bid_price", None)),
            ask=self._decimal_or_none(getattr(tick, "ask_price", None)),
            ts_ns=self._quote_ts_ns(getattr(tick, "ts_event", 0)),
        )

    def _quote_leg_snapshot(self, instrument_id: Any, *, now_ns: int) -> dict[str, Any]:
        instrument = self._resolve_instrument(instrument_id)
        venue = str(getattr(instrument_id, "venue", "")).strip().upper()
        if instrument_id == self.config.reference_instrument_id:
            venue = "IBKR"
        symbol = str(getattr(instrument, "raw_symbol", "")).strip() or str(instrument_id)
        payload: dict[str, Any] = {
            "venue": venue,
            "symbol": symbol,
            "instrument_id": str(instrument_id),
        }
        snapshot = self._latest_quotes.get(instrument_id)
        if snapshot is None:
            return payload

        bid = self._decimal_or_none(snapshot.get("bid"))
        ask = self._decimal_or_none(snapshot.get("ask"))
        ts_ns = self._quote_ts_ns(snapshot.get("ts_ns"))
        if bid is not None:
            payload["bid"] = float(bid)
        if ask is not None:
            payload["ask"] = float(ask)
        if bid is not None and ask is not None:
            payload["mid"] = float((bid + ask) / Decimal("2"))
        if ts_ns > 0:
            payload["ts_ms"] = ts_ns // 1_000_000
            payload["age_ms"] = max(0, (int(now_ns) - ts_ns) // 1_000_000)
        return payload

    def _quote_snapshot_payload(self, *, now_ns: int) -> dict[str, Any]:
        maker_leg = self._quote_leg_snapshot(self.config.maker_instrument_id, now_ns=now_ns)
        ref_leg = self._quote_leg_snapshot(self.config.reference_instrument_id, now_ns=now_ns)
        return publisher_mod.build_quote_snapshot_payload(
            maker_leg=maker_leg,
            hedge_leg=dict(ref_leg),
            ref_leg=ref_leg,
            hedge_ready=bool(
                self.tradeable
                and maker_leg.get("bid") is not None
                and maker_leg.get("ask") is not None
                and ref_leg.get("bid") is not None
                and ref_leg.get("ask") is not None
            ),
            hedge_route="SMART",
            hedge_disabled_reason=self.hedge_disabled_reason,
            ibkr_quote_age_ms=(
                int(ref_leg["age_ms"])
                if ref_leg.get("age_ms") is not None
                else None
            ),
            assumed_hedge_fee_bps=float(self._runtime_params.get("assumed_hedge_fee_bps", 1.0)),
            ts_ms=max(0, int(now_ns)) // 1_000_000,
        )

    def _publish_state_snapshot(self, *, now_ns: int | None = None) -> None:
        publish_ns = int(self.clock.timestamp_ns()) if now_ns is None else int(now_ns)
        state = "running" if self._effective_bot_on() else "bot_off"
        if self.hedge_disabled_reason:
            state = f"blocked_{self.hedge_disabled_reason}"
        payload = {
            "strategy_id": self._external_strategy_id,
            "state": state,
            "bot_on": self._effective_bot_on(),
            "managed_orders": 0,
            "tracked_managed_orders": 0,
            "ts_event": publish_ns,
            "ts_ms": publish_ns // 1_000_000,
            "maker_role_map": build_role_map_payload(
                maker_leg=str(self.config.maker_instrument_id),
                ref_leg=str(self.config.reference_instrument_id),
            ),
            "maker_v4": {
                "quote_snapshot": self._quote_snapshot_payload(now_ns=publish_ns),
            },
        }
        self._last_state_name = state
        self._last_state_ns = publish_ns
        self._publish_json(TOPIC_STATE, payload)

    def _disable_hedging(self, reason: str) -> None:
        self.tradeable = False
        self.hedge_disabled_reason = str(reason)

    def _remember_fill_id(self, fill_id: str) -> None:
        self._seen_fill_ids.add(fill_id)
        self._fill_ids_head.append(fill_id)
        if len(self._fill_ids_head) > 64:
            self._fill_ids_head = self._fill_ids_head[-64:]
            self._seen_fill_ids = set(self._fill_ids_head)

    def on_start(self) -> None:
        self.tradeable = True
        self.hedge_disabled_reason = None
        try:
            self._load_runtime_params()
        except Exception as exc:
            self.log.error(f"Failed to load MakerV4 runtime params: {exc}")
            self.stop()
            return

        maker_instrument = self._resolve_instrument(self.config.maker_instrument_id)
        if maker_instrument is None:
            self.log.error(f"Could not find instrument for {self.config.maker_instrument_id}")
            self.stop()
            return
        reference_instrument = self._resolve_instrument(self.config.reference_instrument_id)
        if reference_instrument is None:
            self.log.error(f"Could not find instrument for {self.config.reference_instrument_id}")
            self.stop()
            return

        self._maker_instrument = maker_instrument
        self._instruments = {
            self.config.maker_instrument_id: maker_instrument,
            self.config.reference_instrument_id: reference_instrument,
        }
        subscribed_instrument_ids: list[Any] = []
        self._last_market_bbo_publish_ns = {}
        for instrument_id in (
            self.config.maker_instrument_id,
            self.config.reference_instrument_id,
        ):
            if instrument_id in subscribed_instrument_ids:
                continue
            subscribed_instrument_ids.append(instrument_id)
            self._last_market_bbo_publish_ns[instrument_id] = 0
            self._prime_cached_quote(instrument_id)
            self.subscribe_quote_ticks(instrument_id=instrument_id)

        self._publish_balances()
        self._publish_state_snapshot()

    def on_stop(self) -> None:
        unsubscribed_instrument_ids: list[Any] = []
        for instrument_id in (
            self.config.maker_instrument_id,
            self.config.reference_instrument_id,
        ):
            if instrument_id in unsubscribed_instrument_ids:
                continue
            unsubscribed_instrument_ids.append(instrument_id)
            with suppress(Exception):
                self.unsubscribe_quote_ticks(instrument_id=instrument_id)
        self._publish_state_snapshot()

    def on_quote_tick(self, tick: Any) -> None:
        instrument_id = getattr(tick, "instrument_id", None)
        if instrument_id is None:
            return

        bid = self._decimal_or_none(getattr(tick, "bid_price", None))
        ask = self._decimal_or_none(getattr(tick, "ask_price", None))
        ts_ns = self._quote_ts_ns(getattr(tick, "ts_event", 0))
        self._update_quote_snapshot(
            instrument_id=instrument_id,
            bid=bid,
            ask=ask,
            ts_ns=ts_ns,
        )
        if bid is not None and ask is not None:
            self._publish_market_bbo(
                instrument_id=instrument_id,
                bid=bid,
                ask=ask,
                ts_ns=ts_ns,
            )
        self._publish_balances_if_due()
        self._publish_state_snapshot(now_ns=ts_ns)

    def record_maker_fill(
        self,
        *,
        fill: MakerFill,
        quote: IbkrQuoteSnapshot,
        maker_fee_bps: Decimal,
    ) -> HedgeOrderIntent | None:
        fill_id = str(fill.fill_id).strip()
        if not fill_id:
            raise ValueError("`fill.fill_id` must be non-empty")
        if fill_id in self._seen_fill_ids:
            return None
        self._remember_fill_id(fill_id)

        quote_error = validate_ibkr_quote(
            bid=quote.bid,
            ask=quote.ask,
            quote_age_ms=quote.age_ms,
            max_quote_age_ms=int(getattr(self.config, "max_ibkr_quote_age_ms", 1_000)),
            max_spread_bps=Decimal(str(getattr(self.config, "max_ibkr_spread_bps", "25"))),
        )
        if quote_error is not None:
            self._disable_hedging(quote_error)
            return None

        fee_rules = fees_mod.resolve_fee_rules(
            runtime_params=self._runtime_params,
            maker_fee_bps=maker_fee_bps,
            fee_snapshot_age_s=Decimal(str(max(quote.age_ms, 0))) / Decimal("1000"),
        )
        hedge_side = "SELL" if str(fill.side).strip().upper() == "BUY" else "BUY"
        hedge_qty = abs(
            translate_hyperliquid_fill_to_ibkr_shares(
                fill_qty=fill.qty,
                min_share_increment=Decimal(
                    str(getattr(self.config, "hedge_min_share_increment", Decimal("1")))
                ),
            )
        )
        if hedge_qty <= 0:
            self._disable_hedging("hedge_qty_rounds_to_zero")
            return None

        limit_price = build_ibkr_ioc_limit(
            side=hedge_side,
            bid=quote.bid,
            ask=quote.ask,
            cross_mid_bps=Decimal(str(self._runtime_params["hedge_ioc_cross_mid_bps"])),
            tick_size=Decimal(str(getattr(self.config, "hedge_price_tick_size", Decimal("0.01")))),
            quote_age_ms=quote.age_ms,
            max_quote_age_ms=int(getattr(self.config, "max_ibkr_quote_age_ms", 1_000)),
            max_spread_bps=Decimal(str(getattr(self.config, "max_ibkr_spread_bps", "25"))),
        )
        if limit_price is None:
            self._disable_hedging("invalid_hedge_limit")
            return None

        order = HedgeOrderIntent(
            instrument_id=str(self.config.reference_instrument_id),
            side=hedge_side,
            qty=hedge_qty,
            limit_price=limit_price,
            time_in_force="IOC",
            outside_rth=bool(getattr(self.config, "outside_rth_hedge_enabled", False)),
        )
        self._hedge_requests.append(order)
        self._pending_hedge = PendingHedgeState(
            fill_id=fill_id,
            side=hedge_side,
            requested_qty=hedge_qty,
            remaining_qty=hedge_qty,
            limit_price=limit_price,
            outside_rth=order.outside_rth,
        )
        # Fee rules are resolved here to fail closed before any hedge attempt.
        _ = fee_rules
        return order

    def apply_hedge_execution(self, report: HedgeExecutionReport) -> None:
        if self._pending_hedge is None:
            return

        if not report.ok or report.filled_qty <= 0:
            self._disable_hedging(report.error or "hedge_failed")
            return

        remaining = self._pending_hedge.remaining_qty - report.filled_qty
        if remaining > 0:
            self._pending_hedge = PendingHedgeState(
                fill_id=self._pending_hedge.fill_id,
                side=self._pending_hedge.side,
                requested_qty=self._pending_hedge.requested_qty,
                remaining_qty=remaining,
                limit_price=self._pending_hedge.limit_price,
                outside_rth=self._pending_hedge.outside_rth,
                order_id=report.order_id,
            )
            self._disable_hedging("partial_hedge_fill")
            return

        self._pending_hedge = None

    def snapshot_state(self) -> dict[str, object]:
        snapshot: dict[str, object] = {
            "tradeable": self.tradeable,
            "hedge_disabled_reason": self.hedge_disabled_reason,
            "last_fill_ids_head": list(self._fill_ids_head),
        }
        if self._pending_hedge is not None:
            snapshot["pending_hedge"] = asdict(self._pending_hedge)
        return snapshot

    def restore_state(self, snapshot: dict[str, object]) -> None:
        self.tradeable = bool(snapshot.get("tradeable", True))
        reason = snapshot.get("hedge_disabled_reason")
        self.hedge_disabled_reason = None if reason is None else str(reason)
        fill_ids = snapshot.get("last_fill_ids_head")
        if isinstance(fill_ids, list):
            self._fill_ids_head = [str(value) for value in fill_ids if str(value).strip()]
            self._seen_fill_ids = set(self._fill_ids_head)
        pending = snapshot.get("pending_hedge")
        if isinstance(pending, dict):
            self._pending_hedge = PendingHedgeState(
                fill_id=str(pending.get("fill_id", "")),
                side=str(pending.get("side", "")),
                requested_qty=Decimal(str(pending.get("requested_qty", "0"))),
                remaining_qty=Decimal(str(pending.get("remaining_qty", "0"))),
                limit_price=Decimal(str(pending.get("limit_price", "0"))),
                outside_rth=bool(pending.get("outside_rth", False)),
                order_id=(
                    None
                    if pending.get("order_id") in (None, "")
                    else str(pending.get("order_id"))
                ),
            )

    def _publish_market_bbo(
        self,
        *,
        instrument_id: Any,
        bid: Decimal,
        ask: Decimal,
        ts_ns: int,
    ) -> None:
        makerv3_publisher_mod.publish_market_bbo(
            self,
            instrument_id=instrument_id,
            bid=bid,
            ask=ask,
            ts_ns=ts_ns,
        )

    def _publish_balances_if_due(self) -> None:
        makerv3_publisher_mod.publish_balances_if_due(self)

    def _publish_balances(self) -> None:
        makerv3_publisher_mod.publish_balances(self)

    def _publish_json(self, topic: str, payload: dict[str, Any] | list[Any]) -> None:
        makerv3_publisher_mod.publish_json(self, topic, payload)


__all__ = [
    "MakerV4Strategy",
    "MakerV4StrategyConfig",
]
