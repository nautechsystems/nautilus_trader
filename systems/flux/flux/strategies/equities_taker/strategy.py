"""
Explicit equities-taker family built on the shared MakerV4 taker path.
"""

from __future__ import annotations

from decimal import Decimal

from flux.strategies.makerv4 import fees as fees_mod
from flux.strategies.makerv4.market_data import IbkrQuoteSnapshot
from flux.strategies.makerv4.pricing import build_ibkr_ioc_limit
from flux.strategies.makerv4.pricing import validate_ibkr_quote
from flux.strategies.equities_taker import runtime_params as runtime_params_mod
from flux.strategies.makerv3.strategy import OrderQtyUnit
from flux.strategies.makerv3.strategy import SpotCashBorrowingPolicy
from flux.strategies.makerv4.strategy import MakerV4Strategy
from flux.strategies.shared.equities_arb.hedging import HedgeOrderIntent
from flux.strategies.shared.equities_arb.hedging import PendingHedgeState
from flux.strategies.shared.equities_arb.hedging import build_hedge_policy_payload
from nautilus_trader.config import NonNegativeFloat
from nautilus_trader.config import NonNegativeInt
from nautilus_trader.config import PositiveInt
from nautilus_trader.config import StrategyConfig
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
        self._runtime_params = dict(runtime_params_mod.EQUITIES_TAKER_RUNTIME_PARAM_DEFAULTS)

    def _execution_mode(self) -> str:
        return "take_take"

    def _refresh_maker_quotes(self, *, now_ns: int) -> None:
        self._refresh_take_take_orders(now_ns=now_ns)

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
        _ = fee_rules
        return order, pending


__all__ = [
    "EquitiesTakerStrategy",
    "EquitiesTakerStrategyConfig",
]
