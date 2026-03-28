"""
Explicit equities-taker family built on the shared MakerV4 taker path.
"""

from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace
from typing import Any

from flux.strategies.makerv4 import fees as fees_mod
from flux.strategies.makerv4.managed_orders import ManagedMakerOrderState
from flux.strategies.makerv4.market_data import IbkrQuoteSnapshot
from flux.strategies.makerv4.pricing import build_effective_ibkr_fee_bps
from flux.strategies.makerv4.pricing import build_ibkr_ioc_limit
from flux.strategies.makerv4.pricing import build_take_take_limit_price
from flux.strategies.makerv4.pricing import validate_ibkr_quote
from flux.strategies.equities_taker import runtime_params as runtime_params_mod
from flux.strategies.makerv3.strategy import OrderQtyUnit
from flux.strategies.makerv3.strategy import SpotCashBorrowingPolicy
from flux.strategies.makerv4.strategy import MakerV4Strategy
from flux.strategies.shared.equities_arb.hedging import HedgeOrderIntent
from flux.strategies.shared.equities_arb.hedging import PendingHedgeState
from flux.strategies.shared.equities_arb.hedging import build_hedge_policy_payload
from flux.strategies.shared.trades import publish_trade as publish_shared_trade
from nautilus_trader.config import NonNegativeFloat
from nautilus_trader.config import NonNegativeInt
from nautilus_trader.config import PositiveInt
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId


class EquitiesTakerStrategyConfig(StrategyConfig, frozen=True):
    """
    Equities-taker config surface with shared global risk only.
    """

    maker_instrument_id: InstrumentId
    reference_instrument_id: InstrumentId
    order_qty: Decimal
    portfolio_asset_id: str | None = None
    execution_account_scope_id: str | None = None
    qty_unit: OrderQtyUnit = "venue"
    external_strategy_id: str = "equities_taker"
    bot_on: bool | None = None
    qty: Decimal | None = None
    des_qty_global: NonNegativeFloat | None = None
    max_qty_global: NonNegativeFloat | None = None
    max_skew_bps_global: NonNegativeFloat | None = None
    max_age_ms: PositiveInt | None = None
    bid_edge_take_bps: NonNegativeFloat | None = None
    ask_edge_take_bps: NonNegativeFloat | None = None
    take_cooldown_ms: NonNegativeInt | None = None
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
    reference_use_quote_ticks: bool = False
    cancel_all_instrument_orders: bool = False
    ibkr_primary_exchange: str = "NASDAQ"
    hedge_price_tick_size: Decimal = Decimal("0.01")
    hedge_min_share_increment: Decimal = Decimal("1")
    max_ibkr_quote_age_ms: int = 1_000
    max_ibkr_spread_bps: Decimal = Decimal("25")
    outside_rth_hedge_enabled: bool = False
    ibkr_hedge_route: str = ""
    hedge_fee_plan: str = "ibkr_pro_tiered"

    @property
    def active_order_qty(self) -> Decimal:
        return self.qty if self.qty is not None else self.order_qty


