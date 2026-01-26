# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Instrument Configuration
# -------------------------------------------------------------------------------------------------
"""
Instrument-specific parameter overrides for VWAP Wave trading system.

Different instruments (forex majors, crypto pairs) may require different
parameter settings due to their unique volatility and liquidity characteristics.
"""

from dataclasses import dataclass
from decimal import Decimal
from typing import Dict
from typing import Optional

from vwap_wave.config.settings import AcceptanceConfig
from vwap_wave.config.settings import ExhaustionConfig
from vwap_wave.config.settings import InitialBalanceConfig
from vwap_wave.config.settings import RiskConfig
from vwap_wave.config.settings import TradeManagementConfig
from vwap_wave.config.settings import VWAPConfig
from vwap_wave.config.settings import VWAPWaveConfig


@dataclass(frozen=True)
class InstrumentOverrides:
    """Instrument-specific configuration overrides."""

    symbol: str
    vwap: Optional[VWAPConfig] = None
    ib: Optional[InitialBalanceConfig] = None
    acceptance: Optional[AcceptanceConfig] = None
    exhaustion: Optional[ExhaustionConfig] = None
    risk: Optional[RiskConfig] = None
    trade_mgmt: Optional[TradeManagementConfig] = None


# Forex Major Pairs - Lower volatility, tighter ranges
FOREX_MAJORS: Dict[str, InstrumentOverrides] = {
    "EUR/USD": InstrumentOverrides(
        symbol="EUR/USD",
        acceptance=AcceptanceConfig(
            time_bars=3,  # More bars for confirmation in liquid market
            distance_atr_mult=0.8,  # Tighter distance threshold
            momentum_threshold=0.65,
            volume_mult=1.3,
        ),
        risk=RiskConfig(
            base_risk_per_trade=Decimal("0.01"),
            max_risk_per_trade=Decimal("0.015"),  # Lower max for majors
        ),
    ),
    "GBP/USD": InstrumentOverrides(
        symbol="GBP/USD",
        acceptance=AcceptanceConfig(
            time_bars=2,
            distance_atr_mult=1.0,  # Higher volatility than EUR/USD
            momentum_threshold=0.7,
            volume_mult=1.4,
        ),
        risk=RiskConfig(
            base_risk_per_trade=Decimal("0.01"),
            max_risk_per_trade=Decimal("0.015"),
        ),
    ),
    "USD/JPY": InstrumentOverrides(
        symbol="USD/JPY",
        vwap=VWAPConfig(
            session_reset_hour=0,  # Reset at midnight UTC (Asia session start)
        ),
        acceptance=AcceptanceConfig(
            time_bars=2,
            distance_atr_mult=0.9,
            momentum_threshold=0.7,
            volume_mult=1.4,
        ),
    ),
    "USD/CHF": InstrumentOverrides(
        symbol="USD/CHF",
        acceptance=AcceptanceConfig(
            time_bars=3,
            distance_atr_mult=0.85,
            momentum_threshold=0.65,
            volume_mult=1.3,
        ),
    ),
}


