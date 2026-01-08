#!/usr/bin/env python3
"""
FINAL Backtest with Real Options Data
실제 옵션 데이터 기반 최종 백테스트

Key Insight from analysis:
- Strangle 승률 86%, 평균 수익 1.48%/월
- 2020년 3월 -75% 손실 (VIX 80+)
- VIX > 30 일 때 스트랭글 회피 필요
"""

import pandas as pd
import numpy as np
import yfinance as yf
from datetime import timedelta
import warnings
warnings.filterwarnings('ignore')

# Configuration
START_DATE = "2020-01-01"
END_DATE = "2022-12-31"
STARTING_CAPITAL = 100_000
SMA_PERIOD = 200
DISTANCE_THRESHOLD = 0.05

# Real strangle returns from analysis (monthly)
STRANGLE_RETURNS = {
    '2020-01': 2.7, '2020-02': 4.2, '2020-03': -75.6, '2020-04': -4.3,
    '2020-05': 8.7, '2020-06': 6.2, '2020-07': 5.9, '2020-08': 5.0,
    '2020-09': 5.7, '2020-10': 5.8, '2020-11': -0.3, '2020-12': 4.6,
    '2021-01': 5.4, '2021-02': 5.9, '2021-03': 5.1, '2021-04': 3.6,
    '2021-05': 3.8, '2021-06': 3.7, '2021-07': 3.3, '2021-08': 4.0,
    '2021-09': 3.3, '2021-10': 4.4, '2021-11': 3.3, '2021-12': 6.7,
    '2022-01': -11.3, '2022-02': 4.0, '2022-03': 7.1, '2022-04': 4.6,
    '2022-05': 7.2, '2022-06': -8.8, '2022-07': 6.4, '2022-08': 5.0,
    '2022-09': 5.4, '2022-10': 6.3, '2022-11': 5.7, '2022-12': 0.7,
}


def load_market_data():
    """Load market data."""
    print("Loading market data...")
    spy = yf.download("SPY", start="2019-01-01", end=END_DATE, progress=False)['Close']
    tqqq = yf.download("TQQQ", start="2019-01-01", end=END_DATE, progress=False)['Close']
    vix = yf.download("^VIX", start="2019-01-01", end=END_DATE, progress=False)['Close']

    if hasattr(spy, 'columns'): spy = spy.iloc[:, 0]
    if hasattr(tqqq, 'columns'): tqqq = tqqq.iloc[:, 0]
    if hasattr(vix, 'columns'): vix = vix.iloc[:, 0]

    market = pd.DataFrame({'spy': spy, 'tqqq': tqqq, 'vix': vix}).dropna()
    market['sma200'] = market['spy'].rolling(SMA_PERIOD).mean()
    return market


def run_tqqq_sma200(market_df):
    """Simple TQQQ SMA200."""
    market = market_df.loc[START_DATE:END_DATE].copy().dropna()
    balance = STARTING_CAPITAL
    position = 0
    entry_price = 0
    trades = 0
    history = []

    for date, row in market.iterrows():
        spy_price = row['spy']
        tqqq_price = row['tqqq']
        sma = row['sma200']
        if pd.isna(sma): continue

        above_sma = spy_price >= sma

        if above_sma and position == 0:
            shares = int((balance * 0.95) / tqqq_price)
            if shares > 0:
                balance -= shares * tqqq_price * 1.001
                position = shares
                entry_price = tqqq_price
                trades += 1
        elif not above_sma and position > 0:
            balance += position * tqqq_price * 0.999
            position = 0
            trades += 1
        elif position > 0 and (tqqq_price - entry_price) / entry_price <= -0.15:
            balance += position * tqqq_price * 0.999
            position = 0
            trades += 1

        history.append({'date': date, 'value': balance + position * tqqq_price})

    return balance + position * market.iloc[-1]['tqqq'], trades, pd.DataFrame(history)


def run_hybrid_real(market_df, use_vix_filter=True):
    """Hybrid strategy with REAL strangle returns."""
    market = market_df.loc[START_DATE:END_DATE].copy().dropna()
    balance = STARTING_CAPITAL
    position = 0
    entry_price = 0
    trades = 0
    current_regime = "UNKNOWN"
    last_month = None
    strangle_profit = 0
    history = []

    for date, row in market.iterrows():
        spy_price = row['spy']
        tqqq_price = row['tqqq']
        sma = row['sma200']
        vix = row['vix']
        if pd.isna(sma): continue

        distance_pct = (spy_price - sma) / sma

        # Determine regime
        if spy_price < sma:
            new_regime = "STRANGLE"
        elif distance_pct > DISTANCE_THRESHOLD:
            new_regime = "PMCC"
        else:
            new_regime = "TQQQ"

        # Monthly strangle return (REAL DATA)
        current_month = date.strftime('%Y-%m')
        if current_regime == "STRANGLE" and current_month != last_month:
            if current_month in STRANGLE_RETURNS:
                monthly_return = STRANGLE_RETURNS[current_month] / 100

                # VIX Filter: Skip strangle if VIX > 35 (너무 위험)
                if use_vix_filter and vix > 35:
                    monthly_return = 0  # Stay cash instead

                margin_capital = balance * 0.20  # 20% margin
                profit = margin_capital * monthly_return
                balance += profit
                strangle_profit += profit

            last_month = current_month

        # PMCC monthly premium (simulated 0.8%/month)
        if current_regime == "PMCC" and current_month != last_month:
            balance *= 1.008
            last_month = current_month

        # Regime change
        if new_regime != current_regime:
            if position > 0:
                balance += position * tqqq_price * 0.999
                position = 0
                trades += 1

            if new_regime == "TQQQ":
                shares = int((balance * 0.95) / tqqq_price)
                if shares > 0:
                    balance -= shares * tqqq_price * 1.001
                    position = shares
                    entry_price = tqqq_price
                    trades += 1
            elif new_regime == "PMCC":
                shares = int((balance * 0.70) / tqqq_price)
                if shares > 0:
                    balance -= shares * tqqq_price * 1.001
                    position = shares
                    entry_price = tqqq_price
                    trades += 1

            current_regime = new_regime

        # Stop loss for TQQQ
        if current_regime == "TQQQ" and position > 0:
            if (tqqq_price - entry_price) / entry_price <= -0.15:
                balance += position * tqqq_price * 0.999
                position = 0
                trades += 1

        history.append({'date': date, 'value': balance + position * tqqq_price, 'regime': current_regime, 'vix': vix})

    final_value = balance + position * market.iloc[-1]['tqqq']
    return final_value, trades, pd.DataFrame(history), strangle_profit


