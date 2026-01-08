#!/usr/bin/env python3
"""
MNQ 3x + 이중SMA + GDX 전략 백테스트

MNQ 시뮬레이션: QQQ에 3x 레버리지 적용
- MNQ는 NQ(E-mini Nasdaq-100)의 마이크로 버전
- NQ ≈ QQQ 추종
- 3x 레버리지 = QQQ 가격 * 3배 노출

NautilusTrader 프레임워크 사용

사용법:
    python backtest_mnq3x.py
"""

from decimal import Decimal

import pandas as pd
import yfinance as yf

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig, LoggingConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import BarSpecification, BarType
from nautilus_trader.model.enums import AccountType, AggregationSource, BarAggregation, OmsType, PriceType
from nautilus_trader.model.identifiers import InstrumentId, Symbol, TraderId, Venue
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.objects import Money, Price, Quantity
from nautilus_trader.persistence.wranglers import BarDataWrangler

from mnq_dual_sma_strategy import MNQDualSMAConfig, MNQDualSMAStrategy


# =============================================================================
# Configuration
# =============================================================================

START_DATE = "2011-01-01"
END_DATE = "2024-12-31"
STARTING_CAPITAL = 52_500  # $52,500 (약 7천만원)
TARGET_LEVERAGE = 3.0      # MNQ 3x
REBALANCE_BAND_PCT = 0.15  # ±15%


# =============================================================================
# Data Loading
# =============================================================================

def download_data(symbol: str, start: str, end: str) -> pd.DataFrame:
    """Download daily OHLCV data from Yahoo Finance."""
    print(f"  {symbol} 다운로드 중...")
    df = yf.download(symbol, start=start, end=end, progress=False)

    if isinstance(df.columns, pd.MultiIndex):
        df.columns = df.columns.get_level_values(0)

    df = df.rename(columns={
        "Open": "open", "High": "high", "Low": "low",
        "Close": "close", "Volume": "volume",
    })
    df = df[["open", "high", "low", "close", "volume"]].copy()
    df["volume"] = df["volume"].fillna(0).astype(int)

    return df


def create_equity_instrument(symbol: str, venue: Venue) -> Equity:
    """Create an equity instrument."""
    return Equity(
        instrument_id=InstrumentId(Symbol(symbol), venue),
        raw_symbol=Symbol(symbol),
        currency=USD,
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )


# =============================================================================
# Backtest
# =============================================================================