# Crypto Pairs - Higher volatility, wider ranges
CRYPTO_PAIRS: Dict[str, InstrumentOverrides] = {
    "BTC/USDT": InstrumentOverrides(
        symbol="BTC/USDT",
        vwap=VWAPConfig(
            session_reset_hour=0,  # 24/7 market, reset at UTC midnight
        ),
        acceptance=AcceptanceConfig(
            time_bars=2,
            distance_atr_mult=1.2,  # Higher for crypto volatility
            momentum_threshold=0.75,
            volume_mult=1.8,  # Higher volume threshold
        ),
        exhaustion=ExhaustionConfig(
            sd_min=2.5,  # Higher SD for crypto extremes
            volume_spike_mult=2.5,
            price_progress_threshold=0.25,
        ),
        risk=RiskConfig(
            base_risk_per_trade=Decimal("0.0075"),  # Lower base for volatile asset
            max_risk_per_trade=Decimal("0.015"),
            max_daily_drawdown=Decimal("0.04"),  # Tighter daily limit
        ),
        trade_mgmt=TradeManagementConfig(
            trail_distance_atr=2.0,  # Wider trail for crypto
            trail_step_atr=0.5,
        ),
    ),
    "ETH/USDT": InstrumentOverrides(
        symbol="ETH/USDT",
        vwap=VWAPConfig(
            session_reset_hour=0,
        ),
        acceptance=AcceptanceConfig(
            time_bars=2,
            distance_atr_mult=1.3,  # ETH more volatile than BTC
            momentum_threshold=0.75,
            volume_mult=2.0,
        ),
        exhaustion=ExhaustionConfig(
            sd_min=2.5,
            volume_spike_mult=2.5,
            price_progress_threshold=0.3,
        ),
        risk=RiskConfig(
            base_risk_per_trade=Decimal("0.007"),
            max_risk_per_trade=Decimal("0.012"),
            max_daily_drawdown=Decimal("0.035"),
        ),
        trade_mgmt=TradeManagementConfig(
            trail_distance_atr=2.5,
            trail_step_atr=0.6,
        ),
    ),
    "SOL/USDT": InstrumentOverrides(
        symbol="SOL/USDT",
        acceptance=AcceptanceConfig(
            time_bars=2,
            distance_atr_mult=1.5,  # High volatility altcoin
            momentum_threshold=0.8,
            volume_mult=2.2,
        ),
        exhaustion=ExhaustionConfig(
            sd_min=3.0,  # Very high extremes for altcoins
            volume_spike_mult=3.0,
        ),
        risk=RiskConfig(
            base_risk_per_trade=Decimal("0.005"),  # Conservative for altcoins
            max_risk_per_trade=Decimal("0.01"),
            max_daily_drawdown=Decimal("0.03"),
        ),
    ),
}


def get_instrument_config(symbol: str, base_config: VWAPWaveConfig) -> VWAPWaveConfig:
    """
    Get configuration for an instrument with overrides applied.

    Args:
        symbol: Instrument symbol (e.g., "EUR/USD", "BTC/USDT")
        base_config: Base configuration to apply overrides to

    Returns:
        VWAPWaveConfig with instrument-specific overrides applied
    """
    # Check forex majors first
    overrides = FOREX_MAJORS.get(symbol)

    # Then check crypto pairs
    if overrides is None:
        overrides = CRYPTO_PAIRS.get(symbol)

    # If no overrides found, return base config
    if overrides is None:
        return base_config

    # Apply overrides
    return VWAPWaveConfig(
        vwap=overrides.vwap if overrides.vwap else base_config.vwap,
        ib=overrides.ib if overrides.ib else base_config.ib,
        acceptance=overrides.acceptance if overrides.acceptance else base_config.acceptance,
        exhaustion=overrides.exhaustion if overrides.exhaustion else base_config.exhaustion,
        cvd=base_config.cvd,  # CVD config rarely needs overrides
        volume_profile=base_config.volume_profile,
        risk=overrides.risk if overrides.risk else base_config.risk,
        trade_mgmt=overrides.trade_mgmt if overrides.trade_mgmt else base_config.trade_mgmt,
    )


# Correlation groups for risk management
CORRELATION_GROUPS: Dict[str, list] = {
    "USD_PAIRS": ["EUR/USD", "GBP/USD", "USD/JPY", "USD/CHF", "AUD/USD", "NZD/USD"],
    "EUR_CROSSES": ["EUR/GBP", "EUR/JPY", "EUR/CHF", "EUR/AUD"],
    "BTC_CORRELATED": ["BTC/USDT", "ETH/USDT", "SOL/USDT", "AVAX/USDT"],
    "STABLECOIN_PAIRS": ["BTC/USDT", "BTC/USDC", "ETH/USDT", "ETH/USDC"],
}


def get_correlation_group(symbol: str) -> Optional[str]:
    """Get the correlation group for an instrument."""
    for group_name, symbols in CORRELATION_GROUPS.items():
        if symbol in symbols:
            return group_name
    return None
