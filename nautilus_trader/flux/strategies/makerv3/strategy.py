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

from nautilus_trader.flux.strategies.makerv3 import failures as failures_mod
from nautilus_trader.flux.strategies.makerv3 import inventory as inventory_mod
from nautilus_trader.flux.strategies.makerv3 import managed_orders as managed_orders_mod
from nautilus_trader.flux.strategies.makerv3 import market_data as market_data_mod
from nautilus_trader.flux.strategies.makerv3 import pricing as pricing_mod
from nautilus_trader.flux.strategies.makerv3 import publisher as publisher_mod
from nautilus_trader.flux.strategies.makerv3 import rebalancing as rebalancing_mod
from nautilus_trader.flux.strategies.makerv3 import runtime_params as runtime_params_mod
from nautilus_trader.flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_NAME
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_MD_STALE
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_REFERENCE_MD_STALE
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_FV
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_TRADE
from nautilus_trader.flux.strategies.makerv3.wire import build_quote_cycle_envelope
from nautilus_trader.flux.strategies.makerv3.wire import build_quote_cycle_id


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
    from nautilus_trader.flux.strategies.makerv3 import quote_engine as quote_engine_mod

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

        def on_start(self) -> None:
            """
            Start subscriptions, timers, and initial strategy publications.
            """
            self._runtime_params_failed = False
            self._quote_failure_circuit_open = False
            self._quote_failures_ns.clear()
            self._last_stale_cancel_ns = 0
            self._last_state_name = None
            self._state_is_blocked = False
            self._last_actionable_alert_ns.clear()
            self._last_actionable_alert_transition.clear()
            if self.config.maker_instrument_id == self.config.reference_instrument_id:
                self._publish_alert(
                    "maker_instrument_id and reference_instrument_id must be distinct",
                    level="error",
                )
                self.stop()
                return
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

            self._books = {
                self.config.maker_instrument_id: OrderBook(
                    instrument_id=self.config.maker_instrument_id,
                    book_type=BookType.L2_MBP,
                ),
                self.config.reference_instrument_id: OrderBook(
                    instrument_id=self.config.reference_instrument_id,
                    book_type=BookType.L2_MBP,
                ),
            }
            self._last_bbo = dict.fromkeys(self._books)
            self._last_bbo_ts_ns = dict.fromkeys(self._books, 0)
            self._last_market_bbo_publish_ns = dict.fromkeys(self._books, 0)

            self.subscribe_order_book_deltas(
                instrument_id=self.config.maker_instrument_id,
                book_type=BookType.L2_MBP,
            )
            self.subscribe_order_book_deltas(
                instrument_id=self.config.reference_instrument_id,
                book_type=BookType.L2_MBP,
            )

            self._publish_event("started")
            self._publish_balances()
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
            self.unsubscribe_order_book_deltas(instrument_id=self.config.maker_instrument_id)
            self.unsubscribe_order_book_deltas(instrument_id=self.config.reference_instrument_id)
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
            self._reconcile_managed_order(
                getattr(event, "client_order_id", None),
                lifecycle="rejected",
            )
            self.log.warning(
                f"Order rejected strategy_id={self._external_strategy_id} "
                f"client_order_id={getattr(event, 'client_order_id', None)}",
            )

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
        ) -> None:
            had_order = managed_orders_mod.reconcile_managed_order(
                self._managed_client_order_ids,
                client_order_id,
            )
            client_order_id_str = str(client_order_id or "")
            if not client_order_id_str:
                return
            self._publish_event(
                "order_lifecycle",
                lifecycle=lifecycle,
                client_order_id=client_order_id_str,
                tracked_before=had_order,
                tracked_after=len(self._managed_client_order_ids),
            )

        def _position_signed_qty(self) -> Decimal | None:
            positions: list[Position] = []
            with suppress(Exception):
                positions.extend(
                    self.cache.positions_open(
                        instrument_id=self.config.maker_instrument_id,
                    ),
                )
            return inventory_mod.position_signed_qty(positions)

        def _spot_balance_total(self, currency_code: str) -> Decimal | None:
            accounts: list[Account] = []
            with suppress(Exception):
                accounts.extend(list(self.cache.accounts()))
            if not accounts and hasattr(self, "portfolio"):
                try:
                    maker_venue = getattr(self.config.maker_instrument_id, "venue", None)
                    account = (
                        self.portfolio.account(venue=maker_venue)
                        if maker_venue is not None
                        else None
                    )
                except Exception:
                    account = None
                if account is not None:
                    accounts.append(account)
            return inventory_mod.spot_balance_total(accounts=accounts, currency_code=currency_code)

        def _maker_base_currency_code(self) -> str | None:
            instrument = self._maker_instrument
            if instrument is None:
                instrument = self._instruments.get(self.config.maker_instrument_id)
            return inventory_mod.maker_base_currency_code(
                instrument=instrument,
                instrument_id=self.config.maker_instrument_id,
            )

        def _compute_inventory_skew(
            self,
            *,
            runtime_params: Mapping[str, Any] | None = None,
        ) -> dict[str, Any]:
            position_qty = self._position_signed_qty()
            base_currency = self._maker_base_currency_code()
            spot_qty = self._spot_balance_total(base_currency) if base_currency else None
            if runtime_params is None:
                runtime_params = self._quote_runtime_params_snapshot()
            return inventory_mod.compute_inventory_skew(
                position_qty=position_qty,
                spot_qty=spot_qty,
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
        ) -> None:
            quote_engine_mod.publish_recovery_state_if_blocked(
                self,
                managed_orders_count=managed_orders_count,
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

        def _publish_state(self, state: str, *, managed_orders_count: int | None = None) -> None:
            publisher_mod.publish_state(self, state, managed_orders_count=managed_orders_count)

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
