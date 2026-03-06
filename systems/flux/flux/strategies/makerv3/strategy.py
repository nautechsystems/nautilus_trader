"""
Implement the canonical production MakerV3 quoting strategy.
"""

from __future__ import annotations

from collections.abc import Mapping
from contextlib import suppress
from datetime import timedelta
from decimal import Decimal
from typing import TYPE_CHECKING
from typing import Any

from flux.common.keys import FluxRedisKeys
from flux.common.portfolio_inventory import DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
from flux.common.portfolio_inventory import StrategyInventoryComponent
from flux.common.portfolio_inventory import decode_portfolio_inventory
from flux.common.portfolio_inventory import encode_component
from flux.strategies.makerv3 import failures as failures_mod
from flux.strategies.makerv3 import inventory as inventory_mod
from flux.strategies.makerv3 import managed_orders as managed_orders_mod
from flux.strategies.makerv3 import market_data as market_data_mod
from flux.strategies.makerv3 import pricing as pricing_mod
from flux.strategies.makerv3 import publisher as publisher_mod
from flux.strategies.makerv3 import rebalancing as rebalancing_mod
from flux.strategies.makerv3 import runtime_params as runtime_params_mod
from flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_NAME
from flux.strategies.makerv3.constants import (
    ALERT_COOLDOWN_ORDER_REJECTED_BURST_MS,
)
from flux.strategies.makerv3.constants import ALERT_KEY_ORDER_REJECTED_BURST
from flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE
from flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_MD_STALE
from flux.strategies.makerv3.constants import REASON_BLOCKED_REFERENCE_MD_STALE
from flux.strategies.makerv3.constants import TOPIC_FV
from flux.strategies.makerv3.constants import TOPIC_TRADE
from flux.strategies.makerv3.wire import build_quote_cycle_envelope
from flux.strategies.makerv3.wire import build_quote_cycle_id


if TYPE_CHECKING:
    from nautilus_trader.accounting.accounts.base import Account
    from nautilus_trader.model.identifiers import ClientOrderId
    from nautilus_trader.model.objects import Price
    from nautilus_trader.model.orders import Order
    from nautilus_trader.model.position import Position


_to_decimal = pricing_mod.to_decimal
_to_decimal_or_none = pricing_mod.to_decimal_or_none
_to_int_or_default = pricing_mod.to_int_or_default


_decimal_to_json_str = publisher_mod.decimal_to_json_str


_clamp_decimal = pricing_mod.clamp_decimal
_bps_to_price_offset = pricing_mod.bps_to_price_offset
_price_to_decimal = pricing_mod.price_to_decimal
_round_price_to_tick = pricing_mod.round_price_to_tick
_clamp_post_only_price = pricing_mod.clamp_post_only_price
_nudge_unique_price = pricing_mod.nudge_unique_price
_apply_inventory_skew_to_edges = pricing_mod.apply_inventory_skew_to_edges


def _did_bot_turn_off(previous_bot_on: bool, current_bot_on: bool) -> bool:
    return bool(previous_bot_on) and (not bool(current_bot_on))


def _normalized_reject_reason(reason: Any) -> str:
    text = str(reason or "").strip()
    return text or "unknown"


_validate_three_band_input = pricing_mod.validate_three_band_input
build_ladder_targets = pricing_mod.build_ladder_targets
build_ladder_place_cancel_levels = pricing_mod.build_ladder_place_cancel_levels
build_ladder_place_cancel_levels_from_bps = pricing_mod.build_ladder_place_cancel_levels_from_bps
plan_side_rebalance_actions = rebalancing_mod.plan_side_rebalance_actions


_NAUTILUS_IMPORT_ERROR: ModuleNotFoundError | None = None
try:
    from nautilus_trader.config import NonNegativeFloat
    from nautilus_trader.config import NonNegativeInt
    from nautilus_trader.config import PositiveInt
    from nautilus_trader.config import StrategyConfig
    from nautilus_trader.model.book import OrderBook
    from nautilus_trader.model.data import OrderBookDeltas
    from nautilus_trader.model.enums import BookType
    from nautilus_trader.model.enums import OrderSide
    from nautilus_trader.model.events import OrderFilled
    from nautilus_trader.model.identifiers import InstrumentId
    from nautilus_trader.model.instruments import Instrument
    from nautilus_trader.model.objects import Quantity
    from nautilus_trader.trading.strategy import Strategy
except ModuleNotFoundError as e:  # pragma: no cover - pure-math test fallback
    _NAUTILUS_IMPORT_ERROR = e


