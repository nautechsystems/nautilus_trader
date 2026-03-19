"""
Implement the first MakerV4 pure strategy core slice.
"""

from __future__ import annotations

from collections.abc import Mapping
from contextlib import suppress
from dataclasses import asdict
from decimal import Decimal
from types import SimpleNamespace
from typing import Any

from flux.common.quantity_units import exposure_from_venue_qty
from flux.common.keys import FluxRedisKeys
from flux.common.quantity_units import normalize_order_qty_unit
from flux.common.portfolio_inventory import DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
from flux.common.portfolio_inventory import StrategyInventoryComponent
from flux.common.portfolio_inventory import decode_portfolio_inventory
from flux.common.portfolio_inventory import encode_component
from flux.common.quantity_units import venue_qty_from_base_qty
from flux.strategies.makerv3 import inventory as inventory_mod
from flux.strategies.makerv3 import managed_orders as makerv3_managed_orders_mod
from flux.strategies.makerv3 import publisher as makerv3_publisher_mod
from flux.strategies.makerv3.constants import ALERT_COOLDOWN_RUNTIME_PARAMS_FAILURE_MS
from flux.strategies.makerv3.constants import ALERT_COOLDOWN_TERMINAL_ORDER_DENIED_MS
from flux.strategies.makerv3.constants import ALERT_COOLDOWN_VENUE_PROTECTION_CIRCUIT_BREAKER_MS
from flux.strategies.makerv3.constants import ALERT_KEY_RUNTIME_PARAMS_FAILURE
from flux.strategies.makerv3.constants import ALERT_KEY_VENUE_PROTECTION_CIRCUIT_BREAKER
from flux.strategies.makerv3.constants import TOPIC_EVENT
from flux.strategies.makerv3.constants import TOPIC_STATE
from flux.strategies.makerv4 import fees as fees_mod
from flux.strategies.makerv4 import publisher as publisher_mod
from flux.strategies.makerv4 import runtime_params as runtime_params_mod
from flux.strategies.makerv4.instruments import translate_hyperliquid_fill_to_ibkr_shares
from flux.strategies.makerv4.managed_orders import HedgeBacklogState
from flux.strategies.makerv4.managed_orders import HedgeOrderIntent
from flux.strategies.makerv4.managed_orders import ManagedMakerOrderState
from flux.strategies.makerv4.managed_orders import PendingHedgeState
from flux.strategies.makerv4.market_data import IbkrQuoteSnapshot
from flux.strategies.makerv4.pricing import build_effective_ibkr_fee_bps
from flux.strategies.makerv4.pricing import build_fee_assumptions
from flux.strategies.makerv4.pricing import build_maker_quote_price
from flux.strategies.makerv4.pricing import build_take_take_limit_price
from flux.strategies.makerv4.pricing import build_ibkr_ioc_limit
from flux.strategies.makerv4.pricing import validate_ibkr_quote
from flux.strategies.shared.account_projection_positions import (
    read_matching_shared_account_position_row,
)
from flux.strategies.shared import alerts as shared_alerts_mod
from flux.strategies.shared.ibkr_order_policy import build_ibkr_hedge_order_policy
from flux.strategies.shared.ibkr_order_policy import is_us_equities_regular_session
from flux.strategies.shared.ibkr_tags import build_ibkr_order_tags
from flux.strategies.shared.publisher_common import build_role_map_payload
from flux.strategies.shared.quote_health import evaluate_quote_health
from flux.strategies.shared.trades import publish_trade as publish_shared_trade
from flux.strategies.shared.venue_protection import extract_hyperliquid_request_quota
from flux.strategies.shared.venue_protection import is_venue_protection_reason
from flux.strategies.shared.venue_protection import normalize_reason_text
from nautilus_trader.adapters.interactive_brokers.common import IB_CLIENT_ID
from flux.strategies.makerv4.wire import HedgeExecutionReport
from flux.strategies.makerv4.wire import MakerFill
from flux.strategies.makerv3.strategy import MakerV3StrategyConfig
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy


def _normalized_timestamp_ms(value: Any) -> int:
    try:
        ts_value = int(value or 0)
    except Exception:
        return 0
    while ts_value > 10_000_000_000_000:
        ts_value //= 1_000
    return max(0, ts_value)


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
    ibkr_hedge_route: str = ""
    hedge_fee_plan: str = "ibkr_pro_tiered"


