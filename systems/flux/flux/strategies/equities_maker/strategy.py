"""
Thin equities-maker family built on the shared MakerV4 maker hedge path.
"""

from __future__ import annotations

from decimal import Decimal

from flux.strategies.equities_maker import runtime_params as runtime_params_mod
from flux.strategies.makerv3.strategy import OrderQtyUnit
from flux.strategies.makerv3.strategy import SpotCashBorrowingPolicy
from flux.strategies.makerv4.strategy import MakerV4Strategy
from nautilus_trader.config import NonNegativeFloat
from nautilus_trader.config import NonNegativeInt
from nautilus_trader.config import PositiveInt
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.identifiers import InstrumentId


class EquitiesMakerStrategyConfig(StrategyConfig, frozen=True):
    """
    Equities-maker config surface with shared global risk only.
    """

    maker_instrument_id: InstrumentId
    reference_instrument_id: InstrumentId
    order_qty: Decimal
    portfolio_asset_id: str | None = None
    execution_account_scope_id: str | None = None
    qty_unit: OrderQtyUnit = "venue"
    external_strategy_id: str = "equities_maker"
    bot_on: bool | None = None
    qty: Decimal | None = None
    des_qty_global: NonNegativeFloat | None = None
    max_qty_global: NonNegativeFloat | None = None
    max_skew_bps_global: NonNegativeFloat | None = None
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


class EquitiesMakerStrategy(MakerV4Strategy):
    """
    Thin maker-only family preserving the existing MakerV4 hedge contract.
    """

    def __init__(self, config: EquitiesMakerStrategyConfig) -> None:
        super().__init__(config)
        self._runtime_params = dict(runtime_params_mod.EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS)

    def _execution_mode(self) -> str:
        return "maker_hedge"


__all__ = [
    "EquitiesMakerStrategy",
    "EquitiesMakerStrategyConfig",
]
