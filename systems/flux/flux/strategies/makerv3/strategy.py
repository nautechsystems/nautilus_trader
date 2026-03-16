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
from typing import Literal

from flux.common.keys import FluxRedisKeys
from flux.common.portfolio_inventory import DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
from flux.common.portfolio_inventory import StrategyInventoryComponent
from flux.common.portfolio_inventory import decode_portfolio_inventory
from flux.common.portfolio_inventory import encode_component
from flux.common.quantity_units import exposure_from_venue_qty
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
from flux.strategies.makerv3.constants import (
    ALERT_COOLDOWN_TERMINAL_ORDER_DENIED_MS,
)
from flux.strategies.makerv3.constants import ALERT_KEY_ORDER_REJECTED_BURST
from flux.strategies.makerv3.constants import ALERT_KEY_TERMINAL_ORDER_DENIED
from flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE
from flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_MD_STALE
from flux.strategies.makerv3.constants import REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE
from flux.strategies.makerv3.constants import REASON_BLOCKED_REFERENCE_MD_STALE
from flux.strategies.makerv3.constants import REASON_CANCEL_BOT_OFF
from flux.strategies.makerv3.constants import REASON_CANCEL_BOT_OFF_FLIP
from flux.strategies.makerv3.constants import REASON_CANCEL_MAKER_BOOK_UNAVAILABLE
from flux.strategies.makerv3.constants import REASON_CANCEL_MAKER_MD_STALE
from flux.strategies.makerv3.constants import REASON_CANCEL_NO_TARGETS
from flux.strategies.makerv3.constants import REASON_CANCEL_ON_STOP
from flux.strategies.makerv3.constants import REASON_CANCEL_QUOTE_FAIL_CIRCUIT_BREAKER
from flux.strategies.makerv3.constants import REASON_CANCEL_REFERENCE_MD_STALE
from flux.strategies.makerv3.constants import REASON_PLACE_MISSING_LEVEL
from flux.strategies.makerv3.constants import TOPIC_FV
from flux.strategies.makerv3.constants import TOPIC_ORDER_INTENT
from flux.strategies.makerv3.constants import TOPIC_TRADE
from flux.strategies.makerv3.wire import build_quote_cycle_envelope
from flux.strategies.makerv3.wire import build_quote_cycle_id
from flux.strategies.makerv3.wire import QuoteCycleContext


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


SpotCashBorrowingPolicy = Literal["none", "sell_only", "both_sides"]
OrderQtyUnit = Literal["venue", "base"]


def _did_bot_turn_off(previous_bot_on: bool, current_bot_on: bool) -> bool:
    return bool(previous_bot_on) and (not bool(current_bot_on))


def _normalized_reject_reason(reason: Any) -> str:
    text = str(reason or "").strip()
    return text or "unknown"


_TERMINAL_CANCEL_REJECT_REASON_FRAGMENTS: tuple[str, ...] = (
    "order not exists",
    "too late to cancel",
    "unknown order sent",
    "order does not exist",
)


def _is_terminal_cancel_reject_reason(reason: Any) -> bool:
    normalized = failures_mod.normalize_reason_text(reason)
    if not normalized:
        return False
    return any(fragment in normalized for fragment in _TERMINAL_CANCEL_REJECT_REASON_FRAGMENTS)


def _order_side_text(side: Any) -> str | None:
    if side is None:
        return None
    name = getattr(side, "name", None)
    if isinstance(name, str) and name:
        return name
    text = str(side).strip().upper()
    if text in {"BUY", "SELL"}:
        return text
    if text == "1":
        return "BUY"
    if text == "2":
        return "SELL"
    return text or None