if _NAUTILUS_IMPORT_ERROR is None:
    from flux.strategies.makerv3 import quote_engine as quote_engine_mod

    class MakerV3StrategyConfig(StrategyConfig, frozen=True):
        """
        Define runtime configuration for `MakerV3Strategy`.
        """

        maker_instrument_id: InstrumentId
        reference_instrument_id: InstrumentId
        order_qty: Decimal
        external_strategy_id: str = "makerv3"
        bot_on: bool | None = None
        qty: Decimal | None = None
        des_qty_global: NonNegativeFloat | None = None
        max_qty_global: NonNegativeFloat | None = None
        max_skew_bps_global: NonNegativeFloat | None = None
        des_qty_local: NonNegativeFloat | None = None
        max_qty_local: NonNegativeFloat | None = None
        max_skew_bps_local: NonNegativeFloat | None = None
        linear_offset_bps: NonNegativeFloat | None = None
        max_age_ms: PositiveInt | None = None
        bid_edge1: NonNegativeFloat | None = None
        ask_edge1: NonNegativeFloat | None = None
        place_edge1: NonNegativeFloat | None = None
        distance1: NonNegativeFloat | None = None
        n_orders1: NonNegativeInt | None = None
        bid_edge2: NonNegativeFloat | None = None
        ask_edge2: NonNegativeFloat | None = None
        place_edge2: NonNegativeFloat | None = None
        distance2: NonNegativeFloat | None = None
        n_orders2: NonNegativeInt | None = None
        bid_edge3: NonNegativeFloat | None = None
        ask_edge3: NonNegativeFloat | None = None
        place_edge3: NonNegativeFloat | None = None
        distance3: NonNegativeFloat | None = None
        n_orders3: NonNegativeInt | None = None
        order_reject_alert_after_count: NonNegativeInt | None = None
        order_reject_alert_after_s: NonNegativeFloat | None = None
        quote_fail_critical_after_count: NonNegativeInt | None = None
        quote_fail_critical_after_s: NonNegativeFloat | None = None
        cancel_all_instrument_orders: bool = False

        @property
        def active_order_qty(self) -> Decimal:
            """
            Return configured quote quantity with runtime override fallback.
            """
            return self.qty if self.qty is not None else self.order_qty

    class MakerV3Strategy(Strategy):
        """
        Run the MakerV3 single-leg quoting strategy orchestration.
        """

        INTERNAL_REQUOTE_THROTTLE_MS = 150
        STALE_CANCEL_COOLDOWN_MS = 1_000
        BALANCES_PUBLISH_INTERVAL_MS = 10_000
        STALE_CANCELS_PER_SIDE_PER_CYCLE = 1
        PARAMS_REFRESH_INTERVAL_MS = 500
        MARKET_BBO_HEARTBEAT_MS = 1_000
        INVENTORY_SKEW_CACHE_TTL_MS = 100

        def __init__(self, config: MakerV3StrategyConfig) -> None:
            """
            Initialize strategy state for lifecycle, pricing, and risk gates.
            """
            super().__init__(config)
            self._maker_instrument: Instrument | None = None
            self._order_qty: Quantity | None = None
            self._price_precision: int = 8
            self._books: dict[InstrumentId, OrderBook] = {}
            self._last_bbo: dict[InstrumentId, tuple[Decimal, Decimal] | None] = {}
            self._last_bbo_ts_ns: dict[InstrumentId, int] = {}
            self._last_market_bbo_publish_ns: dict[InstrumentId, int] = {}
            self._last_requote_ns = 0
            self._last_fv: Decimal | None = None
            self._last_fv_snapshot_ts_ns = 0
            self._last_state_ns = 0
            self._last_balances_ns = 0
            self._last_pricing_debug: dict[str, Any] = {}
            self._last_bot_on = bool(self.config.bot_on)
            configured_strategy_id = (
                self.config.external_strategy_id.strip() if self.config.external_strategy_id else ""
            )
            self._strategy_identity = configured_strategy_id or str(self.id)
            self._external_strategy_id = self._strategy_identity
            self._runtime_params = runtime_params_mod.initial_runtime_params(self.config)
            self._params_manager: Any | None = None
            self._params_manager_factory: Any | None = None
            self._last_params_refresh_ns = 0
            self._params_timer_name = f"maker-v3-params-refresh:{self._strategy_identity}"
            self._runtime_params_failed = False
            self._instruments: dict[InstrumentId, Instrument] = {}
            self._managed_client_order_ids: set[str] = set()
            self._order_rejections_ns_by_reason: dict[str, list[int]] = {}
            self._quote_failures_ns: list[int] = []
            self._quote_failure_circuit_open = False
            self._last_stale_cancel_ns = 0
            self._last_state_name: str | None = None
            self._state_is_blocked = False
            self._inventory_skew_cache = inventory_mod.InventorySkewCache(
                ttl_ms=self.INVENTORY_SKEW_CACHE_TTL_MS,
            )
            self._run_id = self._strategy_identity
            self._quote_cycle_seq = 0
            self._last_actionable_alert_ns: dict[str, int] = {}
            self._last_actionable_alert_transition: dict[str, str] = {}
            self._portfolio_inventory_client: Any | None = None
            self._portfolio_inventory_portfolio_id: str | None = None
            self._portfolio_inventory_namespace = "flux"
            self._portfolio_inventory_schema_version = "v1"
            self._portfolio_inventory_stale_after_ms = (
                DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
            )

        def on_start(self) -> None:
            """
            Start subscriptions, timers, and initial strategy publications.
            """
            self._runtime_params_failed = False
            self._quote_failure_circuit_open = False
            self._order_rejections_ns_by_reason.clear()
            self._quote_failures_ns.clear()
            self._last_stale_cancel_ns = 0
            self._last_state_name = None
            self._state_is_blocked = False
            self._last_actionable_alert_ns.clear()
            self._last_actionable_alert_transition.clear()
            self._last_bot_on = self._runtime_bool("bot_on")
            start_ns = int(self.clock.timestamp_ns())
            self._run_id = f"{self._strategy_identity}:{start_ns // 1_000_000}"
            self._quote_cycle_seq = 0
            instrument_id = self.config.maker_instrument_id
            self._maker_instrument = self.cache.instrument(instrument_id)
            if self._maker_instrument is None:
                self._publish_alert(f"Could not find instrument for {instrument_id}")
                self.stop()
                return

            reference_instrument = self.cache.instrument(self.config.reference_instrument_id)
            if reference_instrument is None:
                self._publish_alert(
                    f"Could not find instrument for {self.config.reference_instrument_id}",
                    level="error",
                )
                self.stop()
                return
            self._instruments = {
                self.config.maker_instrument_id: self._maker_instrument,
                self.config.reference_instrument_id: reference_instrument,
            }

            try:
                self._order_qty = self._maker_instrument.make_qty(self.config.active_order_qty)
            except ValueError:
                self._publish_alert(
                    f"Invalid order quantity configured for {instrument_id}",
                    level="error",
                )
                self.stop()
                return
            try:
                self._refresh_runtime_params(force=True)
            except Exception as e:
                self._fail_fast_runtime_params(context="on_start", exc=e)
                return
            self._last_bot_on = self._effective_bot_on()
            self.clock.set_timer(
                name=self._params_timer_name,
                interval=timedelta(milliseconds=self.PARAMS_REFRESH_INTERVAL_MS),
                callback=self.on_time_event,
            )
            self._price_precision = self._maker_instrument.price_precision

            subscribed_instrument_ids: list[InstrumentId] = []
            for instrument_id in (
                self.config.maker_instrument_id,
                self.config.reference_instrument_id,
            ):
                if instrument_id in subscribed_instrument_ids:
                    continue
                subscribed_instrument_ids.append(instrument_id)

            self._books = {
                instrument_id: OrderBook(
                    instrument_id=instrument_id,
                    book_type=BookType.L2_MBP,
                )
                for instrument_id in subscribed_instrument_ids
            }
            self._last_bbo = dict.fromkeys(self._books)
            self._last_bbo_ts_ns = dict.fromkeys(self._books, 0)
            self._last_market_bbo_publish_ns = dict.fromkeys(self._books, 0)

            for instrument_id in subscribed_instrument_ids:
                self.subscribe_order_book_deltas(
                    instrument_id=instrument_id,
                    book_type=BookType.L2_MBP,
                )

            self._publish_event("started")
            self._publish_balances()
            self._publish_portfolio_inventory_component(state="on_start")
            self._publish_state("on_start")
            self.log.info(
                f"MakerV3 strategy started strategy_id={self._external_strategy_id} "
                f"maker={self.config.maker_instrument_id} reference={self.config.reference_instrument_id}",
            )

        def on_stop(self) -> None:
            """
            Stop quoting, cancel managed orders, and publish terminal state.
            """
            self._cancel_managed_quotes("on_stop", force=True)
            timer_names: set[str] = set()
            try:
                timer_names = set(self.clock.timer_names)
            except Exception:
                timer_names = set()
            if self._params_timer_name in timer_names:
                self.clock.cancel_timer(self._params_timer_name)
            unsubscribed_instrument_ids: list[InstrumentId] = []
            for instrument_id in (
                self.config.maker_instrument_id,
                self.config.reference_instrument_id,
            ):
                if instrument_id in unsubscribed_instrument_ids:
                    continue
                unsubscribed_instrument_ids.append(instrument_id)
                self.unsubscribe_order_book_deltas(instrument_id=instrument_id)
            self._publish_portfolio_inventory_component(state="on_stop")
            self._publish_state("on_stop")
            self.log.info(
                f"MakerV3 strategy stopped strategy_id={self._external_strategy_id}",
            )

        def on_time_event(self, event: Any) -> None:
            """
            Handle periodic runtime-parameter refresh timer events.
            """
            if getattr(event, "name", "") != self._params_timer_name:
                return

            if self._runtime_params_failed:
                return

            now_ns = int(self.clock.timestamp_ns())
            try:
                self._refresh_runtime_params(now_ns=now_ns)
            except Exception as e:
                self._fail_fast_runtime_params(context="on_time_event", exc=e)
                return
            self._publish_balances_if_due()
            self._publish_portfolio_inventory_component()
            bot_on_now = self._effective_bot_on()
            if _did_bot_turn_off(self._last_bot_on, bot_on_now):
                self._cancel_managed_quotes("bot_off_flip", force=True)
                self._publish_state("bot_off")
            self._last_bot_on = bot_on_now
            if bot_on_now:
                self._enforce_stale_market_data(now_ns=now_ns)

        def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
            """
            Process market deltas and trigger quote-cycle refresh when eligible.
            """
            market_data_mod.on_order_book_deltas(self, deltas)

        def _enforce_stale_market_data(self, *, now_ns: int) -> None:
            """
            Enforce stale market-data quote blocks even when deltas go silent.
            """
            if self._quote_failure_circuit_open:
                return

            tracked = self._tracked_managed_order_count()
            if tracked <= 0 and not self._managed_orders():
                return
            cooldown_ns = self.STALE_CANCEL_COOLDOWN_MS * 1_000_000
            cooldown_due = (
                self._last_stale_cancel_ns <= 0
                or now_ns - self._last_stale_cancel_ns >= cooldown_ns
            )
            blocked_transition = not bool(getattr(self, "_state_is_blocked", False))
            if not blocked_transition and not cooldown_due:
                return

            max_age_ms = self._runtime_int("max_age_ms")
            max_age_ns = max_age_ms * 1_000_000

            maker_ts_ns = int(self._last_bbo_ts_ns.get(self.config.maker_instrument_id, 0) or 0)
            if maker_ts_ns <= 0:
                self._handle_stale_quote_block(
                    now_ns=now_ns,
                    state="blocked_maker_md",
                    cancel_reason="maker_md_stale",
                    reason_code=REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE,
                    quote_cycle_id=self._next_quote_cycle_id(now_ns=now_ns),
                    warning_message=(
                        "Quoting blocked (maker book unavailable) "
                        f"strategy_id={self._external_strategy_id}"
                    ),
                )
                return
            maker_age_ns = now_ns - maker_ts_ns
            if maker_age_ns >= max_age_ns:
                age_ms = int(maker_age_ns / 1_000_000)
                self._handle_stale_quote_block(
                    now_ns=now_ns,
                    state="blocked_maker_md",
                    cancel_reason="maker_md_stale",
                    reason_code=REASON_BLOCKED_MAKER_MD_STALE,
                    quote_cycle_id=self._next_quote_cycle_id(now_ns=now_ns),
                    warning_message=(
                        "Quoting blocked (maker data stale) "
                        f"strategy_id={self._external_strategy_id} "
                        f"age_ms={age_ms} max_age_ms={max_age_ms}"
                    ),
                )
                return

            reference_ts_ns = int(
                self._last_bbo_ts_ns.get(self.config.reference_instrument_id, 0) or 0,
            )
            reference_age_ns = now_ns - reference_ts_ns if reference_ts_ns > 0 else None
            if reference_age_ns is None or reference_age_ns >= max_age_ns:
                reference_age_ms = (
                    int(reference_age_ns / 1_000_000) if reference_age_ns is not None else None
                )
                self._handle_stale_quote_block(
                    now_ns=now_ns,
                    state="blocked_reference_md",
                    cancel_reason="reference_md_stale",
                    reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
                    quote_cycle_id=self._next_quote_cycle_id(now_ns=now_ns),
                    warning_message=(
                        "Quoting blocked (reference data stale) "
                        f"strategy_id={self._external_strategy_id} "
                        f"age_ms={reference_age_ms} max_age_ms={max_age_ms}"
                    ),
                )

        def _effective_bot_on(self) -> bool:
            return runtime_params_mod.effective_bot_on(self)

        def _runtime_decimal(self, name: str) -> Decimal:
            return runtime_params_mod.runtime_decimal(self, name)

        def _runtime_int(self, name: str) -> int:
            return runtime_params_mod.runtime_int(self, name)

        def _runtime_bool(self, name: str) -> bool:
            return runtime_params_mod.runtime_bool(self, name)

        def _quote_runtime_params_snapshot(self) -> dict[str, Any]:
            return runtime_params_mod.quote_runtime_params_snapshot(self)

        def _invalidate_inventory_skew_cache(self) -> None:
            self._inventory_skew_cache.invalidate()

        def _cached_inventory_skew(
            self,
            *,
            now_ns: int,
            runtime_params: Mapping[str, Any],
        ) -> dict[str, Any]:
            self._inventory_skew_cache.set_ttl_ms(self.INVENTORY_SKEW_CACHE_TTL_MS)
            return self._inventory_skew_cache.get(
                now_ns=now_ns,
                runtime_params=runtime_params,
                compute=lambda params: self._compute_inventory_skew(runtime_params=params),
            )

        def _tracked_managed_order_count(self) -> int:
            return len(getattr(self, "_managed_client_order_ids", set()))

        @property
        def runtime_strategy_id(self) -> str:
            """
            Return the authoritative strategy identity for runtime wiring.
            """
            return self._strategy_identity

        @staticmethod
        def params_manager_factory(
            *,
            redis_client: Any,
            namespace: str = "flux",
            schema_version: str = "v1",
            defaults: Mapping[str, Any] | None = None,
        ) -> Any:
            """
            Build a params-manager factory bound to the MakerV3 runtime schema.
            """
            return runtime_params_mod.params_manager_factory(
                redis_client=redis_client,
                namespace=namespace,
                schema_version=schema_version,
                defaults=defaults,
            )

        def set_params_manager(self, manager: Any | None) -> None:
            """
            Attach an explicit runtime params manager instance.
            """
            runtime_params_mod.set_params_manager(self, manager)

        def set_params_manager_factory(self, factory: Any | None) -> None:
            """
            Attach a lazy factory used to construct a params manager on demand.
            """
            runtime_params_mod.set_params_manager_factory(self, factory)

        def _ensure_params_manager_identity(self, manager: Any | None) -> None:
            runtime_params_mod.ensure_params_manager_identity(self, manager)

        def _ensure_params_manager(self) -> Any | None:
            return runtime_params_mod.ensure_params_manager(self)

        def _apply_runtime_param_updates(self, updates: dict[str, Any]) -> None:
            runtime_params_mod.apply_runtime_param_updates(self, updates)

        def _refresh_runtime_params(
            self,
            *,
            now_ns: int | None = None,
            force: bool = False,
        ) -> None:
            runtime_params_mod.refresh_runtime_params(self, now_ns=now_ns, force=force)

        def _fail_fast_runtime_params(self, *, context: str, exc: Exception) -> None:
            runtime_params_mod.fail_fast_runtime_params(self, context=context, exc=exc)

        def _handle_quote_failure(self, *, now_ns: int, exc: Exception, context: str) -> None:
            failures_mod.handle_quote_failure(self, now_ns=now_ns, exc=exc, context=context)

        def on_order_filled(self, event: OrderFilled) -> None:
            """
            Handle order fill events and reconcile managed order tracking.
            """
            self._invalidate_inventory_skew_cache()
            self._publish_portfolio_inventory_component(
                state=self._last_state_name or "running",
                now_ms_value=int(int(event.ts_event) // 1_000_000),
            )
            self._publish_json(
                TOPIC_TRADE,
                {
                    "strategy_id": self._external_strategy_id,
                    "event": "order_filled",
                    "instrument_id": str(event.instrument_id),
                    "client_order_id": str(event.client_order_id),
                    "trade_id": str(event.trade_id),
                    "side": str(event.order_side),
                    "qty": str(event.last_qty),
                    "price": str(event.last_px),
                    "ts_event": int(event.ts_event),
                },
            )
            self._reconcile_managed_order(event.client_order_id, lifecycle="filled")

        def on_order_rejected(self, event: Any) -> None:
            """
            Handle order rejection events and reconcile managed tracking.
            """
            self._invalidate_inventory_skew_cache()
            reason = _normalized_reject_reason(getattr(event, "reason", None))
            self._reconcile_managed_order(
                getattr(event, "client_order_id", None),
                lifecycle="rejected",
                instrument_id=getattr(event, "instrument_id", None),
                reason=reason,
                due_post_only=getattr(event, "due_post_only", None),
            )
            self.log.warning(
                f"Order rejected strategy_id={self._external_strategy_id} "
                f"client_order_id={getattr(event, 'client_order_id', None)} "
                f"reason={reason}",
            )
            now_ns = getattr(event, "ts_event", None)
            if now_ns is None:
                with suppress(Exception):
                    now_ns = int(self.clock.timestamp_ns())
            if now_ns is not None:
                self._track_order_rejection_alert(now_ns=int(now_ns), reason=reason)

        def on_order_canceled(self, event: Any) -> None:
            """
            Handle order cancel events and reconcile managed tracking.
            """
            self._invalidate_inventory_skew_cache()
            self._reconcile_managed_order(
                getattr(event, "client_order_id", None),
                lifecycle="canceled",
            )

        def on_order_expired(self, event: Any) -> None:
            """
            Handle order expiry events and reconcile managed tracking.
            """
            self._invalidate_inventory_skew_cache()
            self._reconcile_managed_order(
                getattr(event, "client_order_id", None),
                lifecycle="expired",
            )

        def _reconcile_managed_order(
            self,
            client_order_id: ClientOrderId | str | None,
            *,
            lifecycle: str,
            instrument_id: Any | None = None,
            reason: str | None = None,
            due_post_only: bool | None = None,
        ) -> None:
            had_order = managed_orders_mod.reconcile_managed_order(
                self._managed_client_order_ids,
                client_order_id,
            )
            client_order_id_str = str(client_order_id or "")
            if not client_order_id_str:
                return
            event_payload: dict[str, Any] = {
                "lifecycle": lifecycle,
                "client_order_id": client_order_id_str,
                "tracked_before": had_order,
                "tracked_after": len(self._managed_client_order_ids),
            }
            if instrument_id is not None:
                event_payload["instrument_id"] = str(instrument_id)
            if reason is not None:
                event_payload["reason"] = reason
            if due_post_only is not None:
                event_payload["due_post_only"] = bool(due_post_only)
            self._publish_event("order_lifecycle", **event_payload)

        def _track_order_rejection_alert(self, *, now_ns: int, reason: str) -> None:
            count_threshold = max(0, int(self._runtime_int("order_reject_alert_after_count")))
            if count_threshold <= 0:
                return

            window_seconds = max(Decimal(0), self._runtime_decimal("order_reject_alert_after_s"))
            window_ns = int(window_seconds * Decimal(1_000_000_000))
            reason_key = _normalized_reject_reason(reason)
            reason_rejections = list(self._order_rejections_ns_by_reason.get(reason_key, ()))
            reason_rejections.append(now_ns)
            if window_ns > 0:
                cutoff_ns = now_ns - window_ns
                reason_rejections = [ts_ns for ts_ns in reason_rejections if ts_ns >= cutoff_ns]
            elif count_threshold > 0:
                reason_rejections = reason_rejections[-count_threshold:]
            self._order_rejections_ns_by_reason[reason_key] = reason_rejections

            rejection_count = len(reason_rejections)
            if rejection_count < count_threshold:
                return

            self._publish_actionable_alert(
                alert_key=ALERT_KEY_ORDER_REJECTED_BURST,
                message=(
                    "order_rejected_burst "
                    f"reason={reason_key!r} count={rejection_count} "
                    f"threshold={count_threshold} window_s={window_seconds}"
                ),
                level="error",
                reason_code=ALERT_KEY_ORDER_REJECTED_BURST,
                cooldown_ms=ALERT_COOLDOWN_ORDER_REJECTED_BURST_MS,
                transition=reason_key,
                now_ns=now_ns,
            )

        def _inventory_cache(self) -> Any | None:
            cache = getattr(self, "_cache", None)
            if cache is None:
                cache = getattr(self, "cache", None)
            return cache

        def _resolve_instrument(self, instrument_id: Any) -> Any | None:
            instrument = self._instruments.get(instrument_id)
            if instrument is not None:
                return instrument
            cache = self._inventory_cache()
            instrument_lookup = getattr(cache, "instrument", None)
            if callable(instrument_lookup):
                with suppress(Exception):
                    return instrument_lookup(instrument_id)
            return None

        def _open_positions(self) -> list[Position] | None:
            cache = self._inventory_cache()
            positions_open = getattr(cache, "positions_open", None)
            if not callable(positions_open):
                return None
            with suppress(Exception):
                return list(positions_open())
            return None

        def _position_inventory_qty(
            self,
            currency_code: str,
            *,
            venue: Any | None = None,
        ) -> Decimal | None:
            if not currency_code:
                return None
            positions = self._open_positions()
            if positions is None:
                return None
            cache = self._inventory_cache()
            quantity = inventory_mod.position_inventory_total(
                positions,
                base_currency=currency_code,
                instrument_lookup=self._resolve_instrument if cache is not None else None,
                venue=venue,
            )
            return Decimal(0) if quantity is None else quantity

        def _spot_balance_total(
            self,
            currency_code: str,
            *,
            venue: Any | None = None,
        ) -> Decimal | None:
            if not currency_code:
                return None

            accounts: list[Account] = []
            cache = self._inventory_cache()
            if venue is not None:
                account_for_venue = getattr(cache, "account_for_venue", None)
                if callable(account_for_venue):
                    with suppress(Exception):
                        scoped_account = account_for_venue(venue=venue)
                        if scoped_account is not None:
                            return inventory_mod.spot_balance_total(
                                accounts=[scoped_account],
                                currency_code=currency_code,
                            )
                if hasattr(self, "portfolio"):
                    with suppress(Exception):
                        scoped_account = self.portfolio.account(venue=venue)
                        if scoped_account is not None:
                            return inventory_mod.spot_balance_total(
                                accounts=[scoped_account],
                                currency_code=currency_code,
                            )

            accounts_lookup = getattr(cache, "accounts", None)
            if callable(accounts_lookup):
                with suppress(Exception):
                    accounts.extend(list(accounts_lookup()))

            if not accounts and hasattr(self, "portfolio"):
                maker_venue = getattr(self.config.maker_instrument_id, "venue", None)
                fallback_venue = maker_venue if venue is None else venue
                with suppress(Exception):
                    account = (
                        self.portfolio.account(venue=fallback_venue)
                        if fallback_venue is not None
                        else None
                    )
                    if account is not None:
                        accounts.append(account)
            scoped_accounts = accounts
            if venue is not None:
                venue_code = str(venue).strip().upper()
                scoped_accounts = [
                    account
                    for account in accounts
                    if inventory_mod.account_venue_code(account) == venue_code
                ]
            total = inventory_mod.spot_balance_total(
                accounts=scoped_accounts,
                currency_code=currency_code,
            )
            if total is None and scoped_accounts:
                return Decimal(0)
            return total

        def _maker_base_currency_code(self) -> str | None:
            instrument = self._maker_instrument
            if instrument is None:
                instrument = self._instruments.get(self.config.maker_instrument_id)
            return inventory_mod.maker_base_currency_code(
                instrument=instrument,
                instrument_id=self.config.maker_instrument_id,
            )

        def configure_portfolio_inventory_feed(
            self,
            *,
            redis_client: Any,
            portfolio_id: str,
            namespace: str,
            schema_version: str,
            stale_after_ms: int = DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS,
        ) -> None:
            self._portfolio_inventory_client = redis_client
            self._portfolio_inventory_portfolio_id = portfolio_id.strip() or None
            self._portfolio_inventory_namespace = namespace
            self._portfolio_inventory_schema_version = schema_version
            self._portfolio_inventory_stale_after_ms = max(1, int(stale_after_ms))

        def _maker_instrument_is_spot(self) -> bool:
            instrument_id_text = str(self.config.maker_instrument_id).upper()
            if "-SPOT." in instrument_id_text:
                return True
            if any(
                suffix in instrument_id_text
                for suffix in ("-PERP.", "-SWAP.", "-LINEAR.", "-INVERSE.")
            ):
                return False
            venue_text = str(getattr(self.config.maker_instrument_id, "venue", "")).upper()
            if venue_text.endswith("_SPOT"):
                return True
            return "." in instrument_id_text

        def _maker_local_position_qty(self, currency_code: str | None) -> Decimal | None:
            if not currency_code or self._maker_instrument_is_spot():
                return None
            positions = self._open_positions()
            if positions is None:
                return None
            total = Decimal(0)
            found = False
            for position in positions:
                if getattr(position, "instrument_id", None) != self.config.maker_instrument_id:
                    continue
                signed_qty = inventory_mod.position_signed_qty([position])
                if signed_qty is None:
                    continue
                total += signed_qty
                found = True
            return total if found else Decimal(0)

        def _maker_local_spot_qty(self, currency_code: str | None) -> Decimal | None:
            if not currency_code or not self._maker_instrument_is_spot():
                return None
            maker_venue = getattr(self.config.maker_instrument_id, "venue", None)
            return self._spot_balance_total(currency_code, venue=maker_venue)

        def _portfolio_global_inventory_qty(self, base_currency: str | None) -> Decimal | None:
            portfolio_id = self._portfolio_inventory_portfolio_id
            client = self._portfolio_inventory_client
            if not base_currency or not portfolio_id or client is None:
                return None
            key = FluxRedisKeys.portfolio_inventory(
                portfolio_id=portfolio_id,
                base_currency=base_currency,
                namespace=self._portfolio_inventory_namespace,
                schema_version=self._portfolio_inventory_schema_version,
            )
            with suppress(Exception):
                payload = decode_portfolio_inventory(client.get(key))
                if not isinstance(payload, dict):
                    return None
                ts_ms = int(payload.get("ts_ms") or 0)
                stale_after_ms = int(
                    payload.get("stale_after_ms") or self._portfolio_inventory_stale_after_ms,
                )
                now_ms_value = int(self.clock.timestamp_ns() // 1_000_000)
                if ts_ms <= 0 or now_ms_value - ts_ms > max(1, stale_after_ms):
                    return None
                if payload.get("missing_required"):
                    return None
                return _to_decimal_or_none(payload.get("global_qty"))
            return None

        def _publish_portfolio_inventory_component(
            self,
            *,
            state: str | None = None,
            now_ms_value: int | None = None,
        ) -> None:
            portfolio_id = self._portfolio_inventory_portfolio_id
            client = self._portfolio_inventory_client
            base_currency = self._maker_base_currency_code()
            if not portfolio_id or client is None or not base_currency:
                return
            ts_ms = (
                int(self.clock.timestamp_ns() // 1_000_000)
                if now_ms_value is None
                else int(now_ms_value)
            )
            local_position_qty = self._maker_local_position_qty(base_currency)
            local_spot_qty = self._maker_local_spot_qty(base_currency)
            local_qty = inventory_mod.local_inventory_total(
                local_position_qty=local_position_qty,
                local_spot_qty=local_spot_qty,
            )
            component = StrategyInventoryComponent(
                strategy_id=self._external_strategy_id,
                portfolio_id=portfolio_id,
                base_currency=base_currency,
                local_qty=local_qty,
                ts_ms=ts_ms,
                stale_after_ms=self._portfolio_inventory_stale_after_ms,
                maker_instrument_id=str(self.config.maker_instrument_id),
                state=state or (self._last_state_name or ""),
            )
            key = FluxRedisKeys.portfolio_inventory_component(
                strategy_id=self._external_strategy_id,
                portfolio_id=portfolio_id,
                base_currency=base_currency,
                namespace=self._portfolio_inventory_namespace,
                schema_version=self._portfolio_inventory_schema_version,
            )
            with suppress(Exception):
                client.set(key, encode_component(component))

        def _compute_inventory_skew(
            self,
            *,
            runtime_params: Mapping[str, Any] | None = None,
        ) -> dict[str, Any]:
            base_currency = self._maker_base_currency_code()
            portfolio_global_qty = self._portfolio_global_inventory_qty(base_currency)
            use_shared_portfolio = bool(self._portfolio_inventory_portfolio_id)
            if portfolio_global_qty is None and not use_shared_portfolio:
                global_position_qty = (
                    self._position_inventory_qty(base_currency)
                    if base_currency
                    else None
                )
                global_spot_qty = self._spot_balance_total(base_currency) if base_currency else None
                global_inventory_qty_override = None
                global_inventory_source_override = None
            else:
                global_position_qty = None
                global_spot_qty = None
                global_inventory_qty_override = portfolio_global_qty
                global_inventory_source_override = (
                    "portfolio_component_sum" if portfolio_global_qty is not None else "portfolio_unavailable"
                )
            local_position_qty = self._maker_local_position_qty(base_currency)
            local_spot_qty = self._maker_local_spot_qty(base_currency)
            if runtime_params is None:
                runtime_params = self._quote_runtime_params_snapshot()
            return inventory_mod.compute_inventory_skew(
                global_position_qty=global_position_qty,
                global_spot_qty=global_spot_qty,
                local_position_qty=local_position_qty,
                local_spot_qty=local_spot_qty,
                global_inventory_qty_override=global_inventory_qty_override,
                global_inventory_source_override=global_inventory_source_override,
                base_currency=base_currency,
                runtime_params=runtime_params,
            )

        def _next_quote_cycle_id(self, *, now_ns: int) -> str:
            del now_ns
            self._quote_cycle_seq = int(getattr(self, "_quote_cycle_seq", 0)) + 1
            return build_quote_cycle_id(
                run_id=str(getattr(self, "_run_id", self._strategy_identity)),
                quote_cycle_seq=self._quote_cycle_seq,
            )

        def _publish_quote_cycle_event(
            self,
            *,
            now_ns: int,
            quote_cycle_event: str,
            reason_code: str,
            quote_cycle_id: str,
            payload: dict[str, Any] | None = None,
        ) -> None:
            envelope = build_quote_cycle_envelope(
                run_id=str(getattr(self, "_run_id", self._strategy_identity)),
                quote_cycle_id=quote_cycle_id,
                quote_cycle_event=quote_cycle_event,
                reason_code=reason_code,
                payload=payload,
            )
            self._publish_event(
                QUOTE_CYCLE_EVENT_NAME,
                ts_ns=now_ns,
                **envelope,
            )

        def _handle_stale_quote_block(
            self,
            *,
            now_ns: int,
            state: str,
            cancel_reason: str,
            reason_code: str,
            quote_cycle_id: str,
            warning_message: str,
        ) -> None:
            quote_engine_mod.handle_stale_quote_block(
                self,
                now_ns=now_ns,
                state=state,
                cancel_reason=cancel_reason,
                reason_code=reason_code,
                quote_cycle_id=quote_cycle_id,
                warning_message=warning_message,
            )

        def _publish_recovery_state_if_blocked(
            self,
            *,
            managed_orders_count: int | None = None,
            managed_orders: list[Order] | None = None,
        ) -> None:
            quote_engine_mod.publish_recovery_state_if_blocked(
                self,
                managed_orders_count=managed_orders_count,
                managed_orders=managed_orders,
            )

        def _refresh_quotes(self, now_ns: int, *, quote_cycle_id: str | None = None) -> None:
            quote_engine_mod.refresh_quotes(
                self,
                now_ns=now_ns,
                quote_cycle_id=quote_cycle_id,
            )

        def _publish_state_if_due(self) -> None:
            publisher_mod.publish_state_if_due(self)

        def _publish_balances_if_due(self) -> None:
            publisher_mod.publish_balances_if_due(self)

        def _is_stale_order(
            self,
            order: Order,
            now_ns: int,
            *,
            max_age_ms: int | None = None,
        ) -> bool:
            age_ms = self._runtime_int("max_age_ms") if max_age_ms is None else int(max_age_ms)
            max_age_ns = age_ms * 1_000_000
            ts_init = int(getattr(order, "ts_init", 0))
            return ts_init > 0 and now_ns - ts_init >= max_age_ns

        def _rebalance_side(
            self,
            *,
            side: OrderSide,
            active_orders: list[Order],
            desired_levels: list[tuple[Price, Decimal, Decimal]],
            now_ns: int,
            max_age_ms: int,
        ) -> int:
            side_name = "buy" if side == OrderSide.BUY else "sell"
            active_prices = [_price_to_decimal(order.price) for order in active_orders]
            active_stale = [
                self._is_stale_order(order, now_ns, max_age_ms=max_age_ms)
                for order in active_orders
            ]
            desired_dec = [
                (_price_to_decimal(target_price), cancel_px, match_tol)
                for target_price, cancel_px, match_tol in desired_levels
            ]

            cancel_indices, _ = plan_side_rebalance_actions(
                side=side_name,
                active_prices=active_prices,
                active_stale=active_stale,
                desired_levels=desired_dec,
                stale_cancel_budget=self.STALE_CANCELS_PER_SIDE_PER_CYCLE,
            )

            for index in cancel_indices:
                self.cancel_order(active_orders[index])

            if cancel_indices:
                cancel_index_set = set(cancel_indices)
                active_orders[:] = [
                    order
                    for index, order in enumerate(active_orders)
                    if index not in cancel_index_set
                ]

            return len(cancel_indices)

        def _place_missing_levels(
            self,
            *,
            side: OrderSide,
            active_orders: list[Order],
            desired_levels: list[tuple[Price, Decimal, Decimal]],
            best_bid_px: Decimal,
            best_ask_px: Decimal,
        ) -> int:
            places = 0
            active_prices = [_price_to_decimal(order.price) for order in active_orders]
            for target_price, _, match_tol in desired_levels:
                target_px = _price_to_decimal(target_price)
                if side == OrderSide.BUY and target_px >= best_ask_px:
                    continue
                if side == OrderSide.SELL and target_px <= best_bid_px:
                    continue
                if any(abs(existing_px - target_px) <= match_tol for existing_px in active_prices):
                    continue
                order = self.order_factory.limit(
                    instrument_id=self.config.maker_instrument_id,
                    order_side=side,
                    quantity=self._order_qty,
                    price=target_price,
                    post_only=True,
                )
                self.submit_order(order)
                self._register_managed_order(order)
                places += 1
                active_orders.append(order)
                active_prices.append(target_px)
            return places

        def _register_managed_order(self, order: Order) -> None:
            client_order_id = managed_orders_mod.register_managed_order(
                self._managed_client_order_ids,
                order,
            )
            if client_order_id is None:
                return
            self._invalidate_inventory_skew_cache()

        def _managed_orders(self) -> list[Order]:
            return managed_orders_mod.collect_managed_orders(
                cache=self.cache,
                instrument_id=self.config.maker_instrument_id,
                strategy_id=self.id,
            )

        def _cancel_managed_quotes(
            self,
            reason: str,
            force: bool = False,
            *,
            managed_orders: list[Order] | None = None,
        ) -> None:
            if managed_orders is None:
                managed_orders = self._managed_orders()
            result = managed_orders_mod.cancel_managed_quotes(
                reason=reason,
                force=force,
                tracked_ids=self._managed_client_order_ids,
                managed_orders=managed_orders,
                maker_instrument_id=self.config.maker_instrument_id,
                cancel_order=self.cancel_order,
                cancel_all_orders=self.cancel_all_orders,
                cancel_all_instrument_orders=bool(
                    getattr(self.config, "cancel_all_instrument_orders", False),
                ),
            )
            if not result.should_cancel:
                return
            self._publish_event(
                "quotes_canceled",
                reason=reason,
                force=force,
                cache_count=result.cache_count,
                tracked_count=result.tracked_count,
                cancel_attempts=result.cancel_attempts,
                cancel_exceptions=result.cancel_exceptions,
                cancel_success=result.cancel_success,
                cancel_all_instrument=result.cancel_all_instrument,
                cancel_all_attempted=result.cancel_all_attempted,
                cancel_all_exceptions=result.cancel_all_exceptions,
                cancellation_safety_invariant=managed_orders_mod.CANCELLATION_SAFETY_INVARIANT,
            )
            self.log.info(
                f"Managed quote cancel triggered strategy_id={self._external_strategy_id} "
                f"reason={reason} force={force} cache_count={result.cache_count} "
                f"tracked_count={result.tracked_count} cancel_attempts={result.cancel_attempts} "
                f"cancel_exceptions={result.cancel_exceptions} "
                f"cancel_all_instrument={result.cancel_all_instrument}",
            )
            self._invalidate_inventory_skew_cache()

        def _best_bid_ask(self, instrument_id: InstrumentId) -> tuple[Decimal, Decimal] | None:
            book = self._books.get(instrument_id)
            if book is None:
                return None
            bid = book.best_bid_price()
            ask = book.best_ask_price()
            if bid is None or ask is None:
                return None
            return bid.as_decimal(), ask.as_decimal()

        def _best_mid(self, instrument_id: InstrumentId) -> Decimal | None:
            bbo = self._best_bid_ask(instrument_id)
            if bbo is None:
                return None
            bid, ask = bbo
            return (bid + ask) / Decimal(2)

        def _book_spread(self, instrument_id: InstrumentId) -> Decimal | None:
            bbo = self._best_bid_ask(instrument_id)
            if bbo is None:
                return None
            bid, ask = bbo
            return ask - bid

        def _recompute_and_publish_fv(self) -> None:
            maker_mid = self._best_mid(self.config.maker_instrument_id)
            reference_mid = self._best_mid(self.config.reference_instrument_id)
            if maker_mid is None and reference_mid is None:
                return

            if maker_mid is not None and reference_mid is not None:
                self._last_fv = (maker_mid + reference_mid) / Decimal(2)
            else:
                self._last_fv = maker_mid or reference_mid

            now_ns = int(self.clock.timestamp_ns())
            payload = {
                "strategy_id": self._external_strategy_id,
                "fv": str(self._last_fv),
                "maker_mid": str(maker_mid) if maker_mid is not None else None,
                "reference_mid": str(reference_mid) if reference_mid is not None else None,
                "ts_event": now_ns,
                "ts_ms": now_ns // 1_000_000,
            }
            self._publish_json(TOPIC_FV, [payload])
            self._last_fv_snapshot_ts_ns = now_ns

        def _publish_market_bbo(
            self,
            *,
            instrument_id: InstrumentId,
            bid: Decimal,
            ask: Decimal,
            ts_ns: int,
        ) -> None:
            publisher_mod.publish_market_bbo(
                self,
                instrument_id=instrument_id,
                bid=bid,
                ask=ask,
                ts_ns=ts_ns,
            )

        def _publish_state(
            self,
            state: str,
            *,
            managed_orders_count: int | None = None,
            managed_orders: list[Order] | None = None,
        ) -> None:
            publisher_mod.publish_state(
                self,
                state,
                managed_orders_count=managed_orders_count,
                managed_orders=managed_orders,
            )

        def _publish_event(self, name: str, *, ts_ns: int | None = None, **payload: Any) -> None:
            publisher_mod.publish_event(self, name, ts_ns=ts_ns, **payload)

        def _publish_actionable_alert(
            self,
            *,
            alert_key: str,
            message: str,
            level: str = "warning",
            reason_code: str | None = None,
            cooldown_ms: int = 0,
            transition: str | None = None,
            now_ns: int | None = None,
        ) -> bool:
            return publisher_mod.publish_actionable_alert(
                self,
                alert_key=alert_key,
                message=message,
                level=level,
                reason_code=reason_code,
                cooldown_ms=cooldown_ms,
                transition=transition,
                now_ns=now_ns,
            )

        def _publish_alert(
            self,
            message: str,
            level: str = "warning",
            *,
            ts_ns: int | None = None,
            alert_key: str | None = None,
            reason_code: str | None = None,
            actionable: bool | None = None,
        ) -> None:
            publisher_mod.publish_alert(
                self,
                message,
                level,
                ts_ns=ts_ns,
                alert_key=alert_key,
                reason_code=reason_code,
                actionable=actionable,
            )

        def _publish_balances(self) -> None:
            publisher_mod.publish_balances(self)

        def _publish_json(self, topic: str, payload: dict[str, Any] | list[Any]) -> None:
            publisher_mod.publish_json(self, topic, payload)


else:
    if not TYPE_CHECKING:

        class MakerV3StrategyConfig:  # pragma: no cover - fallback for pure-math tests
            """
            Provide a fallback config type when runtime modules are unavailable.
            """

        class MakerV3Strategy:  # pragma: no cover - fallback for pure-math tests
            """
            Raise eagerly when strategy runtime dependencies are unavailable.
            """

            def __init__(self, *_args: Any, **_kwargs: Any) -> None:
                """
                Raise `ModuleNotFoundError` in pure-math test environments.
                """
                raise ModuleNotFoundError(
                    "NautilusTrader runtime modules are unavailable in this environment",
                ) from _NAUTILUS_IMPORT_ERROR
