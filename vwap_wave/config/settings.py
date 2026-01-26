# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Configuration Settings
# -------------------------------------------------------------------------------------------------
"""
Global configuration parameters for VWAP Wave trading system.

All configurable parameters are defined as dataclasses for type safety and documentation.
Parameters are grouped by functional area.
"""

from dataclasses import dataclass
from dataclasses import field
from decimal import Decimal
from typing import Tuple


@dataclass(frozen=True)
class VWAPConfig:
    """VWAP engine configuration."""

    session_reset_hour: int = 0  # Hour (UTC) to reset VWAP calculation
    sd_bands: Tuple[float, ...] = (1.0, 2.0, 3.0)  # Standard deviation band levels


@dataclass(frozen=True)
class InitialBalanceConfig:
    """Initial Balance configuration."""

    ib_period_minutes: int = 60  # Duration of IB period
    extensions: Tuple[float, ...] = (1.0, 2.0, 3.0)  # IB range extension multipliers


@dataclass(frozen=True)
class AcceptanceConfig:
    """Acceptance detection thresholds."""

    time_bars: int = 2  # Consecutive closes required for time acceptance
    distance_atr_mult: float = 1.0  # ATR multiple for distance acceptance
    momentum_threshold: float = 0.7  # Body-to-range ratio for momentum candles
    volume_mult: float = 1.5  # Volume multiple vs average for confirmation


@dataclass(frozen=True)
class ExhaustionConfig:
    """Exhaustion detection configuration."""

    sd_min: float = 2.0  # Minimum SD band for exhaustion zone
    ib_extension: float = 3.0  # IB extension level for exhaustion zone
    volume_spike_mult: float = 2.0  # Volume spike threshold
    price_progress_threshold: float = 0.2  # Max ATR progress on spike (effort vs result)
    absorption_body_ratio: float = 0.3  # Max body ratio for absorption candle
    confirmation_bars: int = 2  # Lag rule: bars after absorption before signal
    volume_dropoff_threshold: float = 0.7  # Volume must drop to this ratio to confirm


@dataclass(frozen=True)
class CVDConfig:
    """CVD divergence detection configuration."""

    lookback_bars: int = 5  # Bars to scan for divergence
    min_divergence_delta: float = 0.1  # Minimum normalized divergence magnitude


@dataclass(frozen=True)
class VolumeProfileConfig:
    """Volume profile configuration."""

    lookback_bars: int = 50  # Bars for building profile
    price_buckets: int = 50  # Granularity of profile
    hvn_percentile: int = 70  # High Volume Node threshold percentile
    lvn_percentile: int = 30  # Low Volume Node threshold percentile


@dataclass(frozen=True)
class RiskConfig:
    """Risk management configuration."""

    base_risk_per_trade: Decimal = Decimal("0.01")  # 1% base risk
    max_risk_per_trade: Decimal = Decimal("0.02")  # 2% max for high confidence
    min_risk_per_trade: Decimal = Decimal("0.005")  # 0.5% min for low confidence
    max_daily_drawdown: Decimal = Decimal("0.05")  # 5% daily limit
    max_weekly_drawdown: Decimal = Decimal("0.10")  # 10% weekly limit
    max_concurrent_positions: int = 3
    correlation_risk_reduction: float = 0.5
    high_confidence_threshold: float = 0.8
    medium_confidence_threshold: float = 0.6
    low_confidence_threshold: float = 0.4


@dataclass(frozen=True)
class TradeManagementConfig:
    """Trade management configuration."""

    trail_activation_rr: float = 1.0  # R-multiple to activate trailing stop
    trail_distance_atr: float = 1.5  # Trail distance in ATR
    trail_step_atr: float = 0.25  # Minimum move before trail updates
    partial_exit_enabled: bool = True
    partial_exit_rr: float = 1.5  # R-multiple for partial exit
    partial_exit_percent: float = 0.5  # Percentage to exit
    max_trade_duration_bars: int = 50  # Maximum bars before forced review


@dataclass
class VWAPWaveConfig:
    """Master configuration aggregating all subsystem configs."""

    vwap: VWAPConfig = field(default_factory=VWAPConfig)
    ib: InitialBalanceConfig = field(default_factory=InitialBalanceConfig)
    acceptance: AcceptanceConfig = field(default_factory=AcceptanceConfig)
    exhaustion: ExhaustionConfig = field(default_factory=ExhaustionConfig)
    cvd: CVDConfig = field(default_factory=CVDConfig)
    volume_profile: VolumeProfileConfig = field(default_factory=VolumeProfileConfig)
    risk: RiskConfig = field(default_factory=RiskConfig)
    trade_mgmt: TradeManagementConfig = field(default_factory=TradeManagementConfig)
