"""
Implement the first MakerV4 pure strategy core slice.
"""

from __future__ import annotations

from contextlib import suppress
from dataclasses import asdict
from decimal import Decimal
from typing import Any

from flux.common.keys import FluxRedisKeys
from flux.common.portfolio_inventory import DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
from flux.common.portfolio_inventory import StrategyInventoryComponent
from flux.common.portfolio_inventory import decode_portfolio_inventory
from flux.common.portfolio_inventory import encode_component
from flux.strategies.makerv3 import publisher as makerv3_publisher_mod
from flux.strategies.makerv3 import inventory as inventory_mod
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
from nautilus_trader.model.identifiers import InstrumentId
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
    ibkr_hedge_route: str = ""


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
        self._portfolio_inventory_client = None
        self._portfolio_inventory_portfolio_id: str | None = None
        self._portfolio_inventory_namespace = "flux"
        self._portfolio_inventory_schema_version = "v1"
        self._portfolio_inventory_stale_after_ms = DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
        self._portfolio_inventory_allow_partial_global_risk = False
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
        self._reference_balance_snapshot_provider = None
        self.tradeable = True
        self.hedge_disabled_reason: str | None = None
        self._pending_hedge: PendingHedgeState | None = None
        self._hedge_requests: list[HedgeOrderIntent] = []
        self._seen_fill_ids: set[str] = set()
        self._fill_ids_head: list[str] = []

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

    def _effective_bot_on(self) -> bool:
        bot_on = self._runtime_params.get("bot_on", getattr(self.config, "bot_on", False))
        return bool(bot_on)

    def _managed_orders(self) -> list[Any]:
        if self._pending_hedge is None:
            return []
        return [self._pending_hedge]

    def _tracked_managed_order_count(self) -> int:
        return len(self._managed_orders())

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

    def _local_position_summary(
        self,
        base_currency: str | None,
    ) -> inventory_mod.PositionExposureSummary:
        if not base_currency or self._maker_instrument_is_spot():
            return inventory_mod.PositionExposureSummary(venue_qty=None, base_qty=None)
        return self._position_exposure_summary(
            base_currency,
            instrument_id=self.config.maker_instrument_id,
        )

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
        base_currency = self._maker_base_currency_code()
        local_position_summary = self._local_position_summary(base_currency)
        local_spot_qty = self._local_spot_qty(base_currency)
        local_qty_base = inventory_mod.local_inventory_total(
            local_position_qty=local_position_summary.base_qty,
            local_spot_qty=local_spot_qty,
        )
        global_qty_base, diagnostics = self._shared_portfolio_inventory_snapshot(
            base_currency=base_currency,
            now_ms_value=now_ms_value,
        )
        local_inventory_source = "unavailable"
        if local_position_summary.base_qty is not None and local_spot_qty is not None:
            local_inventory_source = "positions_plus_spot"
        elif local_spot_qty is not None:
            local_inventory_source = "spot_balance"
        elif local_position_summary.base_qty is not None:
            local_inventory_source = "positions"
        global_complete = None if diagnostics is None else bool(diagnostics["global_qty_base_complete"])
        global_inventory_source = None
        if global_qty_base is not None:
            global_inventory_source = (
                "portfolio_component_sum"
                if global_complete is not False
                else "portfolio_component_partial_sum"
            )
        return {
            "base_currency": base_currency,
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
        ref_leg = self._quote_leg_snapshot(
            self.config.reference_instrument_id,
            now_ns=now_ns,
            force_venue="IBKR",
        )
        hedge_route = self._hedge_route()
        hedge_instrument_id = self._hedge_instrument_id(hedge_route)
        if self._instrument_id_matches(hedge_instrument_id, self.config.reference_instrument_id):
            hedge_leg = dict(ref_leg)
        else:
            hedge_leg = self._quote_leg_snapshot(
                hedge_instrument_id,
                now_ns=now_ns,
                force_venue="IBKR",
            )
            for field_name in ("symbol", "bid", "ask", "mid", "ts_ms", "age_ms"):
                if field_name not in hedge_leg and field_name in ref_leg:
                    hedge_leg[field_name] = ref_leg[field_name]
        if hedge_route:
            hedge_leg["route"] = hedge_route
        reference_bid = self._decimal_or_none(ref_leg.get("bid"))
        reference_ask = self._decimal_or_none(ref_leg.get("ask"))
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
        return publisher_mod.build_quote_snapshot_payload(
            maker_leg=maker_leg,
            hedge_leg=hedge_leg,
            ref_leg=ref_leg,
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
        )

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
            "maker_role_map": build_role_map_payload(
                maker_leg=str(self.config.maker_instrument_id),
                ref_leg=str(self.config.reference_instrument_id),
                hedge_leg=str(self._hedge_instrument_id(self._hedge_route())),
            ),
            "maker_v4": {
                "quote_snapshot": self._quote_snapshot_payload(now_ns=publish_ns),
            },
        }
        payload.update(inventory_fields)
        pricing_debug: dict[str, Any] = {}
        if isinstance(self._last_pricing_debug, dict) and self._last_pricing_debug:
            pricing_debug["pricing"] = dict(self._last_pricing_debug)
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
            outside_rth=self._pending_hedge.outside_rth,
            order_id=str(order_id),
        )

    def _submit_hedge_intent(self, intent: HedgeOrderIntent) -> str | None:
        _ = intent
        return None

    def _handle_maker_fill_event(self, event: Any, *, now_ns: int) -> None:
        if not bool(self._runtime_params.get("instant_hedge_enabled", True)):
            self._disable_hedging("instant_hedge_disabled")
            return
        hedge_style = str(self._runtime_params.get("hedge_style", "ioc_through_mid")).strip()
        if hedge_style != "ioc_through_mid":
            self._disable_hedging("unsupported_hedge_style")
            return
        quote = self._reference_quote_snapshot(now_ns=now_ns)
        if quote is None:
            self._disable_hedging("missing_ref_quote")
            return
        fill = MakerFill(
            fill_id=str(getattr(event, "trade_id", "")).strip(),
            side=str(getattr(event, "order_side", "")).strip(),
            qty=self._required_decimal(getattr(event, "last_qty", None), field_name="last_qty"),
            price=self._required_decimal(getattr(event, "last_px", None), field_name="last_px"),
            ts_ms=max(0, now_ns // 1_000_000),
        )
        order = self.record_maker_fill(
            fill=fill,
            quote=quote,
            maker_fee_bps=Decimal("0"),
        )
        if order is None:
            return
        self._update_pending_hedge_order_id(self._submit_hedge_intent(order))

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

    def on_start(self) -> None:
        if self._pending_hedge is None:
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

        provider_start = getattr(self._reference_balance_snapshot_provider, "start", None)
        if callable(provider_start):
            try:
                provider_start(strategy=self)
            except TypeError:
                with suppress(Exception):
                    provider_start()

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
        provider_stop = getattr(self._reference_balance_snapshot_provider, "stop", None)
        if callable(provider_stop):
            with suppress(Exception):
                provider_stop()
        self._publish_state_snapshot(state_override="on_stop")

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

    def on_order_filled(self, event: Any) -> None:
        instrument_id = getattr(event, "instrument_id", None)
        if instrument_id is None:
            return

        now_ns = self._quote_ts_ns(getattr(event, "ts_event", 0))
        if instrument_id == self.config.maker_instrument_id:
            self._handle_maker_fill_event(event, now_ns=now_ns)
        elif self._instrument_id_matches(instrument_id, self._hedge_instrument_id(self._hedge_route())) or self._instrument_id_matches(instrument_id, self.config.reference_instrument_id):
            self._handle_hedge_fill_event(event)
        self._publish_state_snapshot(now_ns=now_ns)

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
            max_cross_bps=Decimal(str(self._runtime_params["hedge_ioc_max_cross_bps"])),
            tick_size=Decimal(str(getattr(self.config, "hedge_price_tick_size", Decimal("0.01")))),
            quote_age_ms=quote.age_ms,
            max_quote_age_ms=int(getattr(self.config, "max_ibkr_quote_age_ms", 1_000)),
            max_spread_bps=Decimal(str(getattr(self.config, "max_ibkr_spread_bps", "25"))),
        )
        if limit_price is None:
            self._disable_hedging("invalid_hedge_limit")
            return None

        hedge_route = self._hedge_route()
        hedge_instrument_id = self._hedge_instrument_id(hedge_route)
        order = HedgeOrderIntent(
            instrument_id=str(hedge_instrument_id),
            side=hedge_side,
            qty=hedge_qty,
            limit_price=limit_price,
            time_in_force="IOC",
            outside_rth=bool(getattr(self.config, "outside_rth_hedge_enabled", False)),
        )
        self._hedge_requests.append(order)
        self._last_pricing_debug.update(
            {
                "expected_maker_fee_bps": float(fee_rules.maker_fee_bps),
                "assumed_hedge_fee_bps": float(fee_rules.hedge_fee_bps),
                "fee_snapshot_age_s": (
                    None
                    if fee_rules.fee_snapshot_age_s is None
                    else float(fee_rules.fee_snapshot_age_s)
                ),
                "hedge_route": hedge_route,
                "hedge_instrument_id": str(hedge_instrument_id),
                "hedge_submit_ts_ms": int(fill.ts_ms),
            }
        )
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
                outside_rth=bool(pending.get("outside_rth", False)),
                order_id=(
                    None
                    if pending.get("order_id") in (None, "")
                    else str(pending.get("order_id"))
                ),
            )
        else:
            self._pending_hedge = None

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

    def _publish_json(self, topic: str, payload: dict[str, Any] | list[Any]) -> None:
        makerv3_publisher_mod.publish_json(self, topic, payload)


__all__ = [
    "MakerV4Strategy",
    "MakerV4StrategyConfig",
]