def _json_safe_value(value: Any) -> Any:
    if isinstance(value, Decimal):
        return _decimal_to_json_str(value)
    if isinstance(value, Mapping):
        return {str(key): _json_safe_value(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_json_safe_value(item) for item in value]
    return value


_validate_three_band_input = pricing_mod.validate_three_band_input
build_ladder_targets = pricing_mod.build_ladder_targets
build_ladder_place_cancel_levels = pricing_mod.build_ladder_place_cancel_levels
build_ladder_place_cancel_levels_from_bps = pricing_mod.build_ladder_place_cancel_levels_from_bps
plan_side_rebalance_actions = rebalancing_mod.plan_side_rebalance_actions
plan_side_rebalance_details = rebalancing_mod.plan_side_rebalance_details


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
        qty_unit: OrderQtyUnit = "venue"
        external_strategy_id: str = "makerv3"
        bot_on: bool | None = None
        qty: Decimal | None = None
        des_qty_global: NonNegativeFloat | None = None
        max_qty_global: NonNegativeFloat | None = None
        max_skew_bps_global: NonNegativeFloat | None = None
        des_qty_local: NonNegativeFloat | None = None
        max_qty_local: NonNegativeFloat | None = None
        max_skew_bps_local: NonNegativeFloat | None = None
        linear_offset_bps: float | None = None
        max_age_ms: PositiveInt | None = None
        bid_edge1: float | None = None
        ask_edge1: float | None = None
        place_edge1: NonNegativeFloat | None = None
        distance1: NonNegativeFloat | None = None
        n_orders1: NonNegativeInt | None = None
        bid_edge2: float | None = None
        ask_edge2: float | None = None
        place_edge2: NonNegativeFloat | None = None
        distance2: NonNegativeFloat | None = None
        n_orders2: NonNegativeInt | None = None
        bid_edge3: float | None = None
        ask_edge3: float | None = None
        place_edge3: NonNegativeFloat | None = None
        distance3: NonNegativeFloat | None = None
        n_orders3: NonNegativeInt | None = None
        order_reject_alert_after_count: NonNegativeInt | None = None
        order_reject_alert_after_s: NonNegativeFloat | None = None
        pending_cancel_grace_ms: NonNegativeInt | None = None
        pending_cancel_block_after_ms: NonNegativeInt | None = None
        max_cancels_per_side_per_cycle: NonNegativeInt | None = None
        max_places_per_side_per_cycle: NonNegativeInt | None = None
        max_total_actions_per_cycle: NonNegativeInt | None = None
        max_pending_cancels_per_side: NonNegativeInt | None = None
        quote_liveness_stall_after_ms: NonNegativeInt | None = None
        quote_liveness_recover_after_ms: NonNegativeInt | None = None
        quote_fail_critical_after_count: NonNegativeInt | None = None
        quote_fail_critical_after_s: NonNegativeFloat | None = None
        spot_cash_borrowing_policy: SpotCashBorrowingPolicy = "none"
        force_bot_off_on_start: bool = False
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
        CANCEL_REJECT_RETRY_COOLDOWN_MS = 1_000
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
            self._last_bbo_event_ts_ns: dict[InstrumentId, int] = {}
            self._last_bbo_init_ts_ns: dict[InstrumentId, int] = {}
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
            self._pending_cancel_client_order_ids: set[str] = set()
            self._pending_cancel_first_seen_ns_by_client_order_id: dict[str, int] = {}
            self._cancel_reject_retry_after_ns_by_client_order_id: dict[str, int] = {}
            self._latest_place_intent_by_client_order_id: dict[str, dict[str, Any]] = {}
            self._order_rejections_ns_by_reason: dict[str, list[int]] = {}
            self._quote_failures_ns: list[int] = []
            self._quote_failure_circuit_open = False
            self._terminal_order_denial_circuit_open = False
            self._venue_protection_circuit_open = False
            self._last_stale_cancel_ns = 0
            self._last_completed_quote_ns = 0
            self._last_order_event_ns = 0
            self._last_state_name: str | None = None
            self._state_is_blocked = False
            self._startup_bot_off_active = False
            self._startup_bot_off_control_revision = ""
            self._startup_cleanup_pending = False
            self._stop_allow_instrument_cancel_override: bool | None = None
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
            self._portfolio_inventory_allow_partial_global_risk = False
            self._latest_maker_position_report_snapshot: dict[str, Any] | None = None
            self._last_maker_position_activity_ns = 0
            self._bounded_convergence_next_start_side = OrderSide.BUY

        def on_start(self) -> None:
            """
            Start subscriptions, timers, and initial strategy publications.
            """
            self._runtime_params_failed = False
            self._quote_failure_circuit_open = False
            self._venue_protection_circuit_open = False
            self._order_rejections_ns_by_reason.clear()
            self._quote_failures_ns.clear()
            self._pending_cancel_client_order_ids.clear()
            self._pending_cancel_first_seen_ns_by_client_order_id.clear()
            self._cancel_reject_retry_after_ns_by_client_order_id.clear()
            self._latest_place_intent_by_client_order_id.clear()
            self._last_stale_cancel_ns = 0
            self._last_completed_quote_ns = 0
            self._last_order_event_ns = 0
            self._last_state_name = None
            self._state_is_blocked = False
            self._startup_bot_off_active = False
            self._startup_bot_off_control_revision = ""
            self._startup_cleanup_pending = False
            self._set_managed_only_stop_safety(False)
            self._last_actionable_alert_ns.clear()
            self._last_actionable_alert_transition.clear()
            self._latest_maker_position_report_snapshot = None
            self._last_maker_position_activity_ns = 0
            self._bounded_convergence_next_start_side = OrderSide.BUY
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
                self._order_qty = runtime_params_mod.resolve_order_quantity(
                    self,
                    self.config.active_order_qty,
                )
            except Exception as e:
                self._publish_alert(str(e), level="error")
                self.stop()
                return
            try:
                self._prepare_runtime_params_for_startup()
                self._refresh_runtime_params(force=True)
            except Exception as e:
                self._fail_fast_runtime_params(context="on_start", exc=e)
                return
            self._terminal_order_denial_circuit_open = False
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
            self._last_bbo_event_ts_ns = dict.fromkeys(self._books, 0)
            self._last_bbo_init_ts_ns = dict.fromkeys(self._books, 0)
            self._last_market_bbo_publish_ns = dict.fromkeys(self._books, 0)

            for instrument_id in subscribed_instrument_ids:
                self.subscribe_order_book_deltas(
                    instrument_id=instrument_id,
                    book_type=BookType.L2_MBP,
                )
            msgbus = getattr(self, "msgbus", None)
            if msgbus is None:
                msgbus = getattr(self, "_msgbus", None)
            subscribe = getattr(msgbus, "subscribe", None)
            if callable(subscribe):
                with suppress(Exception):
                    subscribe(
                        topic="reports.execution.*",
                        handler=self._handle_execution_report_message,
                    )

            self._log_startup_qty_guardrail()
            self._begin_startup_cleanup_if_needed()
            self._publish_event("started")
            self._publish_balances()
            startup_state = "blocked_startup_cleanup" if self._startup_cleanup_pending else "on_start"
            self._publish_portfolio_inventory_component(state=startup_state)
            if not self._startup_cleanup_pending:
                self._publish_state("on_start")
            self.log.info(
                f"MakerV3 strategy started strategy_id={self._external_strategy_id} "
                f"maker={self.config.maker_instrument_id} reference={self.config.reference_instrument_id}",
            )

        def _log_startup_qty_guardrail(self) -> None:
            if self._maker_instrument_is_spot():
                return

            base_currency = self._maker_base_currency_code()
            local_position_summary = self._maker_local_position_summary(base_currency)
            qty_unit = runtime_params_mod.configured_qty_unit(self)
            configured_order_qty = self.config.active_order_qty
            resolved_order_qty_venue = _to_decimal_or_none(self._order_qty)
            conversion_status = local_position_summary.qty_conversion_status or "none"
            conversion_source = local_position_summary.qty_conversion_source or "none"
            base_qty_complete = bool(local_position_summary.qty_complete)

            self.log.info(
                "startup_qty_guardrail "
                f"strategy_id={self._external_strategy_id} "
                f"maker={self.config.maker_instrument_id} "
                f"base_currency={base_currency} "
                f"qty_unit={qty_unit} "
                f"configured_order_qty={configured_order_qty} "
                f"resolved_order_qty_venue={resolved_order_qty_venue} "
                f"local_position_qty_venue={local_position_summary.venue_qty} "
                f"local_position_qty_base={local_position_summary.base_qty} "
                f"base_qty_complete={base_qty_complete} "
                f"conversion_status={conversion_status} "
                f"conversion_source={conversion_source}",
            )

            if local_position_summary.venue_qty is None or local_position_summary.base_qty is not None:
                return

            self.log.warning(
                "startup_qty_guardrail_missing_base "
                f"strategy_id={self._external_strategy_id} "
                f"maker={self.config.maker_instrument_id} "
                f"qty_unit={qty_unit} "
                f"local_position_qty_venue={local_position_summary.venue_qty} "
                f"local_position_qty_base={local_position_summary.base_qty} "
                f"conversion_status={conversion_status} "
                f"conversion_source={conversion_source}",
            )

        def on_stop(self) -> None:
            """
            Stop quoting, cancel managed orders, and publish terminal state.
            """
            self._cancel_managed_quotes(
                "on_stop",
                force=True,
                allow_instrument_cancel=self._stop_allow_instrument_cancel_override,
            )
            self._set_managed_only_stop_safety(False)
            self._pending_cancel_client_order_ids.clear()
            self._pending_cancel_first_seen_ns_by_client_order_id.clear()
            self._cancel_reject_retry_after_ns_by_client_order_id.clear()
            self._latest_place_intent_by_client_order_id.clear()
            self._startup_cleanup_pending = False
            timer_names: set[str] = set()
            try:
                timer_names = set(self.clock.timer_names)
            except Exception:
                timer_names = set()
            msgbus = getattr(self, "msgbus", None)
            if msgbus is None:
                msgbus = getattr(self, "_msgbus", None)
            unsubscribe = getattr(msgbus, "unsubscribe", None)
            if callable(unsubscribe):
                with suppress(Exception):
                    unsubscribe(
                        topic="reports.execution.*",
                        handler=self._handle_execution_report_message,
                    )
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
            quote_management_suspended = self._quote_management_suspended()
            if _did_bot_turn_off(self._last_bot_on, bot_on_now) and not quote_management_suspended:
                self._cancel_managed_quotes("bot_off_flip", force=True)
                self._publish_state("bot_off")
            self._last_bot_on = bot_on_now
            if not bot_on_now or quote_management_suspended:
                return

            self._enforce_stale_market_data(now_ns=now_ns)
            if self._quote_failure_circuit_open:
                return
            if now_ns - self._last_requote_ns < self.INTERNAL_REQUOTE_THROTTLE_MS * 1_000_000:
                return
            if not self._books_fresh_for_quoting(now_ns=now_ns):
                return

            quote_cycle = self._begin_quote_cycle(
                now_ns=now_ns,
                trigger_source="timer_guard",
                trigger_instrument_id=self.config.maker_instrument_id,
                trigger_md_ts_event_ns=int(
                    self._last_bbo_event_ts_ns.get(self.config.maker_instrument_id, 0) or 0,
                )
                or None,
                trigger_md_ts_init_ns=int(
                    self._last_bbo_init_ts_ns.get(self.config.maker_instrument_id, 0) or 0,
                )
                or None,
            )
            try:
                self._refresh_quotes(
                    now_ns=now_ns,
                    quote_cycle_id=quote_cycle.quote_cycle_id,
                    quote_cycle=quote_cycle,
                )
                self._quote_failures_ns.clear()
            except Exception as e:
                self._handle_quote_failure(now_ns=now_ns, exc=e, context="on_time_event")

        def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
            """
            Process market deltas and trigger quote-cycle refresh when eligible.
            """
            market_data_mod.on_order_book_deltas(self, deltas)

        def _enforce_stale_market_data(self, *, now_ns: int) -> None:
            """
            Enforce stale market-data quote blocks even when deltas go silent.
            """
            if self._quote_management_suspended():
                return
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
                quote_cycle = self._begin_quote_cycle(
                    now_ns=now_ns,
                    trigger_source="timer_guard",
                    trigger_instrument_id=self.config.maker_instrument_id,
                    trigger_md_ts_event_ns=int(
                        self._last_bbo_event_ts_ns.get(self.config.maker_instrument_id, 0) or 0,
                    )
                    or None,
                    trigger_md_ts_init_ns=int(
                        self._last_bbo_init_ts_ns.get(self.config.maker_instrument_id, 0) or 0,
                    )
                    or None,
                )
                self._handle_stale_quote_block(
                    now_ns=now_ns,
                    state="blocked_maker_md",
                    cancel_reason="maker_book_unavailable",
                    reason_code=REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE,
                    quote_cycle=quote_cycle,
                    warning_message=(
                        "Quoting blocked (maker book unavailable) "
                        f"strategy_id={self._external_strategy_id}"
                    ),
                )
                return
            maker_age_ns = now_ns - maker_ts_ns
            if maker_age_ns >= max_age_ns:
                age_ms = int(maker_age_ns / 1_000_000)
                quote_cycle = self._begin_quote_cycle(
                    now_ns=now_ns,
                    trigger_source="timer_guard",
                    trigger_instrument_id=self.config.maker_instrument_id,
                    trigger_md_ts_event_ns=int(
                        self._last_bbo_event_ts_ns.get(self.config.maker_instrument_id, 0) or 0,
                    )
                    or None,
                    trigger_md_ts_init_ns=int(
                        self._last_bbo_init_ts_ns.get(self.config.maker_instrument_id, 0) or 0,
                    )
                    or None,
                )
                self._handle_stale_quote_block(
                    now_ns=now_ns,
                    state="blocked_maker_md",
                    cancel_reason="maker_md_stale",
                    reason_code=REASON_BLOCKED_MAKER_MD_STALE,
                    quote_cycle=quote_cycle,
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
                quote_cycle = self._begin_quote_cycle(
                    now_ns=now_ns,
                    trigger_source="timer_guard",
                    trigger_instrument_id=self.config.reference_instrument_id,
                    trigger_md_ts_event_ns=int(
                        self._last_bbo_event_ts_ns.get(self.config.reference_instrument_id, 0) or 0,
                    )
                    or None,
                    trigger_md_ts_init_ns=int(
                        self._last_bbo_init_ts_ns.get(self.config.reference_instrument_id, 0) or 0,
                    )
                    or None,
                )
                self._handle_stale_quote_block(
                    now_ns=now_ns,
                    state="blocked_reference_md",
                    cancel_reason="reference_md_stale",
                    reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
                    quote_cycle=quote_cycle,
                    warning_message=(
                        "Quoting blocked (reference data stale) "
                        f"strategy_id={self._external_strategy_id} "
                        f"age_ms={reference_age_ms} max_age_ms={max_age_ms}"
                    ),
                )

        def _effective_bot_on(self) -> bool:
            return runtime_params_mod.effective_bot_on(self)

        def _quote_management_suspended(self) -> bool:
            is_exiting = getattr(self, "is_exiting", None)
            if not callable(is_exiting):
                return False
            with suppress(Exception):
                return bool(is_exiting())
            return False

        def _books_fresh_for_quoting(
            self,
            *,
            now_ns: int,
            max_age_ms: int | None = None,
        ) -> bool:
            max_age_value = self._runtime_int("max_age_ms") if max_age_ms is None else int(max_age_ms)
            max_age_ns = max(1, max_age_value) * 1_000_000
            for instrument_id in (
                self.config.maker_instrument_id,
                self.config.reference_instrument_id,
            ):
                ts_ns = int(self._last_bbo_ts_ns.get(instrument_id, 0) or 0)
                if ts_ns <= 0 or now_ns - ts_ns >= max_age_ns:
                    return False
            return True

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
            if "bot_on" in updates and self._effective_bot_on():
                self._terminal_order_denial_circuit_open = False

        def _persist_runtime_param_updates(
            self,
            updates: dict[str, Any],
            *,
            now_ns: int | None = None,
        ) -> None:
            if now_ns is None:
                now_ns = int(self.clock.timestamp_ns())

            manager = self._ensure_params_manager()
            if manager is None:
                self._apply_runtime_param_updates(updates)
                if "bot_on" in updates:
                    runtime_params_mod.note_explicit_bot_on_update(self)
                return

            update_fn = getattr(manager, "update", None)
            publish_fn = getattr(manager, "publish_update", None)
            if not callable(update_fn) or not callable(publish_fn):
                self._apply_runtime_param_updates(updates)
                if "bot_on" in updates:
                    runtime_params_mod.note_explicit_bot_on_update(self)
                return

            applied_updates = update_fn(updates)
            publish_fn(applied_updates, ts_ms=int(now_ns // 1_000_000))
            self._apply_runtime_param_updates(applied_updates)
            if "bot_on" in applied_updates:
                runtime_params_mod.note_explicit_bot_on_update(self, manager=manager)

        def _refresh_runtime_params(
            self,
            *,
            now_ns: int | None = None,
            force: bool = False,
        ) -> None:
            runtime_params_mod.refresh_runtime_params(self, now_ns=now_ns, force=force)

        def _prepare_runtime_params_for_startup(self) -> None:
            runtime_params_mod.prepare_runtime_params_for_startup(self)

        def _fail_fast_runtime_params(self, *, context: str, exc: Exception) -> None:
            runtime_params_mod.fail_fast_runtime_params(self, context=context, exc=exc)

        def _handle_quote_failure(self, *, now_ns: int, exc: Exception, context: str) -> None:
            failures_mod.handle_quote_failure(self, now_ns=now_ns, exc=exc, context=context)

        def _track_pending_cancel(
            self,
            client_order_id: ClientOrderId | str | None,
            *,
            now_ns: int | None = None,
        ) -> None:
            client_order_id_str = str(client_order_id or "")
            if not client_order_id_str:
                return
            self._pending_cancel_client_order_ids.add(client_order_id_str)
            if now_ns is None:
                with suppress(Exception):
                    now_ns = int(self.clock.timestamp_ns())
            if now_ns is None or int(now_ns) <= 0:
                return
            self._pending_cancel_first_seen_ns_by_client_order_id.setdefault(
                client_order_id_str,
                int(now_ns),
            )

        def _clear_pending_cancel(
            self,
            client_order_id: ClientOrderId | str | None,
        ) -> None:
            client_order_id_str = str(client_order_id or "")
            if not client_order_id_str:
                return
            self._pending_cancel_client_order_ids.discard(client_order_id_str)
            self._pending_cancel_first_seen_ns_by_client_order_id.pop(client_order_id_str, None)

        def _has_pending_managed_cancels(self) -> bool:
            return bool(self._pending_cancel_client_order_ids)

        def _pending_cancel_order(
            self,
            client_order_id: ClientOrderId | str | None,
        ) -> Any | None:
            client_order_id_str = str(client_order_id or "")
            if not client_order_id_str:
                return None
            cache = getattr(self, "_cache", None)
            order_fn = getattr(cache, "order", None)
            if not callable(order_fn):
                return None
            with suppress(Exception):
                return order_fn(client_order_id_str)
            return None

        def _clear_orphaned_pending_cancels(self) -> tuple[str, ...]:
            cleared: list[str] = []
            for client_order_id in sorted(self._pending_cancel_client_order_ids):
                if self._pending_cancel_order(client_order_id) is not None:
                    continue
                self._pending_cancel_client_order_ids.discard(client_order_id)
                self._pending_cancel_first_seen_ns_by_client_order_id.pop(client_order_id, None)
                cleared.append(client_order_id)
            return tuple(cleared)

        def _quote_progress_payload(self) -> dict[str, Any] | None:
            payload: dict[str, Any] = {}
            last_completed_quote_ns = int(getattr(self, "_last_completed_quote_ns", 0) or 0)
            if last_completed_quote_ns > 0:
                payload["last_completed_quote_ts_ms"] = last_completed_quote_ns // 1_000_000
            last_order_event_ns = int(getattr(self, "_last_order_event_ns", 0) or 0)
            if last_order_event_ns > 0:
                payload["last_order_event_ts_ms"] = last_order_event_ns // 1_000_000
            pending_cancel_ids = tuple(self._pending_cancel_client_order_ids)
            pending_cancel_count = len(pending_cancel_ids)
            if pending_cancel_count > 0:
                payload["pending_cancel_count"] = pending_cancel_count
                current_state_ns = int(getattr(self, "_last_state_ns", 0) or 0)
                oldest_pending_cancel_ns = min(
                    (
                        int(self._pending_cancel_first_seen_ns_by_client_order_id.get(client_order_id, 0) or 0)
                        for client_order_id in pending_cancel_ids
                    ),
                    default=0,
                )
                if oldest_pending_cancel_ns > 0 and current_state_ns >= oldest_pending_cancel_ns:
                    payload["oldest_pending_cancel_age_ms"] = (
                        current_state_ns - oldest_pending_cancel_ns
                    ) // 1_000_000
            return payload or None

        def _quote_blockers_payload(self, *, state: str | None = None) -> list[dict[str, Any]]:
            pending_cancel_count = len(self._pending_cancel_client_order_ids)
            if pending_cancel_count <= 0:
                return []
            state_name = str(state or getattr(self, "_last_state_name", None) or "").strip().lower()
            reason_code = (
                "pending_cancel_stuck"
                if state_name == "blocked_pending_cancel"
                else "pending_cancel_in_flight"
            )
            blocker: dict[str, Any] = {
                "reason_code": reason_code,
                "pending_cancel_count": pending_cancel_count,
            }
            quote_progress = self._quote_progress_payload() or {}
            oldest_pending_cancel_age_ms = quote_progress.get("oldest_pending_cancel_age_ms")
            if oldest_pending_cancel_age_ms is not None:
                blocker["oldest_pending_cancel_age_ms"] = oldest_pending_cancel_age_ms
            return [blocker]

        def _record_order_event_progress(self, event: Any) -> None:
            now_ns = getattr(event, "ts_event", None)
            if now_ns is None:
                with suppress(Exception):
                    now_ns = int(self.clock.timestamp_ns())
            if now_ns is None:
                return
            self._last_order_event_ns = max(
                int(getattr(self, "_last_order_event_ns", 0) or 0),
                int(now_ns),
            )

        def _set_managed_only_stop_safety(self, enabled: bool) -> None:
            self.request_immediate_stop(enabled)
            self._stop_allow_instrument_cancel_override = False if enabled else None

        def _set_cancel_reject_retry_after(
            self,
            client_order_id: ClientOrderId | str | None,
            *,
            now_ns: int,
        ) -> None:
            client_order_id_str = str(client_order_id or "")
            if not client_order_id_str:
                return
            self._cancel_reject_retry_after_ns_by_client_order_id[client_order_id_str] = (
                int(now_ns) + self.CANCEL_REJECT_RETRY_COOLDOWN_MS * 1_000_000
            )

        def _clear_cancel_reject_retry_after(
            self,
            client_order_id: ClientOrderId | str | None,
        ) -> None:
            client_order_id_str = str(client_order_id or "")
            if not client_order_id_str:
                return
            self._cancel_reject_retry_after_ns_by_client_order_id.pop(client_order_id_str, None)

        def _is_cancel_reject_retry_blocked(
            self,
            client_order_id: ClientOrderId | str | None,
            *,
            now_ns: int,
        ) -> bool:
            client_order_id_str = str(client_order_id or "")
            if not client_order_id_str:
                return False
            retry_after_ns = int(
                self._cancel_reject_retry_after_ns_by_client_order_id.get(client_order_id_str, 0),
            )
            if retry_after_ns <= 0:
                return False
            if int(now_ns) >= retry_after_ns:
                self._cancel_reject_retry_after_ns_by_client_order_id.pop(client_order_id_str, None)
                return False
            return True

        def _active_cancel_reject_cooldown_order_ids(
            self,
            *,
            now_ns: int,
            managed_orders: list[Order] | None = None,
        ) -> list[str]:
            if managed_orders is None:
                managed_orders = self._managed_orders()
            active_ids: list[str] = []
            for order in managed_orders:
                client_order_id = str(getattr(order, "client_order_id", "") or "")
                if not client_order_id:
                    continue
                if self._is_cancel_reject_retry_blocked(client_order_id, now_ns=now_ns):
                    active_ids.append(client_order_id)
            return active_ids

        def _has_active_cancel_reject_cooldown(
            self,
            *,
            now_ns: int,
            managed_orders: list[Order] | None = None,
        ) -> bool:
            return bool(
                self._active_cancel_reject_cooldown_order_ids(
                    now_ns=now_ns,
                    managed_orders=managed_orders,
                ),
            )

        def _startup_cleanup_active(
            self,
            *,
            managed_orders: list[Order] | None = None,
        ) -> bool:
            if not self._startup_cleanup_pending:
                return False
            if managed_orders is None:
                managed_orders = self._managed_orders()
            if managed_orders or self._has_pending_managed_cancels():
                return True
            self._startup_cleanup_pending = False
            self._set_managed_only_stop_safety(False)
            return False

        def _begin_startup_cleanup_if_needed(self) -> None:
            managed_orders = self._managed_orders()
            if not managed_orders:
                self._startup_cleanup_pending = False
                self._set_managed_only_stop_safety(False)
                return
            self._startup_cleanup_pending = True
            self._set_managed_only_stop_safety(True)
            self._publish_state(
                "blocked_startup_cleanup",
                managed_orders_count=len(managed_orders),
                managed_orders=managed_orders,
            )
            self._publish_event(
                "startup_cleanup_started",
                managed_orders=len(managed_orders),
            )
            self._cancel_managed_quotes(
                "startup_cleanup",
                managed_orders=managed_orders,
                allow_instrument_cancel=False,
            )

        def on_order_filled(self, event: OrderFilled) -> None:
            """
            Handle order fill events and reconcile managed order tracking.
            """
            self._record_order_event_progress(event)
            self._invalidate_inventory_skew_cache()
            self._clear_cancel_reject_retry_after(getattr(event, "client_order_id", None))
            self._clear_pending_cancel(getattr(event, "client_order_id", None))
            place_intent = self._pop_place_intent(getattr(event, "client_order_id", None))
            self._publish_portfolio_inventory_component(
                state=self._last_state_name or "running",
                now_ms_value=int(int(event.ts_event) // 1_000_000),
            )
            trade_payload = {
                "strategy_id": self._external_strategy_id,
                "event": "order_filled",
                "instrument_id": str(event.instrument_id),
                "client_order_id": str(event.client_order_id),
                "trade_id": str(event.trade_id),
                "side": str(event.order_side),
                "qty": str(event.last_qty),
                "price": str(event.last_px),
                "ts_event": int(event.ts_event),
            }
            if place_intent is not None:
                for key in ("run_id", "quote_cycle_id", "reason_code", "level_index"):
                    if place_intent.get(key) is not None:
                        trade_payload[key] = place_intent[key]
            self._publish_json(TOPIC_TRADE, trade_payload)
            self._record_maker_position_activity(event)
            self._reconcile_managed_order(event.client_order_id, lifecycle="filled")
            self._publish_current_state_snapshot()

        def on_position_opened(self, event: Any) -> None:
            """
            Position cache events are downstream of reconciliation and startup replay.
            They must not invalidate a fresher direct maker venue report snapshot.
            """
            _ = event

        def on_position_changed(self, event: Any) -> None:
            """
            Position cache events are downstream of reconciliation and startup replay.
            They must not invalidate a fresher direct maker venue report snapshot.
            """
            _ = event

        def on_position_closed(self, event: Any) -> None:
            """
            Position cache events are downstream of reconciliation and startup replay.
            They must not invalidate a fresher direct maker venue report snapshot.
            """
            _ = event

        def on_order_rejected(self, event: Any) -> None:
            """
            Handle order rejection events and reconcile managed tracking.
            """
            self._record_order_event_progress(event)
            self._invalidate_inventory_skew_cache()
            self._clear_cancel_reject_retry_after(getattr(event, "client_order_id", None))
            reason = _normalized_reject_reason(getattr(event, "reason", None))
            self._reconcile_managed_order(
                getattr(event, "client_order_id", None),
                lifecycle="rejected",
                instrument_id=getattr(event, "instrument_id", None),
                reason=reason,
                due_post_only=getattr(event, "due_post_only", None),
            )
            self._publish_current_state_snapshot()
            self.log.warning(
                f"Order rejected strategy_id={self._external_strategy_id} "
                f"client_order_id={getattr(event, 'client_order_id', None)} "
                f"reason={reason}",
            )
            now_ns = getattr(event, "ts_event", None)
            if now_ns is None:
                with suppress(Exception):
                    now_ns = int(self.clock.timestamp_ns())
            if now_ns is None:
                return
            self._pop_place_intent(getattr(event, "client_order_id", None))
            if failures_mod.is_venue_protection_reason(reason):
                failures_mod.handle_venue_protection(
                    self,
                    now_ns=int(now_ns),
                    reason=reason,
                    source_event="order_rejected",
                    client_order_id=getattr(event, "client_order_id", None),
                )
                return
            if failures_mod.is_terminal_order_denial_reason(reason):
                self._handle_terminal_order_denial(
                    now_ns=int(now_ns),
                    reason=reason,
                    source_event="order_rejected",
                    client_order_id=getattr(event, "client_order_id", None),
                )
                return
            self._track_order_rejection_alert(
                now_ns=int(now_ns),
                reason=reason,
                source_event="order_rejected",
            )

        def on_order_denied(self, event: Any) -> None:
            """
            Handle local or venue-side order denials and surface repeated bursts.
            """
            self._record_order_event_progress(event)
            self._invalidate_inventory_skew_cache()
            self._clear_cancel_reject_retry_after(getattr(event, "client_order_id", None))
            reason = _normalized_reject_reason(getattr(event, "reason", None))
            self._reconcile_managed_order(
                getattr(event, "client_order_id", None),
                lifecycle="denied",
                instrument_id=getattr(event, "instrument_id", None),
                reason=reason,
            )
            self._publish_current_state_snapshot()
            self.log.warning(
                f"Order denied strategy_id={self._external_strategy_id} "
                f"client_order_id={getattr(event, 'client_order_id', None)} "
                f"reason={reason}",
            )
            now_ns = getattr(event, "ts_event", None)
            if now_ns is None:
                now_ns = getattr(event, "ts_init", None)
            if now_ns is None:
                with suppress(Exception):
                    now_ns = int(self.clock.timestamp_ns())
            if now_ns is None:
                return
            self._pop_place_intent(getattr(event, "client_order_id", None))
            if failures_mod.is_venue_protection_reason(reason):
                failures_mod.handle_venue_protection(
                    self,
                    now_ns=int(now_ns),
                    reason=reason,
                    source_event="order_denied",
                    client_order_id=getattr(event, "client_order_id", None),
                )
                return
            if failures_mod.is_terminal_order_denial_reason(reason):
                self._handle_terminal_order_denial(
                    now_ns=int(now_ns),
                    reason=reason,
                    source_event="order_denied",
                    client_order_id=getattr(event, "client_order_id", None),
                )
                return
            self._track_order_rejection_alert(
                now_ns=int(now_ns),
                reason=reason,
                source_event="order_denied",
            )

        def on_order_pending_cancel(self, event: Any) -> None:
            """
            Track managed orders with cancel requests in flight.
            """
            self._record_order_event_progress(event)
            self._invalidate_inventory_skew_cache()
            self._clear_cancel_reject_retry_after(getattr(event, "client_order_id", None))
            self._track_pending_cancel(
                getattr(event, "client_order_id", None),
                now_ns=getattr(event, "ts_event", None),
            )
            self._publish_current_state_snapshot()

        def on_order_cancel_rejected(self, event: Any) -> None:
            """
            Clear pending-cancel state and hard-stop on venue protection reasons.
            """
            self._record_order_event_progress(event)
            self._invalidate_inventory_skew_cache()
            client_order_id = getattr(event, "client_order_id", None)
            self._clear_pending_cancel(client_order_id)
            self._publish_current_state_snapshot()
            reason = _normalized_reject_reason(getattr(event, "reason", None))
            now_ns = getattr(event, "ts_event", None)
            if now_ns is None:
                with suppress(Exception):
                    now_ns = int(self.clock.timestamp_ns())
            self.log.warning(
                f"Order cancel rejected strategy_id={self._external_strategy_id} "
                f"client_order_id={client_order_id} reason={reason}",
            )
            if now_ns is None:
                return
            if _is_terminal_cancel_reject_reason(reason):
                self._clear_cancel_reject_retry_after(client_order_id)
                self._reconcile_managed_order(
                    client_order_id,
                    lifecycle="cancel_rejected_terminal",
                    instrument_id=getattr(event, "instrument_id", None),
                    reason=reason,
                )
                self._publish_current_state_snapshot()
                return
            if self._startup_cleanup_pending and failures_mod.is_venue_protection_reason(reason):
                self._set_cancel_reject_retry_after(client_order_id, now_ns=int(now_ns))
                self._track_order_rejection_alert(
                    now_ns=int(now_ns),
                    reason=reason,
                    source_event="order_cancel_rejected",
                )
                return
            if not failures_mod.is_venue_protection_reason(reason):
                self._set_cancel_reject_retry_after(client_order_id, now_ns=int(now_ns))
                self._track_order_rejection_alert(
                    now_ns=int(now_ns),
                    reason=reason,
                    source_event="order_cancel_rejected",
                )
                return
            failures_mod.handle_venue_protection(
                self,
                now_ns=int(now_ns),
                reason=reason,
                source_event="order_cancel_rejected",
                client_order_id=client_order_id,
            )

        def on_order_canceled(self, event: Any) -> None:
            """
            Handle order cancel events and reconcile managed tracking.
            """
            self._record_order_event_progress(event)
            self._invalidate_inventory_skew_cache()
            self._clear_cancel_reject_retry_after(getattr(event, "client_order_id", None))
            self._clear_pending_cancel(getattr(event, "client_order_id", None))
            self._reconcile_managed_order(
                getattr(event, "client_order_id", None),
                lifecycle="canceled",
            )
            self._pop_place_intent(getattr(event, "client_order_id", None))
            self._publish_current_state_snapshot()

        def on_order_expired(self, event: Any) -> None:
            """
            Handle order expiry events and reconcile managed tracking.
            """
            self._record_order_event_progress(event)
            self._invalidate_inventory_skew_cache()
            self._clear_cancel_reject_retry_after(getattr(event, "client_order_id", None))
            self._clear_pending_cancel(getattr(event, "client_order_id", None))
            self._reconcile_managed_order(
                getattr(event, "client_order_id", None),
                lifecycle="expired",
            )
            self._pop_place_intent(getattr(event, "client_order_id", None))
            self._publish_current_state_snapshot()

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

        def _track_order_rejection_alert(
            self,
            *,
            now_ns: int,
            reason: str,
            source_event: str,
        ) -> None:
            count_threshold = max(0, int(self._runtime_int("order_reject_alert_after_count")))
            if count_threshold <= 0:
                return

            window_seconds = max(Decimal(0), self._runtime_decimal("order_reject_alert_after_s"))
            window_ns = int(window_seconds * Decimal(1_000_000_000))
            reason_key = _normalized_reject_reason(reason)
            rejection_key = f"{source_event}:{reason_key}"
            reason_rejections = list(self._order_rejections_ns_by_reason.get(rejection_key, ()))
            reason_rejections.append(now_ns)
            if window_ns > 0:
                cutoff_ns = now_ns - window_ns
                reason_rejections = [ts_ns for ts_ns in reason_rejections if ts_ns >= cutoff_ns]
            elif count_threshold > 0:
                reason_rejections = reason_rejections[-count_threshold:]
            self._order_rejections_ns_by_reason[rejection_key] = reason_rejections

            rejection_count = len(reason_rejections)
            if rejection_count < count_threshold:
                return

            self._publish_actionable_alert(
                alert_key=ALERT_KEY_ORDER_REJECTED_BURST,
                message=(
                    "order_rejected_burst "
                    f"source_event={source_event} reason={reason_key!r} count={rejection_count} "
                    f"threshold={count_threshold} window_s={window_seconds}"
                ),
                level="error",
                reason_code=ALERT_KEY_ORDER_REJECTED_BURST,
                cooldown_ms=ALERT_COOLDOWN_ORDER_REJECTED_BURST_MS,
                transition=rejection_key,
                now_ns=now_ns,
            )

        def _handle_terminal_order_denial(
            self,
            *,
            now_ns: int,
            reason: str,
            source_event: str,
            client_order_id: ClientOrderId | str | None,
        ) -> None:
            if self._terminal_order_denial_circuit_open:
                return

            self._terminal_order_denial_circuit_open = True
            normalized_reason = failures_mod.normalize_reason_text(reason) or "unknown"
            raw_reason = str(reason or "")
            client_order_id_text = str(client_order_id or "")
            cancelable_managed_orders = [
                order
                for order in self._managed_orders()
                if getattr(order, "venue_order_id", None) is not None
            ]

            def _safe(effect: Any) -> None:
                with suppress(Exception):
                    effect()

            _safe(lambda: self._persist_runtime_param_updates({"bot_on": False}))
            self._last_bot_on = False
            _safe(
                lambda: self._cancel_managed_quotes(
                    "terminal_order_denied",
                    force=True,
                    managed_orders=cancelable_managed_orders,
                ),
            )
            _safe(lambda: self._publish_state("bot_off"))
            _safe(
                lambda: self._publish_actionable_alert(
                    alert_key=ALERT_KEY_TERMINAL_ORDER_DENIED,
                    message=(
                        "terminal_order_denied "
                        f"source_event={source_event} reason={normalized_reason!r} action='bot_off'"
                    ),
                    level="error",
                    reason_code=ALERT_KEY_TERMINAL_ORDER_DENIED,
                    cooldown_ms=ALERT_COOLDOWN_TERMINAL_ORDER_DENIED_MS,
                    transition=f"{source_event}:{normalized_reason}",
                    now_ns=now_ns,
                    source_event=source_event,
                    raw_reason=raw_reason,
                    client_order_id=client_order_id_text,
                ),
            )
            _safe(
                lambda: self._publish_event(
                    "terminal_order_denied",
                    source_event=source_event,
                    reason=normalized_reason,
                    raw_reason=raw_reason,
                    client_order_id=client_order_id_text,
                ),
            )
            _safe(
                lambda: self.log.error(
                    "Terminal order denial triggered bot_off "
                    f"strategy_id={self._external_strategy_id} "
                    f"source_event={source_event} client_order_id={client_order_id_text or 'unknown'} "
                    f"reason={raw_reason}",
                ),
            )

        def _inventory_cache(self) -> Any | None:
            cache = getattr(self, "_cache", None)
            if cache is None:
                cache = getattr(self, "cache", None)
            return cache

        def _record_maker_position_activity(self, event: Any) -> None:
            instrument_id = getattr(event, "instrument_id", None)
            if instrument_id != self.config.maker_instrument_id:
                return
            ts_event = int(getattr(event, "ts_event", 0) or 0)
            if ts_event > 0:
                self._last_maker_position_activity_ns = max(
                    int(getattr(self, "_last_maker_position_activity_ns", 0) or 0),
                    ts_event,
                )
            self._invalidate_inventory_skew_cache()

        def _execution_report_ts_ns(self, report: Any) -> int:
            timestamps = []
            for field_name in ("ts_last", "ts_event", "ts_init"):
                raw_value = getattr(report, field_name, None)
                try:
                    value = int(raw_value or 0)
                except Exception:
                    value = 0
                if value > 0:
                    timestamps.append(value)
            return max(timestamps) if timestamps else 0

        def _aggregate_maker_position_reports(
            self,
            reports: Any,
            *,
            fallback_ts_ns: int = 0,
        ) -> dict[str, Any] | None:
            relevant_reports: list[Any] = [
                report
                for report in list(reports or ())
                if getattr(report, "instrument_id", None) == self.config.maker_instrument_id
            ]
            if not relevant_reports:
                return None

            if len(relevant_reports) > 1 and not any(
                getattr(report, "venue_position_id", None) is not None
                or getattr(report, "position_id", None) is not None
                for report in relevant_reports
            ):
                candidate_reports = [
                    report
                    for report in relevant_reports
                    if (
                        _to_decimal_or_none(getattr(report, "signed_decimal_qty", None))
                        or _to_decimal_or_none(getattr(report, "signed_qty", None))
                        or Decimal(0)
                    )
                    != 0
                ]
                if not candidate_reports:
                    candidate_reports = relevant_reports
                relevant_reports = [
                    max(
                        candidate_reports,
                        key=lambda report: (
                            self._execution_report_ts_ns(report),
                            abs(
                                _to_decimal_or_none(getattr(report, "signed_decimal_qty", None))
                                or _to_decimal_or_none(getattr(report, "signed_qty", None))
                                or Decimal(0)
                            ),
                        ),
                    ),
                ]

            total = Decimal(0)
            avg_px_num = Decimal(0)
            avg_px_den = Decimal(0)
            latest_ts_ns = max(0, int(fallback_ts_ns))
            position_id: str | None = None
            found = False

            for report in relevant_reports:
                signed_qty = _to_decimal_or_none(getattr(report, "signed_decimal_qty", None))
                if signed_qty is None:
                    signed_qty = _to_decimal_or_none(getattr(report, "signed_qty", None))
                if signed_qty is None:
                    continue
                found = True
                total += signed_qty
                avg_px_open = _to_decimal_or_none(getattr(report, "avg_px_open", None))
                if avg_px_open is not None and signed_qty != 0:
                    avg_px_num += abs(signed_qty) * avg_px_open
                    avg_px_den += abs(signed_qty)
                latest_ts_ns = max(latest_ts_ns, self._execution_report_ts_ns(report))
                if not position_id:
                    report_position_id = getattr(report, "venue_position_id", None) or getattr(
                        report,
                        "position_id",
                        None,
                    )
                    if report_position_id is not None:
                        text = str(report_position_id).strip()
                        if text:
                            position_id = text

            if not found:
                return None

            avg_px_open = avg_px_num / avg_px_den if avg_px_den > 0 else None
            return self._build_maker_position_report_snapshot(
                signed_qty=total,
                avg_px_open=avg_px_open,
                position_id=position_id,
                ts_ns=latest_ts_ns,
            )

        def _build_maker_position_report_snapshot(
            self,
            *,
            signed_qty: Decimal,
            avg_px_open: Decimal | None,
            position_id: str | None,
            ts_ns: int,
        ) -> dict[str, Any]:
            instrument = self._resolve_instrument(self.config.maker_instrument_id)
            signed_qty_base: Decimal | None = None
            qty_conversion_status: str | None = None
            qty_conversion_source: str | None = None
            if instrument is None:
                qty_conversion_status = "missing_metadata"
                qty_conversion_source = "maker instrument unavailable"
            else:
                exposure = exposure_from_venue_qty(
                    instrument,
                    signed_qty,
                    last_px=self._inventory_base_exposure_last_px(),
                )
                signed_qty_base = exposure.base_qty
                qty_conversion_status = exposure.qty_conversion_status
                qty_conversion_source = exposure.qty_conversion_source
            return {
                "instrument_id": self.config.maker_instrument_id,
                "signed_qty": signed_qty,
                "signed_qty_venue": signed_qty,
                "quantity_venue": abs(signed_qty),
                "signed_qty_base": signed_qty_base,
                "quantity_base": abs(signed_qty_base) if signed_qty_base is not None else None,
                "qty_conversion_status": qty_conversion_status,
                "qty_conversion_source": qty_conversion_source,
                "avg_px_open": avg_px_open,
                "position_id": position_id,
                "ts_ns": max(0, int(ts_ns)),
            }

        def _fresh_maker_position_report_snapshot(self) -> dict[str, Any] | None:
            snapshot = getattr(self, "_latest_maker_position_report_snapshot", None)
            if not isinstance(snapshot, Mapping):
                return None
            report_ts_ns = int(snapshot.get("ts_ns") or 0)
            local_activity_ns = int(getattr(self, "_last_maker_position_activity_ns", 0) or 0)
            if report_ts_ns > 0 and report_ts_ns >= local_activity_ns:
                return dict(snapshot)
            return None

        def _handle_execution_report_message(self, message: Any) -> None:
            if self._maker_instrument_is_spot():
                return

            snapshot: dict[str, Any] | None = None
            if getattr(message, "instrument_id", None) == self.config.maker_instrument_id:
                snapshot = self._aggregate_maker_position_reports(
                    [message],
                    fallback_ts_ns=self._execution_report_ts_ns(message),
                )
            else:
                position_reports = getattr(message, "position_reports", None)
                if isinstance(position_reports, Mapping):
                    reports = position_reports.get(self.config.maker_instrument_id)
                    snapshot = self._aggregate_maker_position_reports(
                        reports,
                        fallback_ts_ns=int(getattr(message, "ts_init", 0) or 0),
                    )
                    if snapshot is None:
                        # Reconciliation can encode a flat maker position by omitting the maker row
                        # from an otherwise fresh position_reports snapshot.
                        snapshot = self._build_maker_position_report_snapshot(
                            signed_qty=Decimal(0),
                            avg_px_open=None,
                            position_id=None,
                            ts_ns=int(getattr(message, "ts_init", 0) or 0),
                        )
            if snapshot is None:
                return

            previous_snapshot = getattr(self, "_latest_maker_position_report_snapshot", None)
            previous_ts_ns = (
                int(previous_snapshot.get("ts_ns") or 0)
                if isinstance(previous_snapshot, Mapping)
                else 0
            )
            current_ts_ns = int(snapshot.get("ts_ns") or 0)
            if previous_ts_ns > current_ts_ns:
                return

            self._latest_maker_position_report_snapshot = dict(snapshot)
            self._invalidate_inventory_skew_cache()

            if getattr(self, "_last_state_name", None) is None:
                return

            now_ms_value = current_ts_ns // 1_000_000 if current_ts_ns > 0 else None
            self._publish_portfolio_inventory_component(
                state=self._last_state_name or "running",
                now_ms_value=now_ms_value,
            )
            self._publish_current_state_snapshot()

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

        def _inventory_positions(self) -> list[Position] | None:
            positions = self._open_positions()
            if positions is None:
                return None
            cache = self._inventory_cache()
            orders_for_position = getattr(cache, "orders_for_position", None)
            return inventory_mod.effective_inventory_positions(
                positions,
                order_lookup=orders_for_position if callable(orders_for_position) else None,
            )

        def _inventory_base_exposure_last_px(self) -> Decimal | None:
            if self._last_fv is not None:
                return self._last_fv

            maker_mid = self._best_mid(self.config.maker_instrument_id)
            if maker_mid is not None:
                return maker_mid

            return self._best_mid(self.config.reference_instrument_id)

        def _position_exposure_summary(
            self,
            currency_code: str,
            *,
            venue: Any | None = None,
            instrument_id: Any | None = None,
        ) -> inventory_mod.PositionExposureSummary:
            if not currency_code:
                return inventory_mod.PositionExposureSummary(venue_qty=None, base_qty=None)
            positions = self._inventory_positions()
            if positions is None:
                return inventory_mod.PositionExposureSummary(venue_qty=None, base_qty=None)
            cache = self._inventory_cache()
            return inventory_mod.position_exposure_summary(
                positions,
                base_currency=currency_code,
                instrument_lookup=self._resolve_instrument if cache is not None else None,
                venue=venue,
                instrument_id=instrument_id,
                last_px=self._inventory_base_exposure_last_px(),
            )

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
                if not scoped_accounts:
                    account_for_venue = getattr(cache, "account_for_venue", None)
                    if callable(account_for_venue):
                        with suppress(Exception):
                            scoped_account = account_for_venue(venue=venue)
                            if scoped_account is not None:
                                scoped_accounts = [scoped_account]
                if not scoped_accounts and hasattr(self, "portfolio"):
                    with suppress(Exception):
                        scoped_account = self.portfolio.account(venue=venue)
                        if scoped_account is not None:
                            scoped_accounts = [scoped_account]
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
            allow_partial_global_risk: bool = False,
        ) -> None:
            self._portfolio_inventory_client = redis_client
            self._portfolio_inventory_portfolio_id = portfolio_id.strip() or None
            self._portfolio_inventory_namespace = namespace
            self._portfolio_inventory_schema_version = schema_version
            self._portfolio_inventory_stale_after_ms = max(1, int(stale_after_ms))
            self._portfolio_inventory_allow_partial_global_risk = bool(allow_partial_global_risk)

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

        def _should_allow_cash_borrowing(self, side: OrderSide) -> bool:
            if not self._maker_instrument_is_spot():
                return False

            policy = str(self.config.spot_cash_borrowing_policy).strip().lower()
            if policy == "both_sides":
                return True
            if policy == "sell_only":
                return side == OrderSide.SELL
            return False

        def _maker_local_position_summary(
            self,
            currency_code: str | None,
        ) -> inventory_mod.PositionExposureSummary:
            if not currency_code or self._maker_instrument_is_spot():
                return inventory_mod.PositionExposureSummary(venue_qty=None, base_qty=None)
            fresh_report_snapshot = self._fresh_maker_position_report_snapshot()
            if fresh_report_snapshot is not None:
                signed_qty = _to_decimal_or_none(fresh_report_snapshot.get("signed_qty"))
                if signed_qty is not None:
                    snapshot_base_qty_raw = fresh_report_snapshot.get("signed_qty_base")
                    if snapshot_base_qty_raw is None:
                        snapshot_base_qty_raw = fresh_report_snapshot.get("base_qty")
                    snapshot_base_qty = _to_decimal_or_none(snapshot_base_qty_raw)
                    snapshot_conversion_status = str(
                        fresh_report_snapshot.get("qty_conversion_status") or "",
                    ).strip() or None
                    snapshot_conversion_source = str(
                        fresh_report_snapshot.get("qty_conversion_source") or "",
                    ).strip() or None
                    if snapshot_base_qty is not None or snapshot_conversion_status is not None:
                        return inventory_mod.PositionExposureSummary(
                            venue_qty=signed_qty,
                            base_qty=snapshot_base_qty,
                            qty_complete=snapshot_base_qty is not None,
                            qty_conversion_status=snapshot_conversion_status,
                            qty_conversion_source=snapshot_conversion_source,
                        )
                    instrument = self._resolve_instrument(self.config.maker_instrument_id)
                    if instrument is None:
                        return inventory_mod.PositionExposureSummary(
                            venue_qty=signed_qty,
                            base_qty=None,
                            qty_complete=False,
                            qty_conversion_status="missing_metadata",
                            qty_conversion_source="maker instrument unavailable",
                        )
                    exposure = exposure_from_venue_qty(
                        instrument,
                        signed_qty,
                        last_px=self._inventory_base_exposure_last_px(),
                    )
                    return inventory_mod.PositionExposureSummary(
                        venue_qty=signed_qty,
                        base_qty=exposure.base_qty,
                        qty_complete=exposure.base_qty is not None,
                        qty_conversion_status=exposure.qty_conversion_status,
                        qty_conversion_source=exposure.qty_conversion_source,
                    )
            return self._position_exposure_summary(
                currency_code,
                instrument_id=self.config.maker_instrument_id,
            )

        def _maker_local_spot_qty(self, currency_code: str | None) -> Decimal | None:
            if not currency_code or not self._maker_instrument_is_spot():
                return None
            maker_venue = getattr(self.config.maker_instrument_id, "venue", None)
            return self._spot_balance_total(currency_code, venue=maker_venue)

        def _shared_portfolio_inventory_qty_and_block_reason(
            self,
            base_currency: str | None,
        ) -> tuple[Decimal | None, str | None, dict[str, Any] | None]:
            portfolio_id = self._portfolio_inventory_portfolio_id
            client = self._portfolio_inventory_client
            if not base_currency or not portfolio_id or client is None:
                return None, None, None
            key = FluxRedisKeys.portfolio_inventory(
                portfolio_id=portfolio_id,
                base_currency=base_currency,
                namespace=self._portfolio_inventory_namespace,
                schema_version=self._portfolio_inventory_schema_version,
            )
            with suppress(Exception):
                payload = decode_portfolio_inventory(client.get(key))
                if not isinstance(payload, dict):
                    return None, REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE, None
                ts_ms = int(payload.get("ts_ms") or 0)
                stale_after_ms = int(
                    payload.get("stale_after_ms") or self._portfolio_inventory_stale_after_ms,
                )
                now_ms_value = int(self.clock.timestamp_ns() // 1_000_000)
                if ts_ms <= 0 or now_ms_value - ts_ms > max(1, stale_after_ms):
                    return None, REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE, None
                global_qty = _to_decimal_or_none(
                    payload.get("global_qty_base") or payload.get("global_qty"),
                )
                if global_qty is None:
                    return None, REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE, None
                global_qty_complete = bool(
                    payload.get(
                        "global_qty_base_complete",
                        payload.get("global_qty_complete", True),
                    ),
                )
                diagnostics = {
                    "aggregation_mode": str(payload.get("aggregation_mode") or "strict"),
                    "global_qty_base_complete": global_qty_complete,
                    "global_qty_complete": global_qty_complete,
                    "missing_required": list(payload.get("missing_required") or []),
                    "stale_required": list(payload.get("stale_required") or []),
                    "null_qty_required": list(payload.get("null_qty_required") or []),
                }
                incomplete = not bool(diagnostics["global_qty_base_complete"])
                if incomplete and not self._portfolio_inventory_allow_partial_global_risk:
                    return None, REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE, diagnostics
                return global_qty, None, diagnostics
            return None, REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE, None

        def _portfolio_global_inventory_qty(self, base_currency: str | None) -> Decimal | None:
            global_qty, _block_reason, _diagnostics = self._shared_portfolio_inventory_qty_and_block_reason(
                base_currency,
            )
            return global_qty

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
            local_position_summary = self._maker_local_position_summary(base_currency)
            local_spot_qty = self._maker_local_spot_qty(base_currency)
            local_qty_base = inventory_mod.local_inventory_total(
                local_position_qty=local_position_summary.base_qty,
                local_spot_qty=local_spot_qty,
            )
            component = StrategyInventoryComponent(
                strategy_id=self._external_strategy_id,
                portfolio_id=portfolio_id,
                base_currency=base_currency,
                local_qty_base=local_qty_base,
                ts_ms=ts_ms,
                local_position_qty_venue=local_position_summary.venue_qty,
                local_position_qty_base=local_position_summary.base_qty,
                local_spot_qty=local_spot_qty,
                qty_conversion_status=local_position_summary.qty_conversion_status,
                qty_conversion_source=local_position_summary.qty_conversion_source,
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
            portfolio_global_qty_base, _portfolio_block_reason, portfolio_diagnostics = (
                self._shared_portfolio_inventory_qty_and_block_reason(base_currency)
            )
            use_shared_portfolio = bool(self._portfolio_inventory_portfolio_id)
            global_position_summary = inventory_mod.PositionExposureSummary(
                venue_qty=None,
                base_qty=None,
            )
            if portfolio_global_qty_base is None and not use_shared_portfolio:
                global_position_summary = (
                    self._position_exposure_summary(base_currency)
                    if base_currency
                    else global_position_summary
                )
                global_spot_qty = self._spot_balance_total(base_currency) if base_currency else None
                global_inventory_qty_override = None
                global_inventory_source_override = None
            else:
                global_spot_qty = None
                global_inventory_qty_override = portfolio_global_qty_base
                portfolio_complete = bool(
                    (portfolio_diagnostics or {}).get("global_qty_base_complete", True),
                )
                global_inventory_source_override = (
                    (
                        "portfolio_component_sum"
                        if portfolio_complete
                        else "portfolio_component_partial_sum"
                    )
                    if portfolio_global_qty_base is not None
                    else "portfolio_unavailable"
                )
            local_position_summary = self._maker_local_position_summary(base_currency)
            local_spot_qty = self._maker_local_spot_qty(base_currency)
            if runtime_params is None:
                runtime_params = self._quote_runtime_params_snapshot()
            skew = inventory_mod.compute_inventory_skew(
                global_position_qty_venue=global_position_summary.venue_qty,
                global_position_qty_base=global_position_summary.base_qty,
                global_spot_qty=global_spot_qty,
                local_position_qty_venue=local_position_summary.venue_qty,
                local_position_qty_base=local_position_summary.base_qty,
                local_spot_qty=local_spot_qty,
                global_position_qty_complete=global_position_summary.qty_complete,
                local_position_qty_complete=local_position_summary.qty_complete,
                global_inventory_qty_override=global_inventory_qty_override,
                global_inventory_source_override=global_inventory_source_override,
                base_currency=base_currency,
                runtime_params=runtime_params,
            )
            if global_position_summary.qty_conversion_status is not None:
                skew["global_position_qty_conversion_status"] = global_position_summary.qty_conversion_status
                skew["global_position_qty_conversion_source"] = global_position_summary.qty_conversion_source
            if local_position_summary.qty_conversion_status is not None:
                skew["local_position_qty_conversion_status"] = local_position_summary.qty_conversion_status
                skew["local_position_qty_conversion_source"] = local_position_summary.qty_conversion_source
            if portfolio_diagnostics:
                skew["global_inventory_qty_base_complete"] = bool(
                    portfolio_diagnostics.get("global_qty_base_complete", True),
                )
                skew["global_inventory_qty_complete"] = bool(
                    portfolio_diagnostics.get(
                        "global_qty_base_complete",
                        portfolio_diagnostics.get("global_qty_complete", True),
                    ),
                )
                skew["global_inventory_aggregation_mode"] = str(
                    portfolio_diagnostics.get("aggregation_mode") or "strict",
                )
                skew["global_inventory_missing_required"] = list(
                    portfolio_diagnostics.get("missing_required") or [],
                )
                skew["global_inventory_stale_required"] = list(
                    portfolio_diagnostics.get("stale_required") or [],
                )
                skew["global_inventory_null_qty_required"] = list(
                    portfolio_diagnostics.get("null_qty_required") or [],
                )
            return skew

        def _next_quote_cycle_id(self, *, now_ns: int) -> str:
            del now_ns
            self._quote_cycle_seq = int(getattr(self, "_quote_cycle_seq", 0)) + 1
            return build_quote_cycle_id(
                run_id=str(getattr(self, "_run_id", self._strategy_identity)),
                quote_cycle_seq=self._quote_cycle_seq,
            )

        def _begin_quote_cycle(
            self,
            *,
            now_ns: int,
            trigger_source: str | None,
            trigger_instrument_id: InstrumentId | str | None = None,
            trigger_md_ts_event_ns: int | None = None,
            trigger_md_ts_init_ns: int | None = None,
        ) -> QuoteCycleContext:
            quote_cycle_id = self._next_quote_cycle_id(now_ns=now_ns)
            return QuoteCycleContext(
                run_id=str(getattr(self, "_run_id", self._strategy_identity)),
                quote_cycle_id=quote_cycle_id,
                quote_cycle_seq=int(getattr(self, "_quote_cycle_seq", 0)),
                instrument_id=str(self.config.maker_instrument_id),
                trigger_source=trigger_source,
                trigger_instrument_id=str(trigger_instrument_id) if trigger_instrument_id is not None else None,
                trigger_md_ts_event_ns=trigger_md_ts_event_ns,
                trigger_md_ts_init_ns=trigger_md_ts_init_ns,
                ts_cycle_start_ns=int(now_ns),
            )

        def _quote_cycle_context_from_id(
            self,
            *,
            now_ns: int,
            quote_cycle_id: str,
            trigger_source: str | None = None,
            trigger_instrument_id: InstrumentId | str | None = None,
            trigger_md_ts_event_ns: int | None = None,
            trigger_md_ts_init_ns: int | None = None,
        ) -> QuoteCycleContext:
            quote_cycle_seq = int(getattr(self, "_quote_cycle_seq", 0))
            suffix = quote_cycle_id.rsplit(":", 1)[-1]
            with suppress(ValueError):
                quote_cycle_seq = int(suffix)
            return QuoteCycleContext(
                run_id=str(getattr(self, "_run_id", self._strategy_identity)),
                quote_cycle_id=quote_cycle_id,
                quote_cycle_seq=quote_cycle_seq,
                instrument_id=str(self.config.maker_instrument_id),
                trigger_source=trigger_source,
                trigger_instrument_id=str(trigger_instrument_id) if trigger_instrument_id is not None else None,
                trigger_md_ts_event_ns=trigger_md_ts_event_ns,
                trigger_md_ts_init_ns=trigger_md_ts_init_ns,
                ts_cycle_start_ns=int(now_ns),
            )

        def _publish_quote_cycle_event(
            self,
            *,
            now_ns: int,
            quote_cycle_event: str,
            reason_code: str,
            quote_cycle: QuoteCycleContext | None = None,
            quote_cycle_id: str | None = None,
            payload: dict[str, Any] | None = None,
            **payload_fields: Any,
        ) -> None:
            event_payload = dict(payload or {})
            event_payload.update(payload_fields)
            if quote_cycle is None:
                quote_cycle_id_value = quote_cycle_id or self._next_quote_cycle_id(now_ns=now_ns)
                quote_cycle = self._quote_cycle_context_from_id(
                    now_ns=now_ns,
                    quote_cycle_id=quote_cycle_id_value,
                )
            envelope = build_quote_cycle_envelope(
                context=quote_cycle,
                quote_cycle_event=quote_cycle_event,
                reason_code=reason_code,
                ts_cycle_end_ns=now_ns,
                payload=event_payload,
            )
            self._publish_event(
                QUOTE_CYCLE_EVENT_NAME,
                ts_ns=now_ns,
                **envelope,
            )

        def _quote_cycle_decision_context(
            self,
            *,
            runtime_params: Mapping[str, Any] | None = None,
            managed_orders: list[Order] | None = None,
            per_level_outcomes: list[dict[str, Any]] | None = None,
            bounded_convergence: Mapping[str, Any] | None = None,
        ) -> dict[str, Any] | None:
            if runtime_params is None:
                runtime_params = self._quote_runtime_params_snapshot()
            managed_orders_list = managed_orders if managed_orders is not None else self._managed_orders()
            payload: dict[str, Any] = {}

            if self._last_pricing_debug:
                payload.update(_json_safe_value(self._last_pricing_debug))

            if runtime_params:
                payload["runtime_params"] = _json_safe_value(dict(runtime_params))

            maker_quote_status = publisher_mod._maker_quote_status_payload(  # noqa: SLF001
                self,
                managed_orders=managed_orders_list,
            )
            if maker_quote_status is not None:
                payload["maker_quote_status"] = maker_quote_status

            maker_role_map = publisher_mod._maker_role_map_payload(self)  # noqa: SLF001
            if maker_role_map:
                payload["maker_role_map"] = maker_role_map

            if per_level_outcomes:
                payload["per_level_outcomes"] = list(per_level_outcomes)

            if bounded_convergence:
                payload["bounded_convergence"] = _json_safe_value(dict(bounded_convergence))

            return payload or None

        def _publish_order_intent(
            self,
            *,
            intent_type: str,
            client_order_id: str,
            quote_cycle: QuoteCycleContext | None = None,
            quote_cycle_id: str | None = None,
            reason_code: str,
            side: OrderSide | str | None,
            level_index: int | None,
            target_px: Decimal | Price | str | None,
            cancel_px: Decimal | Price | str | None,
            match_tol: Decimal | str | None,
            ts_decision_ns: int,
            ts_submit_local_ns: int | None = None,
            ts_cancel_request_local_ns: int | None = None,
            decision_context_json: dict[str, Any] | None = None,
        ) -> None:
            payload: dict[str, Any] = {
                "strategy_id": self.runtime_strategy_id,
                "external_strategy_id": self._external_strategy_id,
                "client_order_id": client_order_id,
                "intent_type": intent_type,
                "run_id": str(getattr(self, "_run_id", self._strategy_identity)),
                "quote_cycle_id": (
                    quote_cycle.quote_cycle_id if quote_cycle is not None else quote_cycle_id
                ),
                "reason_code": reason_code,
                "side": _order_side_text(side),
                "level_index": level_index,
                "target_px": None if target_px is None else str(target_px),
                "cancel_px": None if cancel_px is None else str(cancel_px),
                "match_tol": None if match_tol is None else str(match_tol),
                "ts_market_data_event_ns": (
                    quote_cycle.trigger_md_ts_event_ns if quote_cycle is not None else None
                ),
                "ts_market_data_recv_ns": (
                    quote_cycle.trigger_md_ts_init_ns if quote_cycle is not None else None
                ),
                "ts_decision_ns": int(ts_decision_ns),
                "ts_submit_local_ns": ts_submit_local_ns,
                "ts_cancel_request_local_ns": ts_cancel_request_local_ns,
                "decision_context_json": None,
            }
            with suppress(Exception):
                self._publish_json(TOPIC_ORDER_INTENT, payload)
            if intent_type == "PLACE":
                self._latest_place_intent_by_client_order_id[client_order_id] = payload

        def _pop_place_intent(
            self,
            client_order_id: ClientOrderId | str | None,
        ) -> dict[str, Any] | None:
            client_order_id_str = str(client_order_id or "")
            if not client_order_id_str:
                return None
            return self._latest_place_intent_by_client_order_id.pop(client_order_id_str, None)

        def _handle_stale_quote_block(
            self,
            *,
            now_ns: int,
            state: str,
            cancel_reason: str,
            reason_code: str,
            quote_cycle: QuoteCycleContext | None = None,
            quote_cycle_id: str | None = None,
            warning_message: str,
        ) -> None:
            quote_engine_mod.handle_stale_quote_block(
                self,
                now_ns=now_ns,
                state=state,
                cancel_reason=cancel_reason,
                reason_code=reason_code,
                quote_cycle=quote_cycle,
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

        def _refresh_quotes(
            self,
            now_ns: int,
            *,
            quote_cycle_id: str | None = None,
            quote_cycle: QuoteCycleContext | None = None,
        ) -> None:
            quote_engine_mod.refresh_quotes(
                self,
                now_ns=now_ns,
                quote_cycle_id=quote_cycle_id,
                quote_cycle=quote_cycle,
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
            max_reprice_cancel_actions: int | None = None,
            max_place_actions: int | None = None,
            max_total_actions: int | None = None,
            backlog_mode: str = "normal",
            planned_level_indices_out: list[int] | None = None,
            cancel_actions: tuple[Any, ...] | list[Any] | None = None,
            quote_cycle: QuoteCycleContext | None = None,
            quote_cycle_id: str | None = None,
            decision_context_json: dict[str, Any] | None = None,
        ) -> int:
            if max_reprice_cancel_actions is None:
                max_reprice_cancel_actions = self._runtime_int("max_cancels_per_side_per_cycle")
            if max_place_actions is None:
                max_place_actions = self._runtime_int("max_places_per_side_per_cycle")
            if max_total_actions is None:
                max_total_actions = self._runtime_int("max_total_actions_per_cycle")

            desired_dec = [
                (_price_to_decimal(target_price), cancel_px, match_tol)
                for target_price, cancel_px, match_tol in desired_levels
            ]
            if cancel_actions is None:
                side_name = "buy" if side == OrderSide.BUY else "sell"
                active_prices = [_price_to_decimal(order.price) for order in active_orders]
                active_stale = [
                    self._is_stale_order(order, now_ns, max_age_ms=max_age_ms)
                    for order in active_orders
                ]
                plan = rebalancing_mod.plan_side_bounded_convergence(
                    side=side_name,
                    active_prices=active_prices,
                    active_stale=active_stale,
                    desired_levels=desired_dec,
                    stale_cancel_budget=self.STALE_CANCELS_PER_SIDE_PER_CYCLE,
                    max_reprice_cancel_actions=max(
                        0,
                        int(max_reprice_cancel_actions or 0),
                    ),
                    max_place_actions=max(
                        0,
                        int(max_place_actions or 0),
                    ),
                    max_total_actions=max(
                        0,
                        int(max_total_actions or 0),
                    ),
                    backlog_mode=backlog_mode,
                )
                cancel_actions = plan.cancel_actions
                if planned_level_indices_out is not None:
                    planned_level_indices_out[:] = [
                        int(level_index)
                        for level_index in plan.place_level_indices
                    ]
            elif planned_level_indices_out is not None:
                planned_level_indices_out[:] = [
                    int(level_index)
                    for level_index in planned_level_indices_out
                ]

            cancel_count = 0
            for cancel_action in cancel_actions:
                index = cancel_action.index
                order = active_orders[index]
                if self._is_cancel_reject_retry_blocked(
                    getattr(order, "client_order_id", None),
                    now_ns=now_ns,
                ):
                    continue
                target_px: Decimal | None = None
                match_tol: Decimal | None = None
                if index < len(desired_dec):
                    target_px = desired_dec[index][0]
                    match_tol = desired_dec[index][2]
                self._publish_order_intent(
                    intent_type="CANCEL",
                    client_order_id=str(getattr(order, "client_order_id", "")),
                    quote_cycle=quote_cycle,
                    quote_cycle_id=quote_cycle_id,
                    reason_code=cancel_action.reason_code,
                    side=side,
                    level_index=index,
                    target_px=target_px,
                    cancel_px=_price_to_decimal(order.price),
                    match_tol=match_tol,
                    ts_decision_ns=now_ns,
                    ts_cancel_request_local_ns=now_ns,
                    decision_context_json=decision_context_json,
                )
                self.cancel_order(order)
                self._track_pending_cancel(getattr(order, "client_order_id", None), now_ns=now_ns)
                cancel_count += 1

            return cancel_count

        def _place_missing_levels(
            self,
            *,
            side: OrderSide,
            active_orders: list[Order],
            desired_levels: list[tuple[Price, Decimal, Decimal]],
            best_bid_px: Decimal,
            best_ask_px: Decimal,
            now_ns: int | None = None,
            quote_cycle: QuoteCycleContext | None = None,
            quote_cycle_id: str | None = None,
            decision_context_json: dict[str, Any] | None = None,
            per_level_outcomes: list[dict[str, Any]] | None = None,
            level_indices: tuple[int, ...] | list[int] | None = None,
            pending_backlog_mode: str | None = None,
        ) -> int:
            if pending_backlog_mode is None:
                if self._has_pending_managed_cancels():
                    return 0
            elif str(pending_backlog_mode).lower() != "normal":
                return 0
            if now_ns is None:
                now_ns = int(self.clock.timestamp_ns())
            if self._has_active_cancel_reject_cooldown(
                now_ns=now_ns,
                managed_orders=active_orders,
            ):
                return 0
            places = 0
            active_prices = [_price_to_decimal(order.price) for order in active_orders]
            selected_level_indices = (
                range(len(desired_levels))
                if level_indices is None
                else [
                    int(level_index)
                    for level_index in level_indices
                    if 0 <= int(level_index) < len(desired_levels)
                ]
            )
            for level_index in selected_level_indices:
                target_price, cancel_px, match_tol = desired_levels[level_index]
                if self._terminal_order_denial_circuit_open or not self._effective_bot_on():
                    break
                target_px = _price_to_decimal(target_price)
                if side == OrderSide.BUY and target_px >= best_ask_px:
                    if per_level_outcomes is not None:
                        per_level_outcomes.append(
                            {
                                "side": _order_side_text(side),
                                "level_index": level_index,
                                "outcome": "skipped_crossed_book",
                            },
                        )
                    continue
                if side == OrderSide.SELL and target_px <= best_bid_px:
                    if per_level_outcomes is not None:
                        per_level_outcomes.append(
                            {
                                "side": _order_side_text(side),
                                "level_index": level_index,
                                "outcome": "skipped_crossed_book",
                            },
                        )
                    continue
                if any(abs(existing_px - target_px) <= match_tol for existing_px in active_prices):
                    if per_level_outcomes is not None:
                        per_level_outcomes.append(
                            {
                                "side": _order_side_text(side),
                                "level_index": level_index,
                                "outcome": "skipped_existing_match",
                            },
                        )
                    continue
                order = self.order_factory.limit(
                    instrument_id=self.config.maker_instrument_id,
                    order_side=side,
                    quantity=self._order_qty,
                    price=target_price,
                    post_only=True,
                )
                self.submit_order(
                    order,
                    allow_cash_borrowing=self._should_allow_cash_borrowing(side),
                )
                self._register_managed_order(order)
                self._publish_order_intent(
                    intent_type="PLACE",
                    client_order_id=str(getattr(order, "client_order_id", "")),
                    quote_cycle=quote_cycle,
                    quote_cycle_id=quote_cycle_id,
                    reason_code=REASON_PLACE_MISSING_LEVEL,
                    side=side,
                    level_index=level_index,
                    target_px=target_px,
                    cancel_px=cancel_px,
                    match_tol=match_tol,
                    ts_decision_ns=now_ns,
                    ts_submit_local_ns=now_ns,
                    decision_context_json=decision_context_json,
                )
                places += 1
                active_orders.append(order)
                active_prices.append(target_px)
                if per_level_outcomes is not None:
                    per_level_outcomes.append(
                        {
                            "side": _order_side_text(side),
                            "level_index": level_index,
                            "outcome": "placed",
                        },
                    )
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
            managed_orders = managed_orders_mod.collect_managed_orders(
                cache=self.cache,
                instrument_id=self.config.maker_instrument_id,
                strategy_id=self.id,
            )
            pending_cancel_ids = getattr(self, "_pending_cancel_client_order_ids", set())
            if not pending_cancel_ids:
                return managed_orders
            return [
                order
                for order in managed_orders
                if str(getattr(order, "client_order_id", "") or "") not in pending_cancel_ids
            ]

        def _publish_current_state_snapshot(self) -> None:
            current_state = getattr(self, "_last_state_name", None) or "running"
            self._publish_state(current_state, refresh_pricing_debug=False)

        def _cancel_managed_quotes(
            self,
            reason: str,
            force: bool = False,
            *,
            managed_orders: list[Order] | None = None,
            allow_instrument_cancel: bool | None = None,
            quote_cycle: QuoteCycleContext | None = None,
            quote_cycle_id: str | None = None,
            now_ns: int | None = None,
            reason_code: str | None = None,
            decision_context_json: dict[str, Any] | None = None,
        ) -> None:
            if managed_orders is None:
                managed_orders = self._managed_orders()
            requested_cancel_ids: set[str] = set()
            cancel_request_ns = int(self.clock.timestamp_ns()) if now_ns is None else int(now_ns)
            cancel_reason_code = reason_code or {
                "bot_off": REASON_CANCEL_BOT_OFF,
                "bot_off_flip": REASON_CANCEL_BOT_OFF_FLIP,
                "maker_book_unavailable": REASON_CANCEL_MAKER_BOOK_UNAVAILABLE,
                "maker_md_stale": REASON_CANCEL_MAKER_MD_STALE,
                "reference_md_stale": REASON_CANCEL_REFERENCE_MD_STALE,
                "no_targets": REASON_CANCEL_NO_TARGETS,
                "on_stop": REASON_CANCEL_ON_STOP,
                "quote_fail_circuit_breaker": REASON_CANCEL_QUOTE_FAIL_CIRCUIT_BREAKER,
            }.get(reason)

            if cancel_reason_code is not None:
                for order in managed_orders:
                    self._publish_order_intent(
                        intent_type="CANCEL",
                        client_order_id=str(getattr(order, "client_order_id", "")),
                        quote_cycle=quote_cycle,
                        quote_cycle_id=quote_cycle_id,
                        reason_code=cancel_reason_code,
                        side=getattr(order, "side", None),
                        level_index=None,
                        target_px=None,
                        cancel_px=getattr(order, "price", None),
                        match_tol=None,
                        ts_decision_ns=cancel_request_ns,
                        ts_cancel_request_local_ns=cancel_request_ns,
                        decision_context_json=decision_context_json,
                    )

            def _cancel_order(order: Order) -> None:
                self.cancel_order(order)
                client_order_id = str(getattr(order, "client_order_id", "") or "")
                if not client_order_id:
                    return
                requested_cancel_ids.add(client_order_id)
                self._track_pending_cancel(client_order_id, now_ns=cancel_request_ns)

            result = managed_orders_mod.cancel_managed_quotes(
                reason=reason,
                force=force,
                tracked_ids=self._managed_client_order_ids,
                managed_orders=managed_orders,
                maker_instrument_id=self.config.maker_instrument_id,
                cancel_order=_cancel_order,
                cancel_all_orders=self.cancel_all_orders,
                cancel_all_instrument_orders=bool(
                    getattr(self.config, "cancel_all_instrument_orders", False),
                ),
                allow_instrument_cancel=allow_instrument_cancel,
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
                pending_cancel_count=len(self._pending_cancel_client_order_ids),
                requested_cancel_ids=sorted(requested_cancel_ids),
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
            refresh_pricing_debug: bool = True,
        ) -> None:
            publisher_mod.publish_state(
                self,
                state,
                managed_orders_count=managed_orders_count,
                managed_orders=managed_orders,
                refresh_pricing_debug=refresh_pricing_debug,
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
            **extra_fields: Any,
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
                **extra_fields,
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
            **extra_fields: Any,
        ) -> None:
            publisher_mod.publish_alert(
                self,
                message,
                level,
                ts_ns=ts_ns,
                alert_key=alert_key,
                reason_code=reason_code,
                actionable=actionable,
                **extra_fields,
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