class EquitiesTakerStrategy(MakerV4Strategy):
    """
    Explicit taker-only family preserving the existing aggressive taker hedge contract.
    """

    def __init__(self, config: EquitiesTakerStrategyConfig) -> None:
        super().__init__(config)
        self._runtime_params = self._seed_runtime_params_from_config(config)

    @staticmethod
    def _seed_runtime_params_from_config(config: EquitiesTakerStrategyConfig) -> dict[str, Any]:
        seeded = dict(runtime_params_mod.EQUITIES_TAKER_RUNTIME_PARAM_DEFAULTS)
        seeded["qty"] = config.active_order_qty
        for name in runtime_params_mod.EQUITIES_TAKER_RUNTIME_PARAM_REGISTRY.names:
            if not hasattr(config, name):
                continue
            value = getattr(config, name)
            if value is None:
                continue
            seeded[name] = value
        return seeded

    def _execution_mode(self) -> str:
        return "take_take"

    def _submit_taker_order(self, *, side: str, target_price: Decimal, now_ns: int) -> None:
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
        self._last_take_submission_ns = max(0, int(now_ns))

    def _taker_signal(self, *, now_ns: int) -> tuple[str, Decimal] | None:
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
            hl_taker_fee_bps=fee_assumptions.hl_taker_fee_bps,
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
            hl_taker_fee_bps=fee_assumptions.hl_taker_fee_bps,
            hedge_fee_bps=hedge_fee_bps,
        )
        if sell_price is not None:
            return ("SELL", sell_price)
        return None

    def _refresh_taker_orders(self, *, now_ns: int) -> None:
        if self._managed_maker_orders:
            if any(state.post_only for state in self._managed_maker_orders.values()):
                self._cancel_managed_maker_orders()
            return
        signal = self._taker_signal(now_ns=now_ns)
        if signal is None:
            return
        if self._maker_order_hedge_qty() <= 0:
            self._disable_hedging("hedge_qty_rounds_to_zero")
            return
        side, target_price = signal
        self._submit_taker_order(side=side, target_price=target_price, now_ns=now_ns)

    def _reconcile_closed_taker_orders_from_cache(self, *, now_ns: int) -> None:
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

    def _handle_taker_fill_event(self, event: Any, *, now_ns: int) -> None:
        client_order_id = self._accumulate_take_take_fill(event, now_ns=now_ns)
        if client_order_id is None:
            return
        if self._managed_maker_state_for_client_order_id(client_order_id) is not None:
            return
        self._finalize_take_take_hedge(client_order_id, now_ns=now_ns)

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
        policy = build_hedge_policy_payload(
            configured_route=hedge_route,
            outside_rth_enabled=bool(getattr(self.config, "outside_rth_hedge_enabled", False)),
            is_regular_session=self._is_regular_hedge_session(ts_ms=int(fill_ts_ms)),
            hedge_mode="take_take",
        )
        effective_hedge_route = str(policy["route"]).strip().upper() or None
        hedge_instrument_id = self._hedge_instrument_id(effective_hedge_route)
        order = HedgeOrderIntent(
            instrument_id=str(hedge_instrument_id),
            side=hedge_side,
            qty=abs(Decimal(str(hedge_qty))),
            limit_price=limit_price,
            route=str(policy["route"]),
            time_in_force=str(policy["time_in_force"]),
            outside_rth=bool(policy["outside_rth"]),
            include_overnight=bool(policy["include_overnight"]),
            cancel_after_ms=policy["cancel_after_ms"],
        )
        pending = PendingHedgeState(
            fill_id=fill_id,
            side=hedge_side,
            requested_qty=abs(Decimal(str(hedge_qty))),
            remaining_qty=abs(Decimal(str(hedge_qty))),
            limit_price=limit_price,
            route=str(policy["route"]),
            time_in_force=str(policy["time_in_force"]),
            outside_rth=bool(policy["outside_rth"]),
            include_overnight=bool(policy["include_overnight"]),
            cancel_after_ms=policy["cancel_after_ms"],
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
        return order, pending

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
        self._record_supervisor_quote_observation(
            instrument_id=instrument_id,
            ts_ns=ts_ns,
        )
        self._refresh_quote_tradeability(now_ns=ts_ns)
        self._reconcile_closed_taker_orders_from_cache(now_ns=ts_ns)
        self._retry_hedge_backlog(now_ns=ts_ns)
        self._refresh_taker_orders(now_ns=ts_ns)
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
            if maker_state is not None:
                publish_shared_trade(
                    self._publish_json,
                    strategy_id=self._external_strategy_id,
                    event=event,
                    instrument_lookup=self._resolve_instrument,
                    trade_role="maker",
                )
            self._apply_maker_fill_to_managed_order(event)
            if maker_state is not None:
                if self._cache_order_is_closed(getattr(event, "client_order_id", None)):
                    self._reconcile_managed_maker_order(event)
                self._handle_taker_fill_event(event, now_ns=now_ns)
        elif self._instrument_id_matches(
            instrument_id,
            self._hedge_instrument_id(self._hedge_route()),
        ) or self._instrument_id_matches(instrument_id, self.config.reference_instrument_id):
            self._handle_hedge_fill_event(event)
        self._publish_state_snapshot(now_ns=now_ns)


__all__ = [
    "EquitiesTakerStrategy",
    "EquitiesTakerStrategyConfig",
]