class MakerV4Strategy(Strategy):
    """
    MakerV4 hedge strategy core wrapped in the Nautilus Strategy lifecycle.
    """

    BALANCES_PUBLISH_INTERVAL_MS = 10_000
    PARAMS_REFRESH_INTERVAL_MS = 500
    TAKE_TAKE_ORDER_TRACK_LIMIT = 64
    TAKE_TAKE_ORDER_TTL_NS = 5 * 60 * 1_000_000_000

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
        self._portfolio_inventory_client = None
        self._portfolio_inventory_portfolio_id: str | None = None
        self._portfolio_inventory_namespace = "flux"
        self._portfolio_inventory_schema_version = "v1"
        self._portfolio_inventory_stale_after_ms = DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
        self._portfolio_inventory_allow_partial_global_risk = False
        execution_account_scope_id = str(
            getattr(config, "execution_account_scope_id", "") or "",
        ).strip()
        self._profile_account_projection_client: Any | None = None
        self._profile_account_projection_profile_id: str | None = None
        self._profile_account_projection_account_scope_id = (
            execution_account_scope_id or None
        )
        self._profile_account_projection_namespace = "flux"
        self._profile_account_projection_schema_version = "v1"
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
        self._last_venue_protection: dict[str, Any] = {}
        self._reference_balance_snapshot_provider = None
        self.tradeable = True
        self.hedge_disabled_reason: str | None = None
        self._managed_maker_orders: dict[str, ManagedMakerOrderState] = {}
        self._pending_hedge: PendingHedgeState | None = None
        self._hedge_backlog: HedgeBacklogState | None = None
        self._hedge_requests: list[HedgeOrderIntent] = []
        self._seen_fill_ids: set[str] = set()
        self._fill_ids_head: list[str] = []
        self._last_take_submission_ns = 0
        self._recent_take_take_order_ids: dict[str, int] = {}
        self._take_take_fill_accumulators: dict[str, dict[str, Any]] = {}
        self._take_take_residual_base_fill: dict[str, Any] | None = None
        self._last_runtime_params_refresh_ns = 0
        self._last_actionable_alert_ns: dict[str, int] = {}
        self._last_actionable_alert_transition: dict[str, str] = {}

    def set_params_manager_factory(self, factory) -> None:
        self._params_manager_factory = factory

    def configure_portfolio_inventory_feed(self, **kwargs) -> None:
        self._portfolio_inventory_feed = dict(kwargs)
        self._portfolio_inventory_client = kwargs.get("redis_client")
        portfolio_id = str(kwargs.get("portfolio_id", "")).strip()
        self._portfolio_inventory_portfolio_id = portfolio_id or None
        self._portfolio_inventory_namespace = str(
            kwargs.get("namespace", self._portfolio_inventory_namespace),
        )
        self._portfolio_inventory_schema_version = str(
            kwargs.get("schema_version", self._portfolio_inventory_schema_version),
        )
        self._portfolio_inventory_stale_after_ms = max(
            1,
            int(kwargs.get("stale_after_ms", DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS)),
        )
        self._portfolio_inventory_allow_partial_global_risk = bool(
            kwargs.get("allow_partial_global_risk", False),
        )

    def configure_reference_balance_snapshot_provider(self, provider: Any) -> None:
        self._reference_balance_snapshot_provider = provider

    def configure_profile_account_projection_feed(
        self,
        *,
        redis_client: Any,
        profile_id: str,
        account_scope_id: str,
        namespace: str,
        schema_version: str,
    ) -> None:
        self._profile_account_projection_client = redis_client
        self._profile_account_projection_profile_id = profile_id.strip() or None
        self._profile_account_projection_account_scope_id = account_scope_id.strip() or None
        self._profile_account_projection_namespace = namespace
        self._profile_account_projection_schema_version = schema_version

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

    def _open_positions(self) -> list[Any] | None:
        cache = self._strategy_cache()
        positions_open = getattr(cache, "positions_open", None)
        if not callable(positions_open):
            return None
        strategy_id = getattr(self, "id", None)
        if strategy_id is not None:
            with suppress(Exception):
                return list(positions_open(strategy_id=strategy_id))
            with suppress(Exception):
                return list(positions_open(None, None, strategy_id))
        with suppress(Exception):
            return list(positions_open())
        return None

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

    def _publish_runtime_params_failure(
        self,
        *,
        context: str,
        exc: Exception,
        now_ns: int | None = None,
    ) -> None:
        message = f"runtime_params_failure[{context}] {type(exc).__name__}: {exc}"
        with suppress(Exception):
            self.log.error(message)
        with suppress(Exception):
            self._publish_event(
                "runtime_params_failure",
                ts_ns=now_ns,
                context=context,
                error_type=type(exc).__name__,
                error_message=str(exc),
            )
        with suppress(Exception):
            self._publish_actionable_alert(
                alert_key=ALERT_KEY_RUNTIME_PARAMS_FAILURE,
                message=message,
                level="error",
                reason_code=ALERT_KEY_RUNTIME_PARAMS_FAILURE,
                cooldown_ms=ALERT_COOLDOWN_RUNTIME_PARAMS_FAILURE_MS,
                transition=context,
                now_ns=now_ns,
            )

    def _event_cache_order(self, event: Any) -> Any | None:
        client_order_id = getattr(event, "client_order_id", None)
        if client_order_id is None:
            return None
        client_order_id_text = str(client_order_id).strip()
        if not client_order_id_text:
            return None
        cache = self._strategy_cache()
        lookup = getattr(cache, "order", None)
        if not callable(lookup):
            return None
        with suppress(Exception):
            return lookup(client_order_id)
        with suppress(Exception):
            return lookup(client_order_id_text)
        return None

    @staticmethod
    def _has_market_exit_tag(value: Any) -> bool:
        tags = getattr(value, "tags", None)
        if tags in (None, ""):
            return False
        try:
            return "MARKET_EXIT" in tags
        except Exception:
            return False

    def _event_has_market_exit_tag(self, event: Any) -> bool:
        if self._has_market_exit_tag(event):
            return True
        return self._has_market_exit_tag(self._event_cache_order(event))

    def _market_exit_trade_role(self, instrument_id: Any) -> str:
        if self._instrument_id_matches(instrument_id, self.config.maker_instrument_id):
            return "maker"
        if self._instrument_id_matches(instrument_id, self._hedge_instrument_id(self._hedge_route())):
            return "hedge"
        if self._instrument_id_matches(instrument_id, self.config.reference_instrument_id):
            return "hedge"
        return "trade"

    def _publish_market_exit_alert(
        self,
        *,
        alert_key: str,
        message: str,
        now_ns: int,
        transition: str,
        **extra_fields: Any,
    ) -> None:
        self._publish_actionable_alert(
            alert_key=alert_key,
            message=message,
            level="critical",
            reason_code=alert_key,
            cooldown_ms=0,
            transition=transition,
            now_ns=now_ns,
            market_exit=True,
            **extra_fields,
        )

    def _refresh_runtime_params_if_due(self, *, now_ns: int, force: bool = False) -> None:
        normalized_now_ns = max(0, int(now_ns))
        if (
            not force
            and self._last_runtime_params_refresh_ns > 0
            and normalized_now_ns - self._last_runtime_params_refresh_ns
            < self.PARAMS_REFRESH_INTERVAL_MS * 1_000_000
        ):
            return

        bot_on_before = self._effective_bot_on()
        try:
            self._load_runtime_params()
        except Exception as exc:
            self._publish_runtime_params_failure(
                context="refresh",
                exc=exc,
                now_ns=normalized_now_ns,
            )
            self._last_runtime_params_refresh_ns = normalized_now_ns
            return
        self._last_runtime_params_refresh_ns = normalized_now_ns

        if bot_on_before and not self._effective_bot_on():
            self._cancel_managed_maker_orders()

    def _effective_bot_on(self) -> bool:
        bot_on = self._runtime_params.get("bot_on", getattr(self.config, "bot_on", False))
        return bool(bot_on)

    def _execution_mode(self) -> str:
        mode = str(self._runtime_params.get("execution_mode", "maker_hedge")).strip().lower()
        return mode if mode in {"maker_hedge", "take_take"} else "maker_hedge"

    def _can_quote(self) -> bool:
        return (
            self.tradeable
            and self._effective_bot_on()
            and self._pending_hedge is None
            and self._hedge_backlog is None
        )

    def _managed_orders(self) -> list[Any]:
        managed: list[Any] = list(self._managed_maker_orders.values())
        if self._pending_hedge is not None:
            managed.append(self._pending_hedge)
        return managed

    def _tracked_managed_order_count(self) -> int:
        return len(self._managed_orders())

    @staticmethod
    def _enum_name(value: Any) -> str:
        name = getattr(value, "name", None)
        if isinstance(name, str) and name:
            return name.upper()
        return str(value).strip().upper()

    @staticmethod
    def _order_side_enum(side: str) -> OrderSide:
        return OrderSide.BUY if str(side).strip().upper() == "BUY" else OrderSide.SELL

    def _make_order_quantity(self, instrument: Any, qty: Decimal) -> Any:
        make_qty = getattr(instrument, "make_qty", None)
        if callable(make_qty):
            with suppress(Exception):
                return make_qty(qty)
        return qty

    def _make_order_price(self, instrument: Any, price: Decimal) -> Any:
        make_price = getattr(instrument, "make_price", None)
        if callable(make_price):
            with suppress(Exception):
                return make_price(price)
        return price

    def _maker_order_qty(self) -> Decimal:
        venue_qty, _base_qty = self._maker_order_quantities()
        return venue_qty

    def _maker_order_base_qty(self) -> Decimal:
        _venue_qty, base_qty = self._maker_order_quantities()
        return base_qty

    def _maker_order_quantities(self) -> tuple[Decimal, Decimal]:
        raw_qty = self._runtime_params.get("qty", getattr(self.config, "order_qty", Decimal("0")))
        configured_qty = abs(Decimal(str(raw_qty)))
        instrument = self._resolve_instrument(self.config.maker_instrument_id)
        if instrument is None:
            return configured_qty, configured_qty

        qty_unit = normalize_order_qty_unit(
            getattr(self.config, "qty_unit", "venue"),
            context=self._external_strategy_id,
        )
        last_px = self._maker_order_last_px()
        try:
            if qty_unit == "base":
                exposure = venue_qty_from_base_qty(instrument, configured_qty, last_px=last_px)
                if exposure.venue_qty is None:
                    raise RuntimeError(
                        "Failed to convert configured qty to venue quantity for "
                        f"{self._external_strategy_id}: qty={configured_qty} qty_unit={qty_unit} "
                        f"status={exposure.qty_conversion_status} "
                        f"source={exposure.qty_conversion_source}",
                    )
                return abs(exposure.venue_qty), configured_qty

            exposure = exposure_from_venue_qty(instrument, configured_qty, last_px=last_px)
            base_qty = exposure.base_qty if exposure.base_qty is not None else configured_qty
            return configured_qty, abs(base_qty)
        except Exception as exc:
            self.log.error(f"Failed MakerV4 qty conversion for {self._external_strategy_id}: {exc}")
            return Decimal("0"), Decimal("0")

    def _maker_order_last_px(self) -> Decimal | None:
        snapshot = self._latest_quotes.get(self.config.maker_instrument_id)
        if not isinstance(snapshot, Mapping):
            return None
        bid = self._decimal_or_none(snapshot.get("bid"))
        ask = self._decimal_or_none(snapshot.get("ask"))
        if bid is not None and ask is not None:
            return (bid + ask) / Decimal("2")
        return ask or bid

    def _maker_order_hedge_qty(self) -> Decimal:
        return abs(
            translate_hyperliquid_fill_to_ibkr_shares(
                fill_qty=self._maker_order_base_qty(),
                min_share_increment=Decimal(
                    str(getattr(self.config, "hedge_min_share_increment", Decimal("1")))
                ),
            )
        )

    def _maker_tick_size(self) -> Decimal:
        instrument = self._resolve_instrument(self.config.maker_instrument_id)
        tick_size = self._decimal_or_none(getattr(instrument, "price_increment", None))
        return tick_size or Decimal("0.01")

    def _managed_maker_orders_payload(self) -> list[dict[str, Any]]:
        return [
            {
                "client_order_id": row.client_order_id,
                "instrument_id": row.instrument_id,
                "side": row.side,
            }
            for row in self._managed_maker_orders.values()
        ]

    def _pending_hedge_payload(self) -> dict[str, Any] | None:
        if self._pending_hedge is None:
            return None
        return {
            "client_order_id": self._pending_hedge.order_id,
            "instrument_id": str(self._hedge_instrument_id(self._pending_hedge.route)),
            "route": self._pending_hedge.route,
            "side": self._pending_hedge.side,
            "time_in_force": self._pending_hedge.time_in_force,
            "outside_rth": self._pending_hedge.outside_rth,
            "include_overnight": self._pending_hedge.include_overnight,
            "cancel_after_ms": self._pending_hedge.cancel_after_ms,
            "remaining_qty": makerv3_publisher_mod.decimal_to_json_str(
                self._pending_hedge.remaining_qty,
            ),
        }

    def _hedge_backlog_payload(self) -> dict[str, Any] | None:
        if self._hedge_backlog is None:
            return None
        return {
            "fill_id": self._hedge_backlog.fill_id,
            "side": self._hedge_backlog.side,
            "requested_qty": makerv3_publisher_mod.decimal_to_json_str(
                self._hedge_backlog.requested_qty,
            ),
            "blocked_reason": self._hedge_backlog.blocked_reason,
            "fill_ts_ms": self._hedge_backlog.fill_ts_ms,
            "maker_fee_bps": makerv3_publisher_mod.decimal_to_json_str(
                self._hedge_backlog.maker_fee_bps,
            ),
        }

    def _maker_quote_status_payload(self) -> dict[str, int]:
        bid_open = 1 if "BUY" in self._managed_maker_orders else 0
        ask_open = 1 if "SELL" in self._managed_maker_orders else 0
        return {
            "bid_open": bid_open,
            "ask_open": ask_open,
            "bid_depth": bid_open,
            "ask_depth": ask_open,
            "bid_blocked": 0,
            "ask_blocked": 0,
        }

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
    def _signed_decimal_or_none(value: Any) -> Decimal | None:
        if value is None:
            return None
        as_decimal = getattr(value, "as_decimal", None)
        if callable(as_decimal):
            with suppress(Exception):
                parsed = as_decimal()
                if parsed is None:
                    return None
                return Decimal(str(parsed))
        try:
            return Decimal(str(value))
        except Exception:
            return None

    def _best_mid(self, instrument_id: Any) -> Decimal | None:
        snapshot = self._latest_quotes.get(instrument_id)
        if not isinstance(snapshot, dict):
            return None
        bid = self._decimal_or_none(snapshot.get("bid"))
        ask = self._decimal_or_none(snapshot.get("ask"))
        if bid is None or ask is None:
            return None
        return (bid + ask) / Decimal("2")

    def _inventory_base_exposure_last_px(self) -> Decimal | None:
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
        positions = self._open_positions()
        if positions is None:
            return inventory_mod.PositionExposureSummary(venue_qty=None, base_qty=None)
        cache = self._strategy_cache()
        return inventory_mod.position_exposure_summary(
            positions,
            base_currency=currency_code,
            instrument_lookup=self._resolve_instrument if cache is not None else None,
            venue=venue,
            instrument_id=instrument_id,
            last_px=self._inventory_base_exposure_last_px(),
        )

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

    def _spot_balance_total(
        self,
        currency_code: str,
        *,
        venue: Any | None = None,
    ) -> Decimal | None:
        if not currency_code:
            return None

        accounts: list[Any] = []
        cache = self._strategy_cache()
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

    def _portfolio_asset_id(self) -> str | None:
        instrument = self._maker_instrument
        if instrument is None:
            instrument = self._instruments.get(self.config.maker_instrument_id)
        return inventory_mod.portfolio_asset_id(
            configured_asset_id=getattr(self.config, "portfolio_asset_id", None),
            instrument=instrument,
            instrument_id=self.config.maker_instrument_id,
        )

    def _position_summary_from_snapshot(
        self,
        snapshot: Mapping[str, Any],
    ) -> inventory_mod.PositionExposureSummary:
        signed_qty = self._signed_decimal_or_none(
            snapshot.get("signed_qty_venue") or snapshot.get("signed_qty"),
        )
        if signed_qty is None:
            return inventory_mod.PositionExposureSummary(venue_qty=None, base_qty=None)
        snapshot_base_qty_raw = snapshot.get("signed_qty_base")
        if snapshot_base_qty_raw is None:
            snapshot_base_qty_raw = snapshot.get("base_qty")
        snapshot_base_qty = self._signed_decimal_or_none(snapshot_base_qty_raw)
        snapshot_conversion_status = str(
            snapshot.get("qty_conversion_status") or "",
        ).strip() or None
        snapshot_conversion_source = str(
            snapshot.get("qty_conversion_source") or "",
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
        exposure = inventory_mod.base_exposure_from_venue_qty(
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

    @staticmethod
    def _position_summary_has_nonzero_inventory(
        summary: inventory_mod.PositionExposureSummary,
    ) -> bool:
        return any(qty is not None and qty != 0 for qty in (summary.venue_qty, summary.base_qty))

    def _shared_account_maker_position_snapshot(
        self,
        *,
        now_ms_value: int,
    ) -> dict[str, Any] | None:
        client = self._profile_account_projection_client
        profile_id = self._profile_account_projection_profile_id
        account_scope_id = self._profile_account_projection_account_scope_id
        if client is None or profile_id is None or account_scope_id is None:
            return None
        row = read_matching_shared_account_position_row(
            redis_client=client,
            profile_id=profile_id,
            account_scope_id=account_scope_id,
            instrument_id=str(self.config.maker_instrument_id),
            namespace=self._profile_account_projection_namespace,
            schema_version=self._profile_account_projection_schema_version,
        )
        if not isinstance(row, Mapping):
            return None
        signed_qty = self._signed_decimal_or_none(
            row.get("signed_qty_venue") or row.get("signed_qty"),
        )
        if signed_qty is None:
            return None
        projection_ts_ms = _normalized_timestamp_ms(row.get("server_ts_ms"))
        if projection_ts_ms <= 0:
            return None
        if now_ms_value - projection_ts_ms > max(1, self._portfolio_inventory_stale_after_ms):
            return None
        return dict(row)

    def _local_position_context(
        self,
        base_currency: str | None,
        *,
        now_ms_value: int,
    ) -> tuple[inventory_mod.PositionExposureSummary, str | None]:
        if not base_currency or self._maker_instrument_is_spot():
            return inventory_mod.PositionExposureSummary(venue_qty=None, base_qty=None), None
        position_summary = self._position_exposure_summary(
            base_currency,
            instrument_id=self.config.maker_instrument_id,
        )
        if self._position_summary_has_nonzero_inventory(position_summary):
            return position_summary, "positions"
        shared_snapshot = self._shared_account_maker_position_snapshot(now_ms_value=now_ms_value)
        if shared_snapshot is not None:
            return (
                self._position_summary_from_snapshot(shared_snapshot),
                "shared_account_projection",
            )
        if position_summary.venue_qty is not None or position_summary.base_qty is not None:
            return position_summary, "positions"
        return position_summary, None

    def _local_position_summary(
        self,
        base_currency: str | None,
        *,
        now_ms_value: int,
    ) -> inventory_mod.PositionExposureSummary:
        summary, _source = self._local_position_context(
            base_currency,
            now_ms_value=now_ms_value,
        )
        return summary

    def _local_spot_qty(self, base_currency: str | None) -> Decimal | None:
        if not base_currency or not self._maker_instrument_is_spot():
            return None
        maker_venue = getattr(self.config.maker_instrument_id, "venue", None)
        return self._spot_balance_total(base_currency, venue=maker_venue)

    def _shared_portfolio_inventory_snapshot(
        self,
        *,
        base_currency: str | None,
        now_ms_value: int,
    ) -> tuple[Decimal | None, dict[str, Any] | None]:
        portfolio_id = self._portfolio_inventory_portfolio_id
        client = self._portfolio_inventory_client
        if not base_currency or not portfolio_id or client is None:
            return None, None
        key = FluxRedisKeys.portfolio_inventory(
            portfolio_id=portfolio_id,
            base_currency=base_currency,
            namespace=self._portfolio_inventory_namespace,
            schema_version=self._portfolio_inventory_schema_version,
        )
        with suppress(Exception):
            payload = decode_portfolio_inventory(client.get(key))
            if not isinstance(payload, dict):
                return None, None
            ts_ms = int(payload.get("ts_ms") or 0)
            stale_after_ms = int(
                payload.get("stale_after_ms") or self._portfolio_inventory_stale_after_ms,
            )
            if ts_ms <= 0 or now_ms_value - ts_ms > max(1, stale_after_ms):
                return None, {
                    "aggregation_mode": str(payload.get("aggregation_mode") or "strict"),
                    "global_qty_base_complete": False,
                    "global_qty_complete": False,
                }
            global_qty = makerv3_publisher_mod._to_decimal_or_none(
                payload.get("global_qty_base") or payload.get("global_qty"),
            )
            diagnostics = {
                "aggregation_mode": str(payload.get("aggregation_mode") or "strict"),
                "global_qty_base_complete": bool(
                    payload.get(
                        "global_qty_base_complete",
                        payload.get("global_qty_complete", True),
                    ),
                ),
                "global_qty_complete": bool(
                    payload.get(
                        "global_qty_base_complete",
                        payload.get("global_qty_complete", True),
                    ),
                ),
            }
            incomplete = not bool(diagnostics["global_qty_base_complete"])
            if incomplete and not self._portfolio_inventory_allow_partial_global_risk:
                return None, diagnostics
            return global_qty, diagnostics
        return None, None

    def _inventory_contract_snapshot(self, *, now_ms_value: int) -> dict[str, Any]:
        maker_base_currency = self._maker_base_currency_code()
        portfolio_asset_id = self._portfolio_asset_id()
        local_position_summary, local_position_source = self._local_position_context(
            maker_base_currency,
            now_ms_value=now_ms_value,
        )
        local_spot_qty = self._local_spot_qty(maker_base_currency)
        local_qty_base = inventory_mod.local_inventory_total(
            local_position_qty=local_position_summary.base_qty,
            local_spot_qty=local_spot_qty,
        )
        global_qty_base, diagnostics = self._shared_portfolio_inventory_snapshot(
            base_currency=portfolio_asset_id,
            now_ms_value=now_ms_value,
        )
        local_inventory_source = "unavailable"
        if local_position_source == "shared_account_projection":
            local_inventory_source = "shared_account_projection"
        elif local_position_summary.base_qty is not None and local_spot_qty is not None:
            local_inventory_source = "positions_plus_spot"
        elif local_spot_qty is not None:
            local_inventory_source = "spot_balance"
        elif local_position_summary.base_qty is not None:
            local_inventory_source = local_position_source or "positions"
        global_complete = None if diagnostics is None else bool(diagnostics["global_qty_base_complete"])
        global_inventory_source = None
        if global_qty_base is not None:
            global_inventory_source = (
                "portfolio_component_sum"
                if global_complete is not False
                else "portfolio_component_partial_sum"
            )
        return {
            "base_currency": portfolio_asset_id,
            "local_position_summary": local_position_summary,
            "local_spot_qty": local_spot_qty,
            "local_qty_base": local_qty_base,
            "local_inventory_source": local_inventory_source,
            "global_qty_base": global_qty_base,
            "global_inventory_source": global_inventory_source,
            "diagnostics": diagnostics,
        }

    def _publish_portfolio_inventory_component(
        self,
        *,
        state: str,
        now_ms_value: int,
        inventory_snapshot: dict[str, Any] | None = None,
    ) -> None:
        portfolio_id = self._portfolio_inventory_portfolio_id
        client = self._portfolio_inventory_client
        if not portfolio_id or client is None:
            return
        snapshot = (
            inventory_snapshot
            if isinstance(inventory_snapshot, dict)
            else self._inventory_contract_snapshot(now_ms_value=now_ms_value)
        )
        base_currency = snapshot.get("base_currency")
        if not base_currency:
            return
        local_position_summary = snapshot["local_position_summary"]
        component = StrategyInventoryComponent(
            strategy_id=self._external_strategy_id,
            portfolio_id=portfolio_id,
            base_currency=str(base_currency),
            local_qty_base=snapshot.get("local_qty_base"),
            ts_ms=now_ms_value,
            local_position_qty_venue=local_position_summary.venue_qty,
            local_position_qty_base=local_position_summary.base_qty,
            local_spot_qty=snapshot.get("local_spot_qty"),
            qty_conversion_status=local_position_summary.qty_conversion_status,
            qty_conversion_source=local_position_summary.qty_conversion_source,
            stale_after_ms=self._portfolio_inventory_stale_after_ms,
            maker_instrument_id=str(self.config.maker_instrument_id),
            state=state,
        )
        key = FluxRedisKeys.portfolio_inventory_component(
            strategy_id=self._external_strategy_id,
            portfolio_id=portfolio_id,
            base_currency=str(base_currency),
            namespace=self._portfolio_inventory_namespace,
            schema_version=self._portfolio_inventory_schema_version,
        )
        with suppress(Exception):
            client.set(key, encode_component(component))

    def _inventory_state_fields(
        self,
        *,
        now_ms_value: int,
        inventory_snapshot: dict[str, Any] | None = None,
    ) -> tuple[dict[str, Any], dict[str, Any] | None]:
        snapshot = (
            inventory_snapshot
            if isinstance(inventory_snapshot, dict)
            else self._inventory_contract_snapshot(now_ms_value=now_ms_value)
        )
        local_position_summary = snapshot["local_position_summary"]
        diagnostics = snapshot.get("diagnostics")
        state_fields: dict[str, Any] = {}
        skew_fields: dict[str, Any] = {
            "base_currency": snapshot.get("base_currency"),
            "position_qty_venue": makerv3_publisher_mod.decimal_to_json_str(
                local_position_summary.venue_qty,
            ),
            "position_qty_base": makerv3_publisher_mod.decimal_to_json_str(
                local_position_summary.base_qty,
            ),
            "position_qty_complete": bool(local_position_summary.qty_complete),
            "position_qty": makerv3_publisher_mod.decimal_to_json_str(
                local_position_summary.base_qty,
            ),
            "local_position_qty_venue": makerv3_publisher_mod.decimal_to_json_str(
                local_position_summary.venue_qty,
            ),
            "local_position_qty_base": makerv3_publisher_mod.decimal_to_json_str(
                local_position_summary.base_qty,
            ),
            "local_position_qty_complete": bool(local_position_summary.qty_complete),
            "local_position_qty": makerv3_publisher_mod.decimal_to_json_str(
                local_position_summary.base_qty,
            ),
            "local_spot_qty": makerv3_publisher_mod.decimal_to_json_str(
                snapshot.get("local_spot_qty"),
            ),
            "local_inventory_qty_base": makerv3_publisher_mod.decimal_to_json_str(
                snapshot.get("local_qty_base"),
            ),
            "local_inventory_qty_complete": bool(local_position_summary.qty_complete),
            "local_inventory_qty": makerv3_publisher_mod.decimal_to_json_str(
                snapshot.get("local_qty_base"),
            ),
            "local_inventory_source": snapshot.get("local_inventory_source"),
        }
        if local_position_summary.venue_qty is not None:
            state_fields["position_qty_venue"] = skew_fields["position_qty_venue"]
        if local_position_summary.base_qty is not None:
            state_fields["position_qty_base"] = skew_fields["position_qty_base"]
        if snapshot.get("local_qty_base") is not None:
            state_fields["local_qty_base"] = skew_fields["local_inventory_qty_base"]
            state_fields["local_qty"] = skew_fields["local_inventory_qty"]
        if local_position_summary.qty_conversion_status is not None:
            state_fields["qty_conversion_status"] = local_position_summary.qty_conversion_status
            state_fields["qty_conversion_source"] = local_position_summary.qty_conversion_source
            skew_fields["local_position_qty_conversion_status"] = (
                local_position_summary.qty_conversion_status
            )
            skew_fields["local_position_qty_conversion_source"] = (
                local_position_summary.qty_conversion_source
            )

        global_qty_base = snapshot.get("global_qty_base")
        if global_qty_base is not None:
            encoded_global_qty = makerv3_publisher_mod.decimal_to_json_str(global_qty_base)
            state_fields["global_qty_base"] = encoded_global_qty
            state_fields["global_qty"] = encoded_global_qty
            skew_fields["inventory_qty_base"] = encoded_global_qty
            skew_fields["inventory_qty"] = encoded_global_qty
            skew_fields["inventory_source"] = snapshot.get("global_inventory_source")
            skew_fields["global_inventory_qty_base"] = encoded_global_qty
            skew_fields["global_inventory_qty"] = encoded_global_qty
            skew_fields["global_inventory_source"] = snapshot.get("global_inventory_source")
        if diagnostics is not None:
            global_complete = bool(diagnostics.get("global_qty_base_complete", True))
            state_fields["global_qty_base_complete"] = global_complete
            state_fields["global_qty_complete"] = global_complete
            state_fields["aggregation_mode"] = str(diagnostics.get("aggregation_mode") or "strict")
            skew_fields["global_inventory_qty_base_complete"] = global_complete
            skew_fields["global_inventory_qty_complete"] = global_complete
            skew_fields["global_inventory_aggregation_mode"] = str(
                diagnostics.get("aggregation_mode") or "strict",
            )

        return state_fields, skew_fields

    @staticmethod
    def _quote_ts_ns(value: Any) -> int:
        try:
            return int(value)
        except Exception:
            return 0

    @staticmethod
    def _required_decimal(value: Any, *, field_name: str) -> Decimal:
        as_decimal = getattr(value, "as_decimal", None)
        if callable(as_decimal):
            with suppress(Exception):
                return Decimal(str(as_decimal()))
        return Decimal(str(value))

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

    @staticmethod
    def _coerce_instrument_id(reference_instrument_id: Any, instrument_id_text: str) -> Any:
        text = str(instrument_id_text).strip()
        if not text:
            return reference_instrument_id
        with suppress(Exception):
            return InstrumentId.from_str(text)
        return text

    def _hedge_route(self) -> str | None:
        return (
            str(self._last_pricing_debug.get("hedge_route", "")).strip().upper()
            or str(getattr(self.config, "ibkr_hedge_route", "")).strip().upper()
            or None
        )

    def _fee_assumptions_payload(self) -> dict[str, Any]:
        assumptions = self._fee_assumptions()
        return {
            "ibkr_fee_plan": assumptions.ibkr_fee_plan,
            "ibkr_fee_min_usd": float(assumptions.ibkr_fee_min_usd),
            "maker_taker_fee_bps": float(assumptions.maker_taker_fee_bps),
            "maker_maker_fee_bps": float(assumptions.maker_maker_fee_bps),
            "assumed_hedge_fee_bps": float(assumptions.assumed_hedge_fee_bps),
        }

    def _fee_assumptions(self):
        legacy_hedge_fee_plan = str(self._runtime_params.get("hedge_fee_plan", "")).strip().lower()
        default_ibkr_fee_plan = "tiered" if legacy_hedge_fee_plan == "ibkr_pro_tiered" else "fixed"
        return build_fee_assumptions(
            ibkr_fee_plan=str(
                self._runtime_params.get("ibkr_fee_plan", default_ibkr_fee_plan),
            ),
            ibkr_fee_min_usd=self._runtime_params.get("ibkr_fee_min_usd", 0.35),
            maker_taker_fee_bps=self._runtime_params.get(
                "maker_taker_fee_bps",
                self._runtime_params.get("hl_taker_fee_bps", 4.5),
            ),
            maker_maker_fee_bps=self._runtime_params.get(
                "maker_maker_fee_bps",
                self._runtime_params.get("hl_maker_fee_bps", 0.25),
            ),
            assumed_hedge_fee_bps=self._runtime_params.get("assumed_hedge_fee_bps", 1.0),
        )

    @staticmethod
    def _hedge_route_requires_include_overnight(hedge_route: str | None) -> bool:
        return str(hedge_route or "").strip().upper() in {"BLUEOCEAN", "OVERNIGHT", "IBEOS"}

    @staticmethod
    def _time_in_force_enum(time_in_force: str) -> TimeInForce:
        normalized = str(time_in_force).strip().upper()
        return {
            "DAY": TimeInForce.DAY,
            "IOC": TimeInForce.IOC,
            "FOK": TimeInForce.FOK,
            "GTC": TimeInForce.GTC,
        }.get(normalized, TimeInForce.DAY)

    def _is_regular_hedge_session(self, *, ts_ms: int) -> bool:
        return is_us_equities_regular_session(int(ts_ms))

    def _hedge_policy_payload(self, *, now_ns: int) -> dict[str, Any]:
        policy = build_ibkr_hedge_order_policy(
            configured_route=self._hedge_route(),
            outside_rth_enabled=bool(getattr(self.config, "outside_rth_hedge_enabled", False)),
            is_regular_session=self._is_regular_hedge_session(
                ts_ms=max(0, int(now_ns)) // 1_000_000,
            ),
            hedge_mode=self._execution_mode(),
        )
        return {
            "route": policy.route,
            "time_in_force": policy.time_in_force,
            "outside_rth": policy.outside_rth,
            "include_overnight": policy.include_overnight,
            "cancel_after_ms": policy.cancel_after_ms,
        }

    def _hedge_instrument_id(self, hedge_route: str | None = None) -> Any:
        explicit = str(self._last_pricing_debug.get("hedge_instrument_id", "")).strip()
        if explicit:
            return self._coerce_instrument_id(self.config.reference_instrument_id, explicit)

        route = (hedge_route or "").strip().upper()
        reference_instrument_id = self.config.reference_instrument_id
        if not route or route == "SMART":
            return reference_instrument_id

        reference_text = str(reference_instrument_id).strip()
        base, sep, _exchange = reference_text.rpartition(".")
        if not sep or not base:
            return reference_instrument_id
        return self._coerce_instrument_id(reference_instrument_id, f"{base}.{route}")

    @staticmethod
    def _instrument_id_matches(left: Any, right: Any) -> bool:
        return left == right or str(left).strip() == str(right).strip()

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

    def _quote_leg_snapshot(
        self,
        instrument_id: Any,
        *,
        leg_role: str,
        now_ns: int,
        force_venue: str | None = None,
    ) -> dict[str, Any]:
        instrument = self._resolve_instrument(instrument_id)
        venue = (
            str(force_venue).strip().upper()
            if force_venue is not None
            else str(getattr(instrument_id, "venue", "")).strip().upper()
        )
        symbol = str(getattr(instrument, "raw_symbol", "")).strip() or str(instrument_id)
        payload: dict[str, Any] = {
            "venue": venue,
            "symbol": symbol,
            "instrument_id": str(instrument_id),
        }
        snapshot = self._latest_quotes.get(instrument_id)
        if snapshot is None:
            quote_health = evaluate_quote_health(
                leg_role=leg_role,
                bid=None,
                ask=None,
                quote_age_ms=None,
                max_quote_age_ms=(
                    int(getattr(self.config, "max_ibkr_quote_age_ms", 1_000))
                    if leg_role in {"reference", "hedge"}
                    else int(self._runtime_params.get("max_age_ms", 10_000))
                ),
                transport_connected=None,
                subscription_healthy=None,
            )
            payload["feed_state"] = quote_health.feed_state
            payload["quote_state"] = quote_health.quote_state
            payload["pricing_usable"] = quote_health.usable_for_pricing
            payload["hedge_usable"] = quote_health.usable_for_hedging
            if quote_health.reason_code is not None:
                payload["reason_code"] = quote_health.reason_code
            return payload

        bid = self._decimal_or_none(snapshot.get("bid"))
        ask = self._decimal_or_none(snapshot.get("ask"))
        ts_ns = self._quote_ts_ns(snapshot.get("ts_ns"))
        age_ms = max(0, (int(now_ns) - ts_ns) // 1_000_000) if ts_ns > 0 else None
        if bid is not None:
            payload["bid"] = float(bid)
        if ask is not None:
            payload["ask"] = float(ask)
        if bid is not None and ask is not None:
            payload["mid"] = float((bid + ask) / Decimal("2"))
        if ts_ns > 0:
            payload["ts_ms"] = ts_ns // 1_000_000
        if age_ms is not None:
            payload["age_ms"] = age_ms
        quote_health = evaluate_quote_health(
            leg_role=leg_role,
            bid=bid,
            ask=ask,
            quote_age_ms=age_ms,
            max_quote_age_ms=(
                int(getattr(self.config, "max_ibkr_quote_age_ms", 1_000))
                if leg_role in {"reference", "hedge"}
                else int(self._runtime_params.get("max_age_ms", 10_000))
            ),
            transport_connected=True,
            subscription_healthy=True,
        )
        payload["feed_state"] = quote_health.feed_state
        payload["quote_state"] = quote_health.quote_state
        payload["pricing_usable"] = quote_health.usable_for_pricing
        payload["hedge_usable"] = quote_health.usable_for_hedging
        if quote_health.reason_code is not None:
            payload["reason_code"] = quote_health.reason_code
        return payload

    def _quote_snapshot_payload(self, *, now_ns: int) -> dict[str, Any]:
        maker_leg = self._quote_leg_snapshot(
            self.config.maker_instrument_id,
            leg_role="maker",
            now_ns=now_ns,
        )
        ref_leg = self._quote_leg_snapshot(
            self.config.reference_instrument_id,
            leg_role="reference",
            now_ns=now_ns,
            force_venue="IBKR",
        )
        fee_assumptions = self._fee_assumptions_payload()
        hedge_route = self._hedge_route()
        hedge_instrument_id = self._hedge_instrument_id(hedge_route)
        if self._instrument_id_matches(hedge_instrument_id, self.config.reference_instrument_id):
            hedge_leg = dict(ref_leg)
        else:
            hedge_leg = self._quote_leg_snapshot(
                hedge_instrument_id,
                leg_role="hedge",
                now_ns=now_ns,
                force_venue="IBKR",
            )
            for field_name in ("symbol", "bid", "ask", "mid", "ts_ms", "age_ms"):
                if field_name not in hedge_leg and field_name in ref_leg:
                    hedge_leg[field_name] = ref_leg[field_name]
        if hedge_route:
            hedge_leg["route"] = hedge_route
        spread_hedge_leg = hedge_leg if hedge_leg else ref_leg

        def _leg_decimal(leg: Mapping[str, Any] | None, key: str) -> Decimal | None:
            if not isinstance(leg, Mapping):
                return None
            return self._decimal_or_none(leg.get(key))

        def _leg_mid(leg: Mapping[str, Any] | None) -> Decimal | None:
            if not isinstance(leg, Mapping):
                return None
            explicit_mid = self._decimal_or_none(leg.get("mid"))
            if explicit_mid is not None:
                return explicit_mid
            bid = self._decimal_or_none(leg.get("bid"))
            ask = self._decimal_or_none(leg.get("ask"))
            if bid is None or ask is None:
                return None
            return (bid + ask) / Decimal("2")

        def _spread_bps(*, sell_bid: Decimal | None, buy_ask: Decimal | None, ref_mid: Decimal | None) -> float | None:
            if sell_bid is None or buy_ask is None or ref_mid is None or ref_mid <= 0:
                return None
            return float(((sell_bid - buy_ask) / ref_mid) * Decimal("10000"))

        reference_bid = self._decimal_or_none(ref_leg.get("bid"))
        reference_ask = self._decimal_or_none(ref_leg.get("ask"))
        spread_reference_mid = _leg_mid(spread_hedge_leg)
        maker_mid = _leg_mid(maker_leg)
        maker_bid = _leg_decimal(maker_leg, "bid")
        maker_ask = _leg_decimal(maker_leg, "ask")
        hedge_bid = _leg_decimal(spread_hedge_leg, "bid")
        hedge_ask = _leg_decimal(spread_hedge_leg, "ask")
        mid_spread_bps: float | None = None
        if maker_mid is not None and spread_reference_mid is not None and spread_reference_mid > 0:
            mid_spread_bps = float(((maker_mid - spread_reference_mid) / spread_reference_mid) * Decimal("10000"))
        arb_bid_spread_bps = _spread_bps(
            sell_bid=hedge_bid,
            buy_ask=maker_ask,
            ref_mid=spread_reference_mid,
        )
        arb_ask_spread_bps = _spread_bps(
            sell_bid=maker_bid,
            buy_ask=hedge_ask,
            ref_mid=spread_reference_mid,
        )
        quoted_spread_bps: float | None = None
        if reference_bid is not None and reference_ask is not None:
            reference_mid = (reference_bid + reference_ask) / Decimal("2")
            if reference_mid > 0:
                quoted_spread_bps = float(
                    ((reference_ask - reference_mid) / reference_mid) * Decimal("10000")
                )
        expected_maker_fee_bps = self._last_pricing_debug.get("expected_maker_fee_bps")
        assumed_hedge_fee_bps = self._last_pricing_debug.get(
            "assumed_hedge_fee_bps",
            float(self._runtime_params.get("assumed_hedge_fee_bps", 1.0)),
        )
        hedge_slippage_bps = self._last_pricing_debug.get("hedge_slippage_bps_vs_mid")
        effective_spread_bps: float | None = None
        if quoted_spread_bps is not None and expected_maker_fee_bps is not None:
            effective_spread_bps = (
                float(quoted_spread_bps)
                - float(expected_maker_fee_bps)
                - float(assumed_hedge_fee_bps)
                - float(hedge_slippage_bps or 0.0)
            )
        payload = publisher_mod.build_quote_snapshot_payload(
            maker_leg=maker_leg,
            hedge_leg=hedge_leg,
            ref_leg=ref_leg,
            mid_spread_bps=mid_spread_bps,
            arb_bid_spread_bps=arb_bid_spread_bps,
            arb_ask_spread_bps=arb_ask_spread_bps,
            effective_spread_bps=effective_spread_bps,
            quoted_spread_bps=quoted_spread_bps,
            expected_maker_fee_bps=(
                None if expected_maker_fee_bps is None else float(expected_maker_fee_bps)
            ),
            hedge_ready=bool(
                self.tradeable
                and maker_leg.get("bid") is not None
                and maker_leg.get("ask") is not None
                and ref_leg.get("bid") is not None
                and ref_leg.get("ask") is not None
            ),
            hedge_route=hedge_route,
            effective_account_source=self._last_pricing_debug.get("effective_account_source"),
            hedge_disabled_reason=self.hedge_disabled_reason,
            ibkr_quote_age_ms=(
                int(ref_leg["age_ms"])
                if ref_leg.get("age_ms") is not None
                else None
            ),
            assumed_hedge_fee_bps=float(assumed_hedge_fee_bps),
            fee_snapshot_age_s=self._last_pricing_debug.get("fee_snapshot_age_s"),
            hedge_latency_ms=self._last_pricing_debug.get("hedge_latency_ms"),
            hedge_slippage_bps_vs_mid=self._last_pricing_debug.get("hedge_slippage_bps_vs_mid"),
            ts_ms=max(0, int(now_ns)) // 1_000_000,
            fee_assumptions=fee_assumptions,
        )
        payload["max_ibkr_quote_age_ms"] = int(
            getattr(self.config, "max_ibkr_quote_age_ms", 1_000),
        )
        return payload

    def _maker_quote_health(self, *, now_ns: int):
        maker_leg = self._quote_leg_snapshot(
            self.config.maker_instrument_id,
            leg_role="maker",
            now_ns=now_ns,
        )
        return evaluate_quote_health(
            leg_role="maker",
            bid=self._decimal_or_none(maker_leg.get("bid")),
            ask=self._decimal_or_none(maker_leg.get("ask")),
            quote_age_ms=(
                None if maker_leg.get("age_ms") is None else int(maker_leg.get("age_ms"))
            ),
            max_quote_age_ms=int(self._runtime_params.get("max_age_ms", 10_000)),
            transport_connected=(
                True if self._latest_quotes.get(self.config.maker_instrument_id) is not None else None
            ),
            subscription_healthy=(
                True if self._latest_quotes.get(self.config.maker_instrument_id) is not None else None
            ),
        )

    def _maker_quote_targets(self, *, now_ns: int) -> dict[str, Decimal] | None:
        if not self._can_quote():
            return None
        if int(self._runtime_params.get("n_orders1", 0)) <= 0:
            return None
        maker_health = self._maker_quote_health(now_ns=now_ns)
        if not maker_health.usable_for_pricing:
            return None
        reference_quote = self._reference_quote_snapshot(now_ns=now_ns)
        if reference_quote is None:
            return None
        quote_error = validate_ibkr_quote(
            bid=reference_quote.bid,
            ask=reference_quote.ask,
            quote_age_ms=reference_quote.age_ms,
            max_quote_age_ms=int(getattr(self.config, "max_ibkr_quote_age_ms", 1_000)),
            max_spread_bps=Decimal(str(getattr(self.config, "max_ibkr_spread_bps", "25"))),
        )
        if quote_error is not None or reference_quote.mid is None:
            return None
        maker_health = self._maker_quote_health(now_ns=now_ns)
        if not maker_health.usable_for_pricing:
            return None

        maker_snapshot = self._latest_quotes.get(self.config.maker_instrument_id)
        if not isinstance(maker_snapshot, dict):
            return None
        maker_bid = self._decimal_or_none(maker_snapshot.get("bid"))
        maker_ask = self._decimal_or_none(maker_snapshot.get("ask"))
        if maker_bid is None or maker_ask is None or maker_ask <= maker_bid:
            return None

        fee_assumptions = self._fee_assumptions()
        maker_fee_bps = fee_assumptions.maker_maker_fee_bps
        hedge_fee_bps = build_effective_ibkr_fee_bps(
            fee_assumptions=fee_assumptions,
            hedge_notional_usd=reference_quote.mid * self._maker_order_base_qty(),
        )
        tick_size = self._maker_tick_size()
        return {
            "BUY": build_maker_quote_price(
                side="BUY",
                reference_mid=reference_quote.mid,
                target_edge_bps=Decimal(str(self._runtime_params.get("bid_edge1", "0"))),
                maker_fee_bps=maker_fee_bps,
                hedge_fee_bps=hedge_fee_bps,
                offset_bps=Decimal("0"),
                tick_size=tick_size,
            ),
            "SELL": build_maker_quote_price(
                side="SELL",
                reference_mid=reference_quote.mid,
                target_edge_bps=Decimal(str(self._runtime_params.get("ask_edge1", "0"))),
                maker_fee_bps=maker_fee_bps,
                hedge_fee_bps=hedge_fee_bps,
                offset_bps=Decimal("0"),
                tick_size=tick_size,
            ),
        }

    def _maker_reprice_threshold(self, *, reference_mid: Decimal) -> Decimal:
        threshold_bps = Decimal(str(self._runtime_params.get("place_edge1", "0")))
        return abs(reference_mid * threshold_bps / Decimal("10000"))

    def _maker_quotes_match_targets(
        self,
        *,
        targets: dict[str, Decimal],
        reference_mid: Decimal,
    ) -> bool:
        if set(self._managed_maker_orders) != set(targets):
            return False
        threshold = self._maker_reprice_threshold(reference_mid=reference_mid)
        for side, target_price in targets.items():
            existing = self._managed_maker_orders.get(side)
            if existing is None:
                return False
            if existing.pending_cancel:
                return False
            if abs(existing.price - target_price) > threshold:
                return False
        return True

    def _clear_managed_maker_orders(self) -> None:
        self._managed_maker_orders.clear()

    def _mark_managed_maker_orders_pending_cancel(self) -> None:
        if not self._managed_maker_orders:
            return
        if all(state.pending_cancel for state in self._managed_maker_orders.values()):
            return
        self._managed_maker_orders = {
            side: ManagedMakerOrderState(
                client_order_id=state.client_order_id,
                instrument_id=state.instrument_id,
                side=state.side,
                quantity=state.quantity,
                price=state.price,
                post_only=state.post_only,
                pending_cancel=True,
            )
            for side, state in self._managed_maker_orders.items()
        }

    def _cancel_managed_maker_orders(self) -> None:
        if not self._managed_maker_orders:
            return
        if all(state.pending_cancel for state in self._managed_maker_orders.values()):
            return
        cancel_all_orders = getattr(self, "cancel_all_orders", None)
        if not callable(cancel_all_orders):
            return
        try:
            cancel_all_orders(self.config.maker_instrument_id)
        except Exception:
            return
        self._mark_managed_maker_orders_pending_cancel()

    def _managed_maker_side_for_client_order_id(self, client_order_id: Any) -> str | None:
        order_id = str(client_order_id or "").strip()
        if not order_id:
            return None
        for side, state in self._managed_maker_orders.items():
            if state.client_order_id == order_id:
                return side
        return None

    def _managed_maker_state_for_client_order_id(
        self,
        client_order_id: Any,
    ) -> ManagedMakerOrderState | None:
        side = self._managed_maker_side_for_client_order_id(client_order_id)
        if side is None:
            return None
        return self._managed_maker_orders.get(side)

    def _prune_recent_take_take_order_ids(self, *, now_ns: int) -> None:
        cutoff_ns = max(0, int(now_ns)) - self.TAKE_TAKE_ORDER_TTL_NS
        expired = [
            client_order_id
            for client_order_id, seen_ns in self._recent_take_take_order_ids.items()
            if seen_ns < cutoff_ns
        ]
        for client_order_id in expired:
            self._recent_take_take_order_ids.pop(client_order_id, None)
        while len(self._recent_take_take_order_ids) > self.TAKE_TAKE_ORDER_TRACK_LIMIT:
            oldest = next(iter(self._recent_take_take_order_ids))
            self._recent_take_take_order_ids.pop(oldest, None)

    def _remember_recent_take_take_order_id(self, client_order_id: Any, *, now_ns: int) -> None:
        order_id = str(client_order_id or "").strip()
        if not order_id:
            return
        self._prune_recent_take_take_order_ids(now_ns=now_ns)
        self._recent_take_take_order_ids.pop(order_id, None)
        self._recent_take_take_order_ids[order_id] = max(0, int(now_ns))

    def _is_recent_take_take_order_id(self, client_order_id: Any, *, now_ns: int) -> bool:
        order_id = str(client_order_id or "").strip()
        if not order_id:
            return False
        self._prune_recent_take_take_order_ids(now_ns=now_ns)
        return order_id in self._recent_take_take_order_ids

    def _cache_order_is_closed(self, client_order_id: Any) -> bool:
        order_id = str(client_order_id or "").strip()
        if not order_id:
            return False
        cache = getattr(self, "_cache", None)
        if cache is None:
            with suppress(Exception):
                cache = self.cache
        if cache is None:
            return False

        fetch_order = getattr(cache, "order", None)
        if callable(fetch_order):
            with suppress(Exception):
                order = fetch_order(order_id)
                if order is not None:
                    is_closed = getattr(order, "is_closed", None)
                    if callable(is_closed):
                        with suppress(Exception):
                            return bool(is_closed())
                    if is_closed is not None:
                        return bool(is_closed)
                    status = self._enum_name(getattr(order, "status", ""))
                    if status in {"CANCELED", "EXPIRED", "FILLED", "REJECTED"}:
                        return True
                    if status:
                        return False

        strategy_id = getattr(self, "id", None) or getattr(self.config, "strategy_id", None)
        seen_open_order = False
        for fetch_name in ("orders_open", "orders_inflight"):
            fetch = getattr(cache, fetch_name, None)
            if not callable(fetch):
                continue
            try:
                rows = fetch(
                    instrument_id=self.config.maker_instrument_id,
                    strategy_id=strategy_id,
                )
            except TypeError:
                with suppress(Exception):
                    rows = fetch()
            except Exception:
                rows = []
            if rows is None:
                rows = []
            for order in rows:
                candidate = str(getattr(order, "client_order_id", "")).strip()
                if not candidate:
                    continue
                seen_open_order = True
                if candidate == order_id:
                    return False
        return seen_open_order

    def _reconcile_closed_take_take_orders_from_cache(self, *, now_ns: int) -> None:
        if not self._managed_maker_orders:
            return
        for state in list(self._managed_maker_orders.values()):
            if state.post_only:
                continue
            if not self._cache_order_is_closed(state.client_order_id):
                continue
            self._reconcile_managed_maker_order(
                SimpleNamespace(
                    client_order_id=state.client_order_id,
                    instrument_id=self.config.maker_instrument_id,
                )
            )
            self._finalize_take_take_hedge(state.client_order_id, now_ns=now_ns)

    def _reconcile_managed_maker_order(self, event: Any) -> bool:
        side = self._managed_maker_side_for_client_order_id(getattr(event, "client_order_id", None))
        if side is None:
            return False
        instrument_id = getattr(event, "instrument_id", None)
        if instrument_id is not None and not self._instrument_id_matches(
            instrument_id,
            self.config.maker_instrument_id,
        ):
            return False
        self._managed_maker_orders.pop(side, None)
        return True

    def _apply_maker_fill_to_managed_order(self, event: Any) -> bool:
        side = self._managed_maker_side_for_client_order_id(getattr(event, "client_order_id", None))
        if side is None:
            return False
        state = self._managed_maker_orders.get(side)
        if state is None:
            return False
        instrument_id = getattr(event, "instrument_id", None)
        if instrument_id is not None and not self._instrument_id_matches(
            instrument_id,
            self.config.maker_instrument_id,
        ):
            return False
        fill_qty = self._decimal_or_none(getattr(event, "last_qty", None))
        if fill_qty is None:
            return False
        remaining_qty = state.quantity - fill_qty
        if remaining_qty <= 0:
            self._managed_maker_orders.pop(side, None)
            return True
        self._managed_maker_orders[side] = ManagedMakerOrderState(
            client_order_id=state.client_order_id,
            instrument_id=state.instrument_id,
            side=state.side,
            quantity=remaining_qty,
            price=state.price,
            post_only=state.post_only,
            pending_cancel=state.pending_cancel,
        )
        return True

    def _reclaim_managed_maker_orders_from_cache(self) -> None:
        cache = getattr(self, "_cache", None)
        if cache is None:
            with suppress(Exception):
                cache = self.cache
        if cache is None:
            return
        strategy_id = getattr(self, "id", None) or getattr(self.config, "strategy_id", None)
        if strategy_id is None:
            return
        with suppress(Exception):
            open_orders = makerv3_managed_orders_mod.collect_managed_orders(
                cache=cache,
                instrument_id=self.config.maker_instrument_id,
                strategy_id=strategy_id,
            )
            if not open_orders:
                return
            reclaimed: dict[str, ManagedMakerOrderState] = {}
            latest_ts_init_by_side: dict[str, int] = {}
            for order in open_orders:
                side = self._enum_name(getattr(order, "side", ""))
                if side not in {"BUY", "SELL"}:
                    continue
                order_ts_init = int(getattr(order, "ts_init", 0) or 0)
                if side in latest_ts_init_by_side and order_ts_init < latest_ts_init_by_side[side]:
                    continue
                quantity = self._required_decimal(getattr(order, "quantity", None), field_name="quantity")
                exposure = exposure_from_venue_qty(
                    self._resolve_instrument(self.config.maker_instrument_id) or order,
                    quantity,
                    last_px=self._maker_order_last_px(),
                )
                tracked_quantity = exposure.base_qty if exposure.base_qty is not None else quantity
                price = self._required_decimal(getattr(order, "price", None), field_name="price")
                reclaimed[side] = ManagedMakerOrderState(
                    client_order_id=str(getattr(order, "client_order_id", "")).strip(),
                    instrument_id=str(self.config.maker_instrument_id),
                    side=side,
                    quantity=tracked_quantity,
                    price=price,
                    post_only=bool(getattr(order, "post_only", True)),
                    pending_cancel=False,
                )
                if not reclaimed[side].post_only:
                    self._remember_recent_take_take_order_id(
                        reclaimed[side].client_order_id,
                        now_ns=order_ts_init,
                    )
                latest_ts_init_by_side[side] = order_ts_init
            if reclaimed:
                self._managed_maker_orders = reclaimed

    def _submit_maker_quote(self, *, side: str, target_price: Decimal) -> None:
        maker_instrument = self._resolve_instrument(self.config.maker_instrument_id)
        if maker_instrument is None:
            return
        venue_qty, base_qty = self._maker_order_quantities()
        if venue_qty <= 0 or base_qty <= 0:
            self._disable_hedging("maker_qty_conversion_failed")
            return
        order = self.order_factory.limit(
            instrument_id=self.config.maker_instrument_id,
            order_side=self._order_side_enum(side),
            quantity=self._make_order_quantity(maker_instrument, venue_qty),
            price=self._make_order_price(maker_instrument, target_price),
            post_only=True,
        )
        self.submit_order(order)
        self._managed_maker_orders[side] = ManagedMakerOrderState(
            client_order_id=str(getattr(order, "client_order_id", "")).strip(),
            instrument_id=str(self.config.maker_instrument_id),
            side=side,
            quantity=base_qty,
            price=self._required_decimal(getattr(order, "price", target_price), field_name="price"),
            post_only=True,
        )

    def _submit_take_take_order(self, *, side: str, target_price: Decimal, now_ns: int) -> None:
        maker_instrument = self._resolve_instrument(self.config.maker_instrument_id)
        if maker_instrument is None:
            return
        venue_qty, base_qty = self._maker_order_quantities()
        if venue_qty <= 0 or base_qty <= 0:
            self._disable_hedging("maker_qty_conversion_failed")
            return
        order = self.order_factory.limit(
            instrument_id=self.config.maker_instrument_id,
            order_side=self._order_side_enum(side),
            quantity=self._make_order_quantity(maker_instrument, venue_qty),
            price=self._make_order_price(maker_instrument, target_price),
            post_only=False,
            time_in_force=TimeInForce.IOC,
        )
        self.submit_order(order)
        self._managed_maker_orders[side] = ManagedMakerOrderState(
            client_order_id=str(getattr(order, "client_order_id", "")).strip(),
            instrument_id=str(self.config.maker_instrument_id),
            side=side,
            quantity=base_qty,
            price=self._required_decimal(getattr(order, "price", target_price), field_name="price"),
            post_only=False,
        )
        self._remember_recent_take_take_order_id(
            self._managed_maker_orders[side].client_order_id,
            now_ns=now_ns,
        )
        self._last_take_submission_ns = max(0, int(now_ns))

    def _take_take_signal(self, *, now_ns: int) -> tuple[str, Decimal] | None:
        if not self._can_quote():
            return None
        cooldown_ns = max(0, int(self._runtime_params.get("take_cooldown_ms", 0))) * 1_000_000
        if (
            cooldown_ns > 0
            and self._last_take_submission_ns > 0
            and (max(0, int(now_ns)) - self._last_take_submission_ns) < cooldown_ns
        ):
            return None

        reference_quote = self._reference_quote_snapshot(now_ns=now_ns)
        if reference_quote is None:
            return None
        quote_error = validate_ibkr_quote(
            bid=reference_quote.bid,
            ask=reference_quote.ask,
            quote_age_ms=reference_quote.age_ms,
            max_quote_age_ms=int(getattr(self.config, "max_ibkr_quote_age_ms", 1_000)),
            max_spread_bps=Decimal(str(getattr(self.config, "max_ibkr_spread_bps", "25"))),
        )
        if quote_error is not None or reference_quote.mid is None:
            return None
        maker_health = self._maker_quote_health(now_ns=now_ns)
        if not maker_health.usable_for_pricing:
            return None

        maker_snapshot = self._latest_quotes.get(self.config.maker_instrument_id)
        if not isinstance(maker_snapshot, dict):
            return None
        maker_bid = self._decimal_or_none(maker_snapshot.get("bid"))
        maker_ask = self._decimal_or_none(maker_snapshot.get("ask"))
        if maker_bid is None or maker_ask is None or maker_ask <= maker_bid:
            return None

        fee_assumptions = self._fee_assumptions()
        hedge_fee_bps = build_effective_ibkr_fee_bps(
            fee_assumptions=fee_assumptions,
            hedge_notional_usd=reference_quote.mid * self._maker_order_base_qty(),
        )
        buy_price = build_take_take_limit_price(
            side="BUY",
            maker_bid=maker_bid,
            maker_ask=maker_ask,
            reference_bid=reference_quote.bid,
            reference_ask=reference_quote.ask,
            target_edge_bps=Decimal(str(self._runtime_params.get("bid_edge_take_bps", "0"))),
            maker_taker_fee_bps=fee_assumptions.maker_taker_fee_bps,
            hedge_fee_bps=hedge_fee_bps,
        )
        if buy_price is not None:
            return ("BUY", buy_price)

        sell_price = build_take_take_limit_price(
            side="SELL",
            maker_bid=maker_bid,
            maker_ask=maker_ask,
            reference_bid=reference_quote.bid,
            reference_ask=reference_quote.ask,
            target_edge_bps=Decimal(str(self._runtime_params.get("ask_edge_take_bps", "0"))),
            maker_taker_fee_bps=fee_assumptions.maker_taker_fee_bps,
            hedge_fee_bps=hedge_fee_bps,
        )
        if sell_price is not None:
            return ("SELL", sell_price)
        return None

    def _refresh_take_take_orders(self, *, now_ns: int) -> None:
        if self._managed_maker_orders:
            if any(state.post_only for state in self._managed_maker_orders.values()):
                self._cancel_managed_maker_orders()
            return
        signal = self._take_take_signal(now_ns=now_ns)
        if signal is None:
            return
        if self._maker_order_hedge_qty() <= 0:
            self._disable_hedging("hedge_qty_rounds_to_zero")
            return
        side, target_price = signal
        self._submit_take_take_order(side=side, target_price=target_price, now_ns=now_ns)

    def _refresh_maker_quotes(self, *, now_ns: int) -> None:
        if self._execution_mode() == "take_take":
            self._refresh_take_take_orders(now_ns=now_ns)
            return
        targets = self._maker_quote_targets(now_ns=now_ns)
        if targets is None:
            self._cancel_managed_maker_orders()
            return
        reference_quote = self._reference_quote_snapshot(now_ns=now_ns)
        if reference_quote is None or reference_quote.mid is None:
            self._cancel_managed_maker_orders()
            return
        if self._maker_quotes_match_targets(targets=targets, reference_mid=reference_quote.mid):
            return
        if self._managed_maker_orders:
            self._cancel_managed_maker_orders()
            return
        for side in ("BUY", "SELL"):
            self._submit_maker_quote(side=side, target_price=targets[side])

    def _publish_state_snapshot(
        self,
        *,
        now_ns: int | None = None,
        state_override: str | None = None,
    ) -> None:
        publish_ns = int(self.clock.timestamp_ns()) if now_ns is None else int(now_ns)
        state = str(state_override).strip() if state_override is not None else ""
        if not state:
            state = "running" if self._effective_bot_on() else "bot_off"
        if state_override is None and self.hedge_disabled_reason:
            state = f"blocked_{self.hedge_disabled_reason}"
        managed_orders = self._managed_orders()
        now_ms_value = publish_ns // 1_000_000
        inventory_snapshot = self._inventory_contract_snapshot(now_ms_value=now_ms_value)
        self._publish_portfolio_inventory_component(
            state=state,
            now_ms_value=now_ms_value,
            inventory_snapshot=inventory_snapshot,
        )
        inventory_fields, skew_fields = self._inventory_state_fields(
            now_ms_value=now_ms_value,
            inventory_snapshot=inventory_snapshot,
        )
        payload = {
            "strategy_id": self._external_strategy_id,
            "state": state,
            "bot_on": self._effective_bot_on(),
            "managed_orders": len(managed_orders),
            "tracked_managed_orders": self._tracked_managed_order_count(),
            "ts_event": publish_ns,
            "ts_ms": publish_ns // 1_000_000,
            "maker_quote_status": self._maker_quote_status_payload(),
            "maker_role_map": build_role_map_payload(
                maker_leg=str(self.config.maker_instrument_id),
                ref_leg=str(self.config.reference_instrument_id),
                hedge_leg=str(self._hedge_instrument_id(self._hedge_route())),
            ),
            "maker_v4": {
                "quote_snapshot": self._quote_snapshot_payload(now_ns=publish_ns),
                "hedge_policy": self._hedge_policy_payload(now_ns=publish_ns),
                "fee_assumptions": self._fee_assumptions_payload(),
                "managed_maker_orders": self._managed_maker_orders_payload(),
            },
        }
        pending_hedge_payload = self._pending_hedge_payload()
        if pending_hedge_payload is not None:
            payload["maker_v4"]["pending_hedge"] = pending_hedge_payload
        hedge_backlog_payload = self._hedge_backlog_payload()
        if hedge_backlog_payload is not None:
            payload["maker_v4"]["hedge_backlog"] = hedge_backlog_payload
        payload.update(inventory_fields)
        pricing_debug: dict[str, Any] = {}
        if isinstance(self._last_pricing_debug, dict) and self._last_pricing_debug:
            pricing_debug["pricing"] = dict(self._last_pricing_debug)
        if self._last_venue_protection:
            pricing_debug["venue_protection"] = dict(self._last_venue_protection)
        if skew_fields:
            pricing_debug["skew"] = skew_fields
        if pricing_debug:
            payload["pricing_debug"] = pricing_debug
        self._last_state_name = state
        self._last_state_ns = publish_ns
        self._publish_json(TOPIC_STATE, payload)

    def _disable_hedging(self, reason: str) -> None:
        self.tradeable = False
        self.hedge_disabled_reason = str(reason)
        if self.hedge_disabled_reason != "venue_protection":
            self._last_venue_protection = {}
            with suppress(Exception):
                self._publish_actionable_alert(
                    alert_key="maker_v4_hedge_disabled",
                    message=(
                        "maker_v4_hedge_disabled "
                        f"reason={self.hedge_disabled_reason}"
                    ),
                    level="error",
                    reason_code="maker_v4_hedge_disabled",
                    cooldown_ms=ALERT_COOLDOWN_TERMINAL_ORDER_DENIED_MS,
                    transition=self.hedge_disabled_reason,
                    now_ns=int(self.clock.timestamp_ns()),
                    hedge_disabled_reason=self.hedge_disabled_reason,
                    pending_hedge_order_id=(
                        None if self._pending_hedge is None else self._pending_hedge.order_id
                    ),
                    pending_hedge_route=(
                        None if self._pending_hedge is None else self._pending_hedge.route
                    ),
                )
        self._cancel_managed_maker_orders()

    def _remember_fill_id(self, fill_id: str) -> None:
        self._seen_fill_ids.add(fill_id)
        self._fill_ids_head.append(fill_id)
        if len(self._fill_ids_head) > 64:
            self._fill_ids_head = self._fill_ids_head[-64:]
            self._seen_fill_ids = set(self._fill_ids_head)

    def _reference_quote_snapshot(self, *, now_ns: int | None = None) -> IbkrQuoteSnapshot | None:
        instrument_id = self.config.reference_instrument_id
        snapshot = self._latest_quotes.get(instrument_id)
        if snapshot is None:
            return None
        ts_ns = self._quote_ts_ns(snapshot.get("ts_ns"))
        current_ns = int(self.clock.timestamp_ns()) if now_ns is None else int(now_ns)
        return IbkrQuoteSnapshot(
            instrument_id=str(instrument_id),
            bid=self._decimal_or_none(snapshot.get("bid")),
            ask=self._decimal_or_none(snapshot.get("ask")),
            age_ms=max(0, (current_ns - ts_ns) // 1_000_000),
            ts_ms=ts_ns // 1_000_000,
        )

    def _update_pending_hedge_order_id(self, order_id: str | None) -> None:
        if self._pending_hedge is None or not order_id:
            return
        self._pending_hedge = PendingHedgeState(
            fill_id=self._pending_hedge.fill_id,
            side=self._pending_hedge.side,
            requested_qty=self._pending_hedge.requested_qty,
            remaining_qty=self._pending_hedge.remaining_qty,
            limit_price=self._pending_hedge.limit_price,
            route=self._pending_hedge.route,
            time_in_force=self._pending_hedge.time_in_force,
            outside_rth=self._pending_hedge.outside_rth,
            include_overnight=self._pending_hedge.include_overnight,
            cancel_after_ms=self._pending_hedge.cancel_after_ms,
            order_id=str(order_id),
        )

    def _submit_hedge_intent(self, intent: HedgeOrderIntent) -> str | None:
        hedge_instrument_id = self._coerce_instrument_id(
            self.config.reference_instrument_id,
            intent.instrument_id,
        )
        hedge_instrument = self._resolve_instrument(hedge_instrument_id)
        if hedge_instrument is None:
            hedge_instrument = self._resolve_instrument(self.config.reference_instrument_id)
            hedge_instrument_id = self.config.reference_instrument_id
        if hedge_instrument is None:
            self._disable_hedging("missing_hedge_instrument")
            return None
        try:
            order = self.order_factory.limit(
                instrument_id=hedge_instrument_id,
                order_side=self._order_side_enum(intent.side),
                quantity=self._make_order_quantity(hedge_instrument, intent.qty),
                price=self._make_order_price(hedge_instrument, intent.limit_price),
                time_in_force=self._time_in_force_enum(intent.time_in_force),
                tags=build_ibkr_order_tags(
                    outside_rth=bool(intent.outside_rth),
                    include_overnight=bool(intent.include_overnight),
                ),
            )
            self.submit_order(order, client_id=IB_CLIENT_ID)
        except Exception as exc:
            self.log.error(f"Failed to submit MakerV4 hedge order: {exc}")
            self._disable_hedging("hedge_submit_failed")
            return None
        return str(getattr(order, "client_order_id", "")).strip() or None

    def _handle_maker_fill_event(self, event: Any, *, now_ns: int) -> None:
        if not bool(self._runtime_params.get("instant_hedge_enabled", True)):
            self._disable_hedging("instant_hedge_disabled")
            return
        hedge_style = str(self._runtime_params.get("hedge_style", "ioc_through_mid")).strip()
        if hedge_style != "ioc_through_mid":
            self._disable_hedging("unsupported_hedge_style")
            return
        fill = MakerFill(
            fill_id=str(getattr(event, "trade_id", "")).strip(),
            side=str(getattr(event, "order_side", "")).strip(),
            qty=self._required_decimal(getattr(event, "last_qty", None), field_name="last_qty"),
            price=self._required_decimal(getattr(event, "last_px", None), field_name="last_px"),
            ts_ms=max(0, now_ns // 1_000_000),
        )
        fee_assumptions = self._fee_assumptions()
        quote = self._reference_quote_snapshot(now_ns=now_ns)
        if quote is None:
            self._queue_hedge_backlog_for_fill(
                fill=fill,
                maker_fee_bps=fee_assumptions.maker_maker_fee_bps,
                blocked_reason="missing_ref_quote",
            )
            return
        order = self.record_maker_fill(
            fill=fill,
            quote=quote,
            maker_fee_bps=fee_assumptions.maker_maker_fee_bps,
        )
        if order is None:
            return
        self._update_pending_hedge_order_id(self._submit_hedge_intent(order))
        self._cancel_managed_maker_orders()

    def _accumulate_take_take_fill(self, event: Any, *, now_ns: int) -> str | None:
        client_order_id = str(getattr(event, "client_order_id", "")).strip()
        if not client_order_id:
            return None
        fill_id = str(getattr(event, "trade_id", "")).strip()
        if not fill_id:
            fill_id = (
                f"take_take_event:{client_order_id}:"
                f"{self._required_decimal(getattr(event, 'last_qty', None), field_name='last_qty')}:"
                f"{max(0, int(now_ns))}"
            )
        if fill_id in self._seen_fill_ids:
            return None
        self._remember_fill_id(fill_id)

        accumulator = self._take_take_fill_accumulators.get(client_order_id, {})
        accumulated_qty = Decimal(str(accumulator.get("qty", "0")))
        self._take_take_fill_accumulators[client_order_id] = {
            "side": str(getattr(event, "order_side", "")).strip(),
            "qty": accumulated_qty
            + self._required_decimal(getattr(event, "last_qty", None), field_name="last_qty"),
            "price": self._required_decimal(getattr(event, "last_px", None), field_name="last_px"),
            "ts_ms": max(0, int(now_ns) // 1_000_000),
        }
        return client_order_id

    @staticmethod
    def _signed_fill_qty(*, side: str, qty: Decimal) -> Decimal:
        return abs(qty) if str(side).strip().upper() == "BUY" else -abs(qty)

    @staticmethod
    def _is_retryable_hedge_block_reason(reason: str | None) -> bool:
        return str(reason or "").strip() in {
            "missing_ref_quote",
            "missing_bid",
            "missing_ask",
            "locked_or_crossed",
            "stale_quote",
            "missing_midpoint",
            "spread_too_wide",
            "invalid_hedge_limit",
        }

    def _queue_hedge_backlog(
        self,
        *,
        fill_id: str,
        side: str,
        requested_qty: Decimal,
        blocked_reason: str,
        fill_ts_ms: int,
        maker_fee_bps: Decimal,
    ) -> None:
        normalized_reason = str(blocked_reason).strip() or "hedge_retry_blocked"
        normalized_qty = abs(Decimal(str(requested_qty)))
        if normalized_qty <= 0:
            return
        signed_qty = self._signed_fill_qty(side=side, qty=normalized_qty)
        if self._hedge_backlog is not None:
            signed_qty += self._signed_fill_qty(
                side=self._hedge_backlog.side,
                qty=self._hedge_backlog.requested_qty,
            )
        if signed_qty == 0:
            self._hedge_backlog = None
            if self._pending_hedge is None:
                self.tradeable = True
                self.hedge_disabled_reason = None
            return
        backlog_side = "BUY" if signed_qty > 0 else "SELL"
        self._hedge_backlog = HedgeBacklogState(
            fill_id=str(fill_id).strip() or "hedge-backlog",
            side=backlog_side,
            requested_qty=abs(signed_qty),
            blocked_reason=normalized_reason,
            fill_ts_ms=max(0, int(fill_ts_ms)),
            maker_fee_bps=abs(Decimal(str(maker_fee_bps))),
        )
        self.tradeable = False
        self.hedge_disabled_reason = normalized_reason

    def _queue_hedge_backlog_for_fill(
        self,
        *,
        fill: MakerFill,
        maker_fee_bps: Decimal,
        blocked_reason: str,
    ) -> bool:
        fill_id = str(fill.fill_id).strip()
        if fill_id and fill_id not in self._seen_fill_ids:
            self._remember_fill_id(fill_id)
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
            return False
        self._queue_hedge_backlog(
            fill_id=fill_id,
            side=hedge_side,
            requested_qty=hedge_qty,
            blocked_reason=blocked_reason,
            fill_ts_ms=int(fill.ts_ms),
            maker_fee_bps=maker_fee_bps,
        )
        return True

    def _retry_hedge_backlog(self, *, now_ns: int) -> None:
        if self._hedge_backlog is None or self._pending_hedge is not None:
            return
        quote = self._reference_quote_snapshot(now_ns=now_ns)
        if quote is None:
            return
        backlog = self._hedge_backlog
        pending = self._build_pending_hedge_state(
            fill_id=backlog.fill_id,
            hedge_side=backlog.side,
            hedge_qty=backlog.requested_qty,
            quote=quote,
            maker_fee_bps=backlog.maker_fee_bps,
            fill_ts_ms=backlog.fill_ts_ms,
        )
        if isinstance(pending, str):
            if self._is_retryable_hedge_block_reason(pending):
                self._queue_hedge_backlog(
                    fill_id=backlog.fill_id,
                    side=backlog.side,
                    requested_qty=backlog.requested_qty,
                    blocked_reason=pending,
                    fill_ts_ms=backlog.fill_ts_ms,
                    maker_fee_bps=backlog.maker_fee_bps,
                )
                return
            self._disable_hedging(pending)
            return
        order, pending_state = pending
        order_id = self._submit_hedge_intent(order)
        if order_id is None:
            return
        self._hedge_requests.append(order)
        self._pending_hedge = PendingHedgeState(
            fill_id=pending_state.fill_id,
            side=pending_state.side,
            requested_qty=pending_state.requested_qty,
            remaining_qty=pending_state.remaining_qty,
            limit_price=pending_state.limit_price,
            route=pending_state.route,
            time_in_force=pending_state.time_in_force,
            outside_rth=pending_state.outside_rth,
            include_overnight=pending_state.include_overnight,
            cancel_after_ms=pending_state.cancel_after_ms,
            order_id=order_id,
        )
        self._hedge_backlog = None
        self.tradeable = True
        self.hedge_disabled_reason = None

    def _build_pending_hedge_state(
        self,
        *,
        fill_id: str,
        hedge_side: str,
        hedge_qty: Decimal,
        quote: IbkrQuoteSnapshot,
        maker_fee_bps: Decimal,
        fill_ts_ms: int,
    ) -> tuple[HedgeOrderIntent, PendingHedgeState] | str:
        quote_error = validate_ibkr_quote(
            bid=quote.bid,
            ask=quote.ask,
            quote_age_ms=quote.age_ms,
            max_quote_age_ms=int(getattr(self.config, "max_ibkr_quote_age_ms", 1_000)),
            max_spread_bps=Decimal(str(getattr(self.config, "max_ibkr_spread_bps", "25"))),
        )
        if quote_error is not None:
            return quote_error

        fee_rules = fees_mod.resolve_fee_rules(
            runtime_params=self._runtime_params,
            maker_fee_bps=maker_fee_bps,
            fee_snapshot_age_s=Decimal(str(max(quote.age_ms, 0))) / Decimal("1000"),
        )
        limit_price = build_ibkr_ioc_limit(
            side=hedge_side,
            bid=quote.bid,
            ask=quote.ask,
            cross_mid_bps=Decimal(str(self._runtime_params["hedge_ioc_cross_mid_bps"])),
            max_cross_bps=Decimal(str(self._runtime_params["hedge_ioc_max_cross_bps"])),
            tick_size=Decimal(str(getattr(self.config, "hedge_price_tick_size", Decimal("0.01")))),
            quote_age_ms=quote.age_ms,
            max_quote_age_ms=int(getattr(self.config, "max_ibkr_quote_age_ms", 1_000)),
            max_spread_bps=Decimal(str(getattr(self.config, "max_ibkr_spread_bps", "25"))),
        )
        if limit_price is None:
            return "invalid_hedge_limit"

        hedge_route = self._hedge_route()
        policy = build_ibkr_hedge_order_policy(
            configured_route=hedge_route,
            outside_rth_enabled=bool(getattr(self.config, "outside_rth_hedge_enabled", False)),
            is_regular_session=self._is_regular_hedge_session(ts_ms=int(fill_ts_ms)),
            hedge_mode="maker_hedge",
        )
        effective_hedge_route = str(policy.route).strip().upper() or None
        hedge_instrument_id = self._hedge_instrument_id(effective_hedge_route)
        order = HedgeOrderIntent(
            instrument_id=str(hedge_instrument_id),
            side=hedge_side,
            qty=abs(Decimal(str(hedge_qty))),
            limit_price=limit_price,
            route=policy.route,
            time_in_force=policy.time_in_force,
            outside_rth=policy.outside_rth,
            include_overnight=policy.include_overnight,
            cancel_after_ms=policy.cancel_after_ms,
        )
        self._last_pricing_debug.update(
            {
                "expected_maker_fee_bps": float(fee_rules.maker_fee_bps),
                "assumed_hedge_fee_bps": float(fee_rules.hedge_fee_bps),
                "fee_snapshot_age_s": (
                    None
                    if fee_rules.fee_snapshot_age_s is None
                    else float(fee_rules.fee_snapshot_age_s)
                ),
                "hedge_route": effective_hedge_route,
                "hedge_instrument_id": str(hedge_instrument_id),
                "hedge_submit_ts_ms": int(fill_ts_ms),
            }
        )
        pending = PendingHedgeState(
            fill_id=fill_id,
            side=hedge_side,
            requested_qty=abs(Decimal(str(hedge_qty))),
            remaining_qty=abs(Decimal(str(hedge_qty))),
            limit_price=limit_price,
            route=policy.route,
            time_in_force=policy.time_in_force,
            outside_rth=order.outside_rth,
            include_overnight=policy.include_overnight,
            cancel_after_ms=policy.cancel_after_ms,
        )
        _ = fee_rules
        return order, pending
        self._hedge_backlog = None
        self.tradeable = True
        self.hedge_disabled_reason = None

    def _store_take_take_residual(self, *, side: str, qty: Decimal, ts_ms: int) -> None:
        normalized_qty = abs(Decimal(str(qty)))
        if normalized_qty <= 0:
            self._take_take_residual_base_fill = None
            return
        self._take_take_residual_base_fill = {
            "side": str(side).strip().upper(),
            "qty": normalized_qty,
            "ts_ms": max(0, int(ts_ms)),
        }

    def _finalize_take_take_hedge(self, client_order_id: Any, *, now_ns: int) -> None:
        order_id = str(client_order_id or "").strip()
        if not order_id:
            return
        accumulator = self._take_take_fill_accumulators.pop(order_id, None)
        if not isinstance(accumulator, dict):
            return
        fill_qty = Decimal(str(accumulator.get("qty", "0")))
        if fill_qty <= 0:
            return
        fill_side = str(accumulator.get("side", "")).strip().upper() or "BUY"
        fill_ts_ms = max(0, int(accumulator.get("ts_ms", max(0, int(now_ns) // 1_000_000))))
        signed_qty = self._signed_fill_qty(side=fill_side, qty=fill_qty)
        residual = self._take_take_residual_base_fill
        if isinstance(residual, dict):
            residual_side = str(residual.get("side", "")).strip().upper() or "BUY"
            residual_qty = Decimal(str(residual.get("qty", "0")))
            signed_qty += self._signed_fill_qty(side=residual_side, qty=residual_qty)
        if signed_qty == 0:
            self._take_take_residual_base_fill = None
            return
        aggregated_side = "BUY" if signed_qty > 0 else "SELL"
        aggregated_qty = abs(signed_qty)
        self._store_take_take_residual(
            side=aggregated_side,
            qty=aggregated_qty,
            ts_ms=fill_ts_ms,
        )
        hedgeable_qty = abs(
            translate_hyperliquid_fill_to_ibkr_shares(
                fill_qty=aggregated_qty,
                min_share_increment=Decimal(
                    str(getattr(self.config, "hedge_min_share_increment", Decimal("1")))
                ),
            )
        )
        if hedgeable_qty <= 0:
            return
        fill = MakerFill(
            fill_id=f"take_take:{order_id}",
            side=aggregated_side,
            qty=hedgeable_qty,
            price=self._required_decimal(accumulator.get("price"), field_name="price"),
            ts_ms=fill_ts_ms,
        )
        quote = self._reference_quote_snapshot(now_ns=now_ns)
        if quote is None:
            self._queue_hedge_backlog_for_fill(
                fill=fill,
                maker_fee_bps=self._fee_assumptions().maker_maker_fee_bps,
                blocked_reason="missing_ref_quote",
            )
            return
        fee_assumptions = self._fee_assumptions()
        order = self.record_maker_fill(
            fill=fill,
            quote=quote,
            maker_fee_bps=fee_assumptions.maker_maker_fee_bps,
        )
        if order is None:
            return
        remaining_qty = aggregated_qty - hedgeable_qty
        self._store_take_take_residual(
            side=aggregated_side,
            qty=remaining_qty,
            ts_ms=fill_ts_ms,
        )
        self._update_pending_hedge_order_id(self._submit_hedge_intent(order))
        self._cancel_managed_maker_orders()

    def _handle_take_take_fill_event(self, event: Any, *, now_ns: int) -> None:
        client_order_id = self._accumulate_take_take_fill(event, now_ns=now_ns)
        if client_order_id is None:
            return
        if self._managed_maker_state_for_client_order_id(client_order_id) is not None:
            return
        self._finalize_take_take_hedge(client_order_id, now_ns=now_ns)

    def _handle_hedge_fill_event(self, event: Any) -> None:
        if self._pending_hedge is None:
            return
        instrument_id = getattr(event, "instrument_id", None)
        if instrument_id is not None:
            self._last_pricing_debug["hedge_instrument_id"] = str(instrument_id)
        event_ts_ms = max(0, self._quote_ts_ns(getattr(event, "ts_event", 0)) // 1_000_000)
        report = HedgeExecutionReport(
            order_id=(
                str(getattr(event, "client_order_id", "")).strip()
                or str(getattr(event, "venue_order_id", "")).strip()
                or str(getattr(event, "trade_id", "")).strip()
            ),
            ok=True,
            filled_qty=self._required_decimal(
                getattr(event, "last_qty", None),
                field_name="last_qty",
            ),
            avg_fill_price=self._required_decimal(
                getattr(event, "last_px", None),
                field_name="last_px",
            ),
        )
        fill_id = str(getattr(event, "trade_id", "")).strip()
        if fill_id in self._seen_fill_ids:
            return
        if fill_id:
            self._remember_fill_id(fill_id)
        publish_shared_trade(
            self._publish_json,
            strategy_id=self._external_strategy_id,
            event=event,
            instrument_lookup=self._resolve_instrument,
            trade_role="hedge",
        )
        submit_ts_ms = self._last_pricing_debug.get("hedge_submit_ts_ms")
        if submit_ts_ms is not None:
            self._last_pricing_debug["hedge_latency_ms"] = max(
                0,
                int(event_ts_ms) - int(submit_ts_ms),
            )
        reference_quote = self._reference_quote_snapshot(now_ns=max(0, int(event_ts_ms) * 1_000_000))
        reference_mid = reference_quote.mid if reference_quote is not None else None
        pending_side = self._pending_hedge.side
        if reference_mid is not None and report.avg_fill_price is not None and reference_mid > 0:
            fill_px = Decimal(str(report.avg_fill_price))
            if pending_side == "BUY":
                slippage_bps = ((fill_px - reference_mid) / reference_mid) * Decimal("10000")
            else:
                slippage_bps = ((reference_mid - fill_px) / reference_mid) * Decimal("10000")
            self._last_pricing_debug["hedge_slippage_bps_vs_mid"] = float(slippage_bps)
        self.apply_hedge_execution(report)

    def _pending_hedge_matches_event(self, event: Any) -> bool:
        if self._pending_hedge is None:
            return False
        instrument_id = getattr(event, "instrument_id", None)
        if instrument_id is not None and not (
            self._instrument_id_matches(instrument_id, self._hedge_instrument_id(self._hedge_route()))
            or self._instrument_id_matches(instrument_id, self.config.reference_instrument_id)
        ):
            return False
        event_order_id = (
            str(getattr(event, "client_order_id", "")).strip()
            or str(getattr(event, "venue_order_id", "")).strip()
            or str(getattr(event, "order_id", "")).strip()
        )
        if self._pending_hedge.order_id and event_order_id:
            return self._pending_hedge.order_id == event_order_id
        return True

    def _fail_pending_hedge(self, *, reason: str, event: Any) -> None:
        if not self._pending_hedge_matches_event(event):
            return
        self._disable_hedging(reason)
        self._publish_state_snapshot(now_ns=self._quote_ts_ns(getattr(event, "ts_event", 0)))

    def _record_venue_protection(
        self,
        *,
        reason: object,
        source_event: str,
        client_order_id: object | None = None,
    ) -> None:
        raw_reason = str(reason or "")
        normalized_reason = normalize_reason_text(reason) or "unknown"
        quota_fields = extract_hyperliquid_request_quota(reason)
        diagnostics: dict[str, Any] = {
            "source_event": source_event,
            "raw_reason": raw_reason,
            "reason": normalized_reason,
        }
        client_order_id_text = str(client_order_id or "").strip()
        if client_order_id_text:
            diagnostics["client_order_id"] = client_order_id_text
        diagnostics.update(quota_fields)
        self._last_venue_protection = diagnostics
        now_ns = int(self.clock.timestamp_ns())
        self._last_pricing_debug.update(
            {
                "venue_protection_reason": raw_reason or normalized_reason,
                "venue_protection_source_event": source_event,
                **quota_fields,
            }
        )
        with suppress(Exception):
            self._publish_event(
                "venue_protection_circuit_breaker",
                ts_ns=now_ns,
                source_event=source_event,
                reason=normalized_reason,
                raw_reason=raw_reason,
                client_order_id=client_order_id_text,
                **quota_fields,
            )
        with suppress(Exception):
            self._publish_actionable_alert(
                alert_key=ALERT_KEY_VENUE_PROTECTION_CIRCUIT_BREAKER,
                message=(
                    "venue_protection_circuit_breaker triggered "
                    f"source_event={source_event} reason={normalized_reason!r}"
                ),
                level="error",
                reason_code=ALERT_KEY_VENUE_PROTECTION_CIRCUIT_BREAKER,
                cooldown_ms=ALERT_COOLDOWN_VENUE_PROTECTION_CIRCUIT_BREAKER_MS,
                transition=f"{source_event}:{normalized_reason}",
                now_ns=now_ns,
                source_event=source_event,
                raw_reason=raw_reason,
                client_order_id=client_order_id_text,
                **quota_fields,
            )
        with suppress(Exception):
            self.log.error(
                "MakerV4 venue protection triggered "
                f"strategy_id={self._external_strategy_id} "
                f"source_event={source_event} "
                f"client_order_id={client_order_id_text or 'unknown'} "
                f"reason={raw_reason or normalized_reason}"
                + (
                    " "
                    f"quota_requests_used={quota_fields['quota_requests_used']} "
                    f"quota_requests_cap={quota_fields['quota_requests_cap']} "
                    f"quota_cumulative_volume_traded={quota_fields['quota_cumulative_volume_traded']}"
                    if quota_fields
                    else ""
                ),
            )

    def _handle_venue_protection_event(self, event: Any, *, source_event: str) -> bool:
        reason = getattr(event, "reason", None)
        if not is_venue_protection_reason(reason):
            return False
        instrument_id = getattr(event, "instrument_id", None)
        maker_order_event = (
            instrument_id is not None
            and self._instrument_id_matches(instrument_id, self.config.maker_instrument_id)
        ) or (
            self._managed_maker_state_for_client_order_id(
                getattr(event, "client_order_id", None),
            )
            is not None
        )
        self._reconcile_managed_maker_order(event)
        self._record_venue_protection(
            reason=reason,
            source_event=source_event,
            client_order_id=getattr(event, "client_order_id", None),
        )
        if maker_order_event:
            self._publish_state_snapshot(now_ns=self._quote_ts_ns(getattr(event, "ts_event", 0)))
            return True
        self._disable_hedging("venue_protection")
        self._publish_state_snapshot(now_ns=self._quote_ts_ns(getattr(event, "ts_event", 0)))
        return True

    def _reconcile_maker_terminal_event(self, event: Any) -> bool:
        if not self._reconcile_managed_maker_order(event):
            return False
        self._finalize_take_take_hedge(
            getattr(event, "client_order_id", None),
            now_ns=self._quote_ts_ns(getattr(event, "ts_event", 0)),
        )
        self._publish_state_snapshot(now_ns=self._quote_ts_ns(getattr(event, "ts_event", 0)))
        return True

    def on_start(self) -> None:
        if self._pending_hedge is None and self._hedge_backlog is None:
            self.tradeable = True
            self.hedge_disabled_reason = None
        try:
            self._load_runtime_params()
        except Exception as exc:
            self._publish_runtime_params_failure(
                context="start",
                exc=exc,
                now_ns=int(self.clock.timestamp_ns()),
            )
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

        provider_start = getattr(self._reference_balance_snapshot_provider, "start", None)
        if callable(provider_start):
            try:
                provider_start(strategy=self)
            except TypeError:
                with suppress(Exception):
                    provider_start()

        self._reclaim_managed_maker_orders_from_cache()
        self._publish_balances()
        self._publish_state_snapshot()

    def on_stop(self) -> None:
        self._cancel_managed_maker_orders()
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
        provider_stop = getattr(self._reference_balance_snapshot_provider, "stop", None)
        if callable(provider_stop):
            with suppress(Exception):
                provider_stop()
        self._publish_state_snapshot(state_override="on_stop")

    def on_market_exit(self) -> None:
        now_ns = int(self.clock.timestamp_ns())
        self._publish_market_exit_alert(
            alert_key="market_exit_attempt",
            message=(
                "market_exit_attempt triggered "
                f"strategy_id={self._external_strategy_id}"
            ),
            now_ns=now_ns,
            transition=f"market_exit_attempt:{now_ns}",
            pending_hedge_order_id=(
                None if self._pending_hedge is None else self._pending_hedge.order_id
            ),
            pending_hedge_route=(
                None if self._pending_hedge is None else self._pending_hedge.route
            ),
            open_positions=len(self._open_positions() or []),
            managed_orders=self._tracked_managed_order_count(),
        )
        self._publish_event(
            "market_exit_attempt",
            ts_ns=now_ns,
            open_positions=len(self._open_positions() or []),
            managed_orders=self._tracked_managed_order_count(),
        )

    def on_quote_tick(self, tick: Any) -> None:
        instrument_id = getattr(tick, "instrument_id", None)
        if instrument_id is None:
            return

        bid = self._decimal_or_none(getattr(tick, "bid_price", None))
        ask = self._decimal_or_none(getattr(tick, "ask_price", None))
        ts_ns = self._quote_ts_ns(getattr(tick, "ts_event", 0))
        self._refresh_runtime_params_if_due(now_ns=ts_ns)
        self._update_quote_snapshot(
            instrument_id=instrument_id,
            bid=bid,
            ask=ask,
            ts_ns=ts_ns,
        )
        if self._execution_mode() == "take_take":
            self._reconcile_closed_take_take_orders_from_cache(now_ns=ts_ns)
        self._retry_hedge_backlog(now_ns=ts_ns)
        self._refresh_maker_quotes(now_ns=ts_ns)
        if bid is not None and ask is not None:
            self._publish_market_bbo(
                instrument_id=instrument_id,
                bid=bid,
                ask=ask,
                ts_ns=ts_ns,
            )
        self._publish_balances_if_due()
        self._publish_state_snapshot(now_ns=ts_ns)

    def on_order_filled(self, event: Any) -> None:
        instrument_id = getattr(event, "instrument_id", None)
        if instrument_id is None:
            return

        now_ns = self._quote_ts_ns(getattr(event, "ts_event", 0))
        if self._event_has_market_exit_tag(event):
            publish_shared_trade(
                self._publish_json,
                strategy_id=self._external_strategy_id,
                event=event,
                instrument_lookup=self._resolve_instrument,
                trade_role=self._market_exit_trade_role(instrument_id),
                extra_fields={"market_exit": True, "fill_context": "market_exit"},
            )
            self._publish_market_exit_alert(
                alert_key="market_exit_fill",
                message=(
                    "market_exit_fill "
                    f"instrument_id={instrument_id} "
                    f"client_order_id={getattr(event, 'client_order_id', None)}"
                ),
                now_ns=now_ns,
                transition=(
                    f"market_exit_fill:{getattr(event, 'client_order_id', None)}:"
                    f"{getattr(event, 'trade_id', None)}"
                ),
                instrument_id=str(instrument_id),
                client_order_id=str(getattr(event, "client_order_id", "")).strip(),
                trade_id=str(getattr(event, "trade_id", "")).strip(),
            )
            self._publish_state_snapshot(now_ns=now_ns)
            return
        if instrument_id == self.config.maker_instrument_id:
            maker_state = self._managed_maker_state_for_client_order_id(
                getattr(event, "client_order_id", None),
            )
            is_take_take_fill = maker_state is not None and not maker_state.post_only
            if not is_take_take_fill:
                is_take_take_fill = self._is_recent_take_take_order_id(
                    getattr(event, "client_order_id", None),
                    now_ns=now_ns,
                )
            if maker_state is not None:
                publish_shared_trade(
                    self._publish_json,
                    strategy_id=self._external_strategy_id,
                    event=event,
                    instrument_lookup=self._resolve_instrument,
                    trade_role="maker",
                )
            self._apply_maker_fill_to_managed_order(event)
            if is_take_take_fill:
                if self._cache_order_is_closed(getattr(event, "client_order_id", None)):
                    self._reconcile_managed_maker_order(event)
                self._handle_take_take_fill_event(event, now_ns=now_ns)
            else:
                self._handle_maker_fill_event(event, now_ns=now_ns)
        elif self._instrument_id_matches(instrument_id, self._hedge_instrument_id(self._hedge_route())) or self._instrument_id_matches(instrument_id, self.config.reference_instrument_id):
            self._handle_hedge_fill_event(event)
        self._publish_state_snapshot(now_ns=now_ns)

    def on_order_rejected(self, event: Any) -> None:
        if self._handle_venue_protection_event(event, source_event="order_rejected"):
            return
        if self._reconcile_maker_terminal_event(event):
            return
        self._fail_pending_hedge(reason="hedge_rejected", event=event)

    def on_order_canceled(self, event: Any) -> None:
        if self._reconcile_maker_terminal_event(event):
            return
        self._fail_pending_hedge(reason="hedge_canceled", event=event)

    def on_order_expired(self, event: Any) -> None:
        if self._reconcile_maker_terminal_event(event):
            return
        self._fail_pending_hedge(reason="hedge_timeout", event=event)

    def on_order_denied(self, event: Any) -> None:
        reason = str(getattr(event, "reason", "")).strip()
        if reason == "MARKET_EXIT_IN_PROGRESS" or self._event_has_market_exit_tag(event):
            now_ns = self._quote_ts_ns(getattr(event, "ts_event", 0))
            self._publish_market_exit_alert(
                alert_key="market_exit_denied",
                message=(
                    "market_exit_denied "
                    f"instrument_id={getattr(event, 'instrument_id', None)} "
                    f"reason={reason or 'unknown'}"
                ),
                now_ns=now_ns,
                transition=(
                    f"market_exit_denied:{getattr(event, 'client_order_id', None)}:"
                    f"{reason or 'unknown'}"
                ),
                instrument_id=str(getattr(event, "instrument_id", "")).strip(),
                client_order_id=str(getattr(event, "client_order_id", "")).strip(),
                reason=reason or None,
            )
            self._publish_state_snapshot(now_ns=now_ns)
            return
        self._fail_pending_hedge(reason="hedge_denied", event=event)

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
        if self._pending_hedge is not None:
            self._disable_hedging("pending_hedge_exists")
            return None
        self._remember_fill_id(fill_id)

        if self._hedge_backlog is not None:
            self._queue_hedge_backlog_for_fill(
                fill=fill,
                maker_fee_bps=maker_fee_bps,
                blocked_reason=self._hedge_backlog.blocked_reason,
            )
            self._retry_hedge_backlog(now_ns=max(0, int(fill.ts_ms)) * 1_000_000)
            return None

        quote_error = validate_ibkr_quote(
            bid=quote.bid,
            ask=quote.ask,
            quote_age_ms=quote.age_ms,
            max_quote_age_ms=int(getattr(self.config, "max_ibkr_quote_age_ms", 1_000)),
            max_spread_bps=Decimal(str(getattr(self.config, "max_ibkr_spread_bps", "25"))),
        )
        if quote_error is not None:
            if self._is_retryable_hedge_block_reason(quote_error):
                self._queue_hedge_backlog_for_fill(
                    fill=fill,
                    maker_fee_bps=maker_fee_bps,
                    blocked_reason=quote_error,
                )
            else:
                self._disable_hedging(quote_error)
            return None

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

        pending = self._build_pending_hedge_state(
            fill_id=fill_id,
            hedge_side=hedge_side,
            hedge_qty=hedge_qty,
            quote=quote,
            maker_fee_bps=maker_fee_bps,
            fill_ts_ms=int(fill.ts_ms),
        )
        if isinstance(pending, str):
            if self._is_retryable_hedge_block_reason(pending):
                self._queue_hedge_backlog(
                    fill_id=fill_id,
                    side=hedge_side,
                    requested_qty=hedge_qty,
                    blocked_reason=pending,
                    fill_ts_ms=int(fill.ts_ms),
                    maker_fee_bps=maker_fee_bps,
                )
            else:
                self._disable_hedging(pending)
            return None
        order, pending_state = pending
        self._hedge_requests.append(order)
        self._pending_hedge = pending_state
        self._hedge_backlog = None
        self.tradeable = True
        self.hedge_disabled_reason = None
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
                route=self._pending_hedge.route,
                time_in_force=self._pending_hedge.time_in_force,
                outside_rth=self._pending_hedge.outside_rth,
                include_overnight=self._pending_hedge.include_overnight,
                cancel_after_ms=self._pending_hedge.cancel_after_ms,
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
        if self._hedge_backlog is not None:
            snapshot["hedge_backlog"] = asdict(self._hedge_backlog)
        if isinstance(self._take_take_residual_base_fill, dict):
            snapshot["take_take_residual_base_fill"] = dict(self._take_take_residual_base_fill)
        return snapshot

    def restore_state(self, snapshot: dict[str, object]) -> None:
        self.tradeable = bool(snapshot.get("tradeable", True))
        reason = snapshot.get("hedge_disabled_reason")
        self.hedge_disabled_reason = None if reason is None else str(reason)
        fill_ids = snapshot.get("last_fill_ids_head")
        if isinstance(fill_ids, list):
            self._fill_ids_head = [str(value) for value in fill_ids if str(value).strip()]
            self._seen_fill_ids = set(self._fill_ids_head)
        else:
            self._fill_ids_head = []
            self._seen_fill_ids = set()
        pending = snapshot.get("pending_hedge")
        if isinstance(pending, dict):
            self._pending_hedge = PendingHedgeState(
                fill_id=str(pending.get("fill_id", "")),
                side=str(pending.get("side", "")),
                requested_qty=Decimal(str(pending.get("requested_qty", "0"))),
                remaining_qty=Decimal(str(pending.get("remaining_qty", "0"))),
                limit_price=Decimal(str(pending.get("limit_price", "0"))),
                route=str(pending.get("route", "SMART")),
                time_in_force=str(pending.get("time_in_force", "IOC")),
                outside_rth=bool(pending.get("outside_rth", False)),
                include_overnight=bool(pending.get("include_overnight", False)),
                cancel_after_ms=(
                    None
                    if pending.get("cancel_after_ms") in (None, "")
                    else int(pending.get("cancel_after_ms"))
                ),
                order_id=(
                    None
                    if pending.get("order_id") in (None, "")
                    else str(pending.get("order_id"))
                ),
            )
        else:
            self._pending_hedge = None
        backlog = snapshot.get("hedge_backlog")
        if isinstance(backlog, dict):
            self._hedge_backlog = HedgeBacklogState(
                fill_id=str(backlog.get("fill_id", "")),
                side=str(backlog.get("side", "")),
                requested_qty=Decimal(str(backlog.get("requested_qty", "0"))),
                blocked_reason=str(backlog.get("blocked_reason", "")),
                fill_ts_ms=int(backlog.get("fill_ts_ms", 0) or 0),
                maker_fee_bps=Decimal(str(backlog.get("maker_fee_bps", "0"))),
            )
        else:
            self._hedge_backlog = None
        residual = snapshot.get("take_take_residual_base_fill")
        if isinstance(residual, dict):
            self._store_take_take_residual(
                side=str(residual.get("side", "")).strip().upper() or "BUY",
                qty=Decimal(str(residual.get("qty", "0"))),
                ts_ms=int(residual.get("ts_ms", 0) or 0),
            )
        else:
            self._take_take_residual_base_fill = None

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

    def _supplemental_balance_snapshot(self) -> dict[str, Any] | None:
        provider_snapshot = getattr(self._reference_balance_snapshot_provider, "snapshot", None)
        if not callable(provider_snapshot):
            return None
        with suppress(Exception):
            snapshot = provider_snapshot()
            if isinstance(snapshot, dict):
                return snapshot
        return None

    def _publish_event(self, name: str, *, ts_ns: int | None = None, **payload: Any) -> None:
        makerv3_publisher_mod.publish_event(self, name, ts_ns=ts_ns, **payload)

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
        return shared_alerts_mod.publish_actionable_alert(
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
        shared_alerts_mod.publish_alert(
            self,
            message,
            level,
            ts_ns=ts_ns,
            alert_key=alert_key,
            reason_code=reason_code,
            actionable=actionable,
            **extra_fields,
        )

    def _publish_json(self, topic: str, payload: dict[str, Any] | list[Any]) -> None:
        makerv3_publisher_mod.publish_json(self, topic, payload)


__all__ = [
    "MakerV4Strategy",
    "MakerV4StrategyConfig",
]