def run_backtest():
    """Run the MNQ 3x backtest."""
    print("=" * 60)
    print("MNQ 3x + 이중SMA + GDX 백테스트")
    print("=" * 60)
    print(f"기간: {START_DATE} ~ {END_DATE}")
    print(f"시작 자본: ${STARTING_CAPITAL:,}")
    print(f"레버리지: {TARGET_LEVERAGE}x (MNQ 시뮬레이션)")
    print(f"리밸런싱 밴드: ±{REBALANCE_BAND_PCT*100:.0f}%")
    print("=" * 60)
    print("\n데이터 다운로드:")

    # Download data - QQQ for both signal AND long position (simulating MNQ)
    qqq_df = download_data("QQQ", START_DATE, END_DATE)
    gdx_df = download_data("GDX", START_DATE, END_DATE)

    print(f"\n  QQQ: {len(qqq_df)} bars")
    print(f"  GDX: {len(gdx_df)} bars")

    # Create venue
    NASDAQ = Venue("NASDAQ")

    # Create instruments
    # QQQ: 시그널용 AND MNQ 대용 (3x 레버리지 적용)
    qqq_instrument = create_equity_instrument("QQQ", NASDAQ)
    gdx_instrument = create_equity_instrument("GDX", NASDAQ)

    # Create bar types
    bar_spec = BarSpecification(
        step=1,
        aggregation=BarAggregation.DAY,
        price_type=PriceType.LAST,
    )

    qqq_bar_type = BarType(
        instrument_id=qqq_instrument.id,
        bar_spec=bar_spec,
        aggregation_source=AggregationSource.EXTERNAL,
    )

    gdx_bar_type = BarType(
        instrument_id=gdx_instrument.id,
        bar_spec=bar_spec,
        aggregation_source=AggregationSource.EXTERNAL,
    )

    # Convert data to bars
    print("\n바 데이터 변환 중...")
    qqq_wrangler = BarDataWrangler(qqq_bar_type, qqq_instrument)
    gdx_wrangler = BarDataWrangler(gdx_bar_type, gdx_instrument)

    qqq_bars = qqq_wrangler.process(qqq_df)
    gdx_bars = gdx_wrangler.process(gdx_df)

    # Configure backtest engine
    engine_config = BacktestEngineConfig(
        trader_id=TraderId("BACKTEST-001"),
        logging=LoggingConfig(log_level="WARNING"),
    )

    engine = BacktestEngine(config=engine_config)

    # Add venue with MARGIN account (allows leverage)
    engine.add_venue(
        venue=NASDAQ,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(STARTING_CAPITAL, USD)],
        base_currency=USD,
        default_leverage=Decimal(10),  # Allow up to 10x for margin
    )

    # Add instruments
    engine.add_instrument(qqq_instrument)
    engine.add_instrument(gdx_instrument)

    # Add data
    engine.add_data(qqq_bars)
    engine.add_data(gdx_bars)

    # Configure strategy
    # QQQ를 long instrument로 사용, 3x 레버리지 적용 = MNQ 3x 시뮬레이션
    strategy_config = MNQDualSMAConfig(
        strategy_id="MNQ_DUAL_SMA-001",
        qqq_instrument_id=qqq_instrument.id,
        long_instrument_id=qqq_instrument.id,  # QQQ with 3x leverage = MNQ 3x
        hedge_instrument_id=gdx_instrument.id,
        qqq_bar_type=qqq_bar_type,
        long_bar_type=qqq_bar_type,
        hedge_bar_type=gdx_bar_type,
        sma_long_period=200,
        sma_short_period=50,
        target_leverage=TARGET_LEVERAGE,  # 3x leverage on QQQ = MNQ 3x
        rebalance_band_pct=REBALANCE_BAND_PCT,
        close_positions_on_stop=True,
    )

    strategy = MNQDualSMAStrategy(config=strategy_config)
    engine.add_strategy(strategy)

    # Run backtest
    print("\n백테스트 실행 중...")
    engine.run()

    # Generate reports
    print("\n" + "=" * 60)
    print("백테스트 결과")
    print("=" * 60)

    # Account report
    account_report = engine.trader.generate_account_report(NASDAQ)

    # Fills report
    fills_report = engine.trader.generate_order_fills_report()
    print(f"\n총 체결: {len(fills_report)}건")

    # Performance metrics
    if len(account_report) > 0:
        final_balance = float(account_report.iloc[-1]["total"])
        total_return = (final_balance - STARTING_CAPITAL) / STARTING_CAPITAL * 100

        start_dt = pd.to_datetime(START_DATE)
        end_dt = pd.to_datetime(END_DATE)
        years = (end_dt - start_dt).days / 365.25

        if final_balance > 0 and years > 0:
            cagr = ((final_balance / STARTING_CAPITAL) ** (1 / years) - 1) * 100
        else:
            cagr = 0

        print(f"\n시작 자본:   ${STARTING_CAPITAL:,.0f}")
        print(f"최종 자본:   ${final_balance:,.0f}")
        print(f"총 수익률:   {total_return:+,.1f}%")
        print(f"CAGR:       {cagr:.1f}%")
        print(f"기간:        {years:.1f}년")

        # Calculate annualized trades
        trades_per_year = len(fills_report) / years
        print(f"연평균 체결: {trades_per_year:.1f}건")

    # Cleanup
    engine.reset()
    engine.dispose()

    print("\n" + "=" * 60)
    print("백테스트 완료!")
    print("=" * 60)


if __name__ == "__main__":
    run_backtest()