def calculate_metrics(final_value, history_df):
    """Calculate metrics."""
    years = (pd.to_datetime(END_DATE) - pd.to_datetime(START_DATE)).days / 365.25
    total_return = (final_value - STARTING_CAPITAL) / STARTING_CAPITAL * 100
    cagr = ((final_value / STARTING_CAPITAL) ** (1 / years) - 1) * 100
    history_df['peak'] = history_df['value'].cummax()
    history_df['drawdown'] = (history_df['value'] - history_df['peak']) / history_df['peak']
    max_dd = history_df['drawdown'].min() * 100
    return {'total_return': total_return, 'cagr': cagr, 'max_drawdown': max_dd}


def main():
    print("="*70)
    print("FINAL BACKTEST: Real Options Data (2020-2022)")
    print("="*70)

    market = load_market_data()

    # Run backtests
    print("\nRunning backtests...")

    sma_value, sma_trades, sma_history = run_tqqq_sma200(market)
    sma_metrics = calculate_metrics(sma_value, sma_history)

    hybrid_value, hybrid_trades, hybrid_history, strangle_profit = run_hybrid_real(market, use_vix_filter=False)
    hybrid_metrics = calculate_metrics(hybrid_value, hybrid_history)

    hybrid_vix_value, hybrid_vix_trades, hybrid_vix_history, strangle_vix_profit = run_hybrid_real(market, use_vix_filter=True)
    hybrid_vix_metrics = calculate_metrics(hybrid_vix_value, hybrid_vix_history)

    # Results
    print("\n" + "="*70)
    print("RESULTS (2020-2022, 실제 옵션 수익률 사용)")
    print("="*70)

    print(f"\n{'Strategy':<30} {'Final':>12} {'Return':>10} {'CAGR':>8} {'MaxDD':>8}")
    print("-"*70)
    print(f"{'TQQQ SMA200':<30} ${sma_value:>10,.0f} {sma_metrics['total_return']:>8.1f}% {sma_metrics['cagr']:>6.1f}% {sma_metrics['max_drawdown']:>6.1f}%")
    print(f"{'Hybrid (No VIX Filter)':<30} ${hybrid_value:>10,.0f} {hybrid_metrics['total_return']:>8.1f}% {hybrid_metrics['cagr']:>6.1f}% {hybrid_metrics['max_drawdown']:>6.1f}%")
    print(f"{'Hybrid (VIX>35 Filter)':<30} ${hybrid_vix_value:>10,.0f} {hybrid_vix_metrics['total_return']:>8.1f}% {hybrid_vix_metrics['cagr']:>6.1f}% {hybrid_vix_metrics['max_drawdown']:>6.1f}%")
    print("-"*70)

    print(f"\nStrangle Profit (No Filter): ${strangle_profit:,.0f}")
    print(f"Strangle Profit (VIX Filter): ${strangle_vix_profit:,.0f}")

    # 2022 Analysis
    print("\n" + "="*70)
    print("2022 BEAR MARKET PERFORMANCE")
    print("="*70)

    for name, hist in [("SMA200", sma_history), ("Hybrid", hybrid_history), ("Hybrid+VIX", hybrid_vix_history)]:
        h2022 = hist[hist['date'] >= '2022-01-01']
        if len(h2022) > 0:
            ret = (h2022.iloc[-1]['value'] / h2022.iloc[0]['value'] - 1) * 100
            print(f"{name}: {ret:+.1f}%")

    # Regime time
    print("\n" + "="*70)
    print("REGIME TIME ALLOCATION")
    print("="*70)
    regime_counts = hybrid_history['regime'].value_counts()
    for r, c in regime_counts.items():
        print(f"  {r}: {c} days ({c/len(hybrid_history)*100:.1f}%)")

    # Winner
    print("\n" + "="*70)
    winner = max([
        ("TQQQ SMA200", sma_value),
        ("Hybrid (No Filter)", hybrid_value),
        ("Hybrid (VIX Filter)", hybrid_vix_value)
    ], key=lambda x: x[1])
    print(f"WINNER: {winner[0]} (${winner[1]:,.0f})")
    print("="*70)


if __name__ == "__main__":
    main()
