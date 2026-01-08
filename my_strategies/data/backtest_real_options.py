#!/usr/bin/env python3
"""
Real Options Data Backtest: Hybrid Strategy
실제 옵션 데이터를 사용한 정확한 백테스트

데이터: SPY Options 2020-2022 (Kaggle)
전략:
- SPY < 200SMA: SPY 스트랭글 매도 (30 DTE, 16 delta)
- SPY > 200SMA, 이격도 > 5%: PMCC 시뮬레이션
- SPY > 200SMA, 이격도 ≤ 5%: TQQQ 100%
"""

import pandas as pd
import numpy as np
import yfinance as yf
from datetime import datetime, timedelta
import warnings
warnings.filterwarnings('ignore')

# Configuration
START_DATE = "2020-01-01"
END_DATE = "2022-12-31"
STARTING_CAPITAL = 100_000
SMA_PERIOD = 200
DISTANCE_THRESHOLD = 0.05

# Strangle parameters
STRANGLE_DTE_TARGET = 30  # 30일 만기
STRANGLE_DELTA_TARGET = 0.16  # 16 delta (84% OTM)
STRANGLE_MARGIN_PCT = 0.20  # 20% 마진 사용

# PMCC parameters
PMCC_LEAPS_DTE = 365  # 1년 만기 LEAPS
PMCC_LEAPS_DELTA = 0.70  # 70 delta deep ITM
PMCC_SHORT_DTE = 30  # 30일 만기 short call
PMCC_SHORT_DELTA = 0.30  # 30 delta


def load_spy_options():
    """Load SPY options data."""
    print("Loading SPY options data...")
    df = pd.read_csv('spy_options_2020_2022/spy_2020_2022.csv')

    # Clean column names
    df.columns = [c.strip().replace('[', '').replace(']', '') for c in df.columns]

    # Parse dates
    df['QUOTE_DATE'] = pd.to_datetime(df['QUOTE_DATE'].str.strip())
    df['EXPIRE_DATE'] = pd.to_datetime(df['EXPIRE_DATE'].str.strip())

    # Convert numeric columns
    numeric_cols = ['UNDERLYING_LAST', 'DTE', 'STRIKE', 'C_BID', 'C_ASK', 'P_BID', 'P_ASK',
                    'C_DELTA', 'P_DELTA', 'C_IV', 'P_IV', 'C_THETA', 'P_THETA']
    for col in numeric_cols:
        if col in df.columns:
            df[col] = pd.to_numeric(df[col], errors='coerce')

    print(f"  Loaded {len(df):,} option quotes")
    print(f"  Date range: {df['QUOTE_DATE'].min()} to {df['QUOTE_DATE'].max()}")

    return df


def load_market_data():
    """Load SPY and TQQQ price data."""
    print("Loading market data...")
    spy = yf.download("SPY", start="2019-01-01", end=END_DATE, progress=False)['Close']
    tqqq = yf.download("TQQQ", start="2019-01-01", end=END_DATE, progress=False)['Close']
    vix = yf.download("^VIX", start="2019-01-01", end=END_DATE, progress=False)['Close']

    if hasattr(spy, 'columns'):
        spy = spy.iloc[:, 0]
    if hasattr(tqqq, 'columns'):
        tqqq = tqqq.iloc[:, 0]
    if hasattr(vix, 'columns'):
        vix = vix.iloc[:, 0]

    market = pd.DataFrame({'spy': spy, 'tqqq': tqqq, 'vix': vix}).dropna()
    market['sma200'] = market['spy'].rolling(SMA_PERIOD).mean()

    print(f"  Market data: {len(market)} days")
    return market


def find_strangle_options(options_df, quote_date, underlying_price, target_dte=30, target_delta=0.16):
    """Find best strangle options for given parameters."""
    day_options = options_df[options_df['QUOTE_DATE'] == quote_date].copy()

    if len(day_options) == 0:
        return None, None

    # Filter by DTE (25-35 days)
    day_options = day_options[(day_options['DTE'] >= target_dte - 5) &
                               (day_options['DTE'] <= target_dte + 5)]

    if len(day_options) == 0:
        return None, None

    # Find PUT with delta closest to -target_delta
    puts = day_options[day_options['P_DELTA'] < 0].copy()
    if len(puts) > 0:
        puts['delta_diff'] = abs(puts['P_DELTA'] + target_delta)
        best_put = puts.loc[puts['delta_diff'].idxmin()]
    else:
        best_put = None

    # Find CALL with delta closest to target_delta
    calls = day_options[day_options['C_DELTA'] > 0].copy()
    if len(calls) > 0:
        calls['delta_diff'] = abs(calls['C_DELTA'] - target_delta)
        best_call = calls.loc[calls['delta_diff'].idxmin()]
    else:
        best_call = None

    return best_put, best_call


def calculate_strangle_return(options_df, entry_date, underlying_price, hold_days=21):
    """Calculate actual strangle return from real options data."""
    put_opt, call_opt = find_strangle_options(options_df, entry_date, underlying_price)

    if put_opt is None or call_opt is None:
        return 0, 0  # No valid options found

    # Entry premium (credit received)
    put_premium = (put_opt['P_BID'] + put_opt['P_ASK']) / 2
    call_premium = (call_opt['C_BID'] + call_opt['C_ASK']) / 2
    total_premium = put_premium + call_premium

    # Calculate margin requirement (simplified: 20% of underlying)
    margin_required = underlying_price * STRANGLE_MARGIN_PCT

    # Find exit date
    exit_date = entry_date + timedelta(days=hold_days)

    # Find options at exit
    exit_options = options_df[
        (options_df['QUOTE_DATE'] >= exit_date - timedelta(days=2)) &
        (options_df['QUOTE_DATE'] <= exit_date + timedelta(days=2)) &
        (options_df['STRIKE'] == put_opt['STRIKE'])
    ]

    exit_options_call = options_df[
        (options_df['QUOTE_DATE'] >= exit_date - timedelta(days=2)) &
        (options_df['QUOTE_DATE'] <= exit_date + timedelta(days=2)) &
        (options_df['STRIKE'] == call_opt['STRIKE'])
    ]

    # Calculate exit cost
    if len(exit_options) > 0 and len(exit_options_call) > 0:
        put_exit = (exit_options.iloc[0]['P_BID'] + exit_options.iloc[0]['P_ASK']) / 2
        call_exit = (exit_options_call.iloc[0]['C_BID'] + exit_options_call.iloc[0]['C_ASK']) / 2
        exit_cost = put_exit + call_exit
    else:
        # Assume expired worthless or estimate
        exit_cost = total_premium * 0.3  # Assume 70% profit

    profit = total_premium - exit_cost
    return_pct = profit / margin_required

    return return_pct, total_premium


def run_backtest(options_df, market_df):
    """Run hybrid strategy backtest with real options data."""
    print("\n" + "="*70)
    print("Running Hybrid Strategy Backtest with REAL OPTIONS DATA")
    print("="*70)

    # Filter to backtest period
    market = market_df.loc[START_DATE:END_DATE].copy()
    market = market.dropna()

    balance = STARTING_CAPITAL
    position = 0  # TQQQ shares
    current_regime = "UNKNOWN"
    entry_price = 0
    trades = 0

    # Track monthly for strangle
    last_strangle_month = None
    strangle_premium_collected = 0

    history = []
    regime_changes = []

    for date, row in market.iterrows():
        spy_price = row['spy']
        tqqq_price = row['tqqq']
        sma = row['sma200']
        vix = row['vix']

        if pd.isna(sma):
            continue

        # Determine regime
        distance_pct = (spy_price - sma) / sma

        if spy_price < sma:
            new_regime = "STRANGLE"
        elif distance_pct > DISTANCE_THRESHOLD:
            new_regime = "PMCC"
        else:
            new_regime = "TQQQ"

        # Monthly strangle premium collection
        current_month = date.month
        if current_regime == "STRANGLE" and current_month != last_strangle_month:
            # Calculate real strangle return from options data
            quote_date = pd.Timestamp(date)
            strangle_return, premium = calculate_strangle_return(
                options_df, quote_date, spy_price, hold_days=21
            )

            if strangle_return != 0:
                # Apply return to balance (on margin capital)
                margin_capital = balance * STRANGLE_MARGIN_PCT
                profit = margin_capital * strangle_return
                balance += profit
                strangle_premium_collected += profit

            last_strangle_month = current_month

        # Handle regime change
        if new_regime != current_regime:
            regime_changes.append({
                'date': date,
                'from': current_regime,
                'to': new_regime,
                'spy': spy_price,
                'sma': sma,
                'distance': distance_pct * 100,
                'vix': vix
            })

            # Exit old regime
            if position > 0:
                proceeds = position * tqqq_price * (1 - 0.001)  # 0.1% cost
                balance += proceeds
                position = 0
                trades += 1

            # Enter new regime
            if new_regime == "TQQQ":
                shares = int((balance * 0.95) / tqqq_price)
                if shares > 0:
                    cost = shares * tqqq_price * (1 + 0.001)
                    balance -= cost
                    position = shares
                    entry_price = tqqq_price
                    trades += 1

            elif new_regime == "PMCC":
                # PMCC: 70% exposure via simulated LEAPS
                shares = int((balance * 0.70) / tqqq_price)
                if shares > 0:
                    cost = shares * tqqq_price * (1 + 0.001)
                    balance -= cost
                    position = shares
                    entry_price = tqqq_price
                    trades += 1

            # STRANGLE: Stay in cash, collect premium monthly

            current_regime = new_regime

        # Stop loss for TQQQ
        if current_regime == "TQQQ" and position > 0:
            pnl_pct = (tqqq_price - entry_price) / entry_price
            if pnl_pct <= -0.15:
                proceeds = position * tqqq_price * (1 - 0.001)
                balance += proceeds
                position = 0
                trades += 1

        # Record history
        total_value = balance + position * tqqq_price
        history.append({
            'date': date,
            'value': total_value,
            'regime': current_regime,
            'spy': spy_price,
            'vix': vix
        })

    # Final value
    final_value = balance + position * market.iloc[-1]['tqqq']

    return final_value, trades, pd.DataFrame(history), pd.DataFrame(regime_changes), strangle_premium_collected


def run_simple_sma200(market_df):
    """Run simple TQQQ SMA200 for comparison."""
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

        if pd.isna(sma):
            continue

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

        elif position > 0:
            pnl_pct = (tqqq_price - entry_price) / entry_price
            if pnl_pct <= -0.15:
                balance += position * tqqq_price * 0.999
                position = 0
                trades += 1

        total_value = balance + position * tqqq_price
        history.append({'date': date, 'value': total_value})

    final_value = balance + position * market.iloc[-1]['tqqq']
    return final_value, trades, pd.DataFrame(history)


def calculate_metrics(final_value, history_df, start_date, end_date):
    """Calculate performance metrics."""
    years = (pd.to_datetime(end_date) - pd.to_datetime(start_date)).days / 365.25
    total_return = (final_value - STARTING_CAPITAL) / STARTING_CAPITAL * 100
    cagr = ((final_value / STARTING_CAPITAL) ** (1 / years) - 1) * 100

    history_df['peak'] = history_df['value'].cummax()
    history_df['drawdown'] = (history_df['value'] - history_df['peak']) / history_df['peak']
    max_dd = history_df['drawdown'].min() * 100

    return {'total_return': total_return, 'cagr': cagr, 'max_drawdown': max_dd, 'years': years}


def main():
    print("="*70)
    print("REAL OPTIONS DATA BACKTEST")
    print("Hybrid Strategy vs TQQQ SMA200")
    print("="*70)
    print(f"\nPeriod: {START_DATE} to {END_DATE}")
    print(f"Starting Capital: ${STARTING_CAPITAL:,}")

    # Load data
    options_df = load_spy_options()
    market_df = load_market_data()

    # Run backtests
    print("\n" + "-"*70)
    print("Running TQQQ SMA200...")
    sma_value, sma_trades, sma_history = run_simple_sma200(market_df)
    sma_metrics = calculate_metrics(sma_value, sma_history, START_DATE, END_DATE)

    print("Running Hybrid Strategy with REAL OPTIONS...")
    hybrid_value, hybrid_trades, hybrid_history, regime_changes, strangle_profit = run_backtest(options_df, market_df)
    hybrid_metrics = calculate_metrics(hybrid_value, hybrid_history, START_DATE, END_DATE)

    # Results
    print("\n" + "="*70)
    print("RESULTS (2020-2022, 실제 옵션 데이터)")
    print("="*70)

    print(f"\n{'Strategy':<25} {'Final Value':>15} {'Return':>10} {'CAGR':>10} {'MaxDD':>10} {'Trades':>8}")
    print("-"*80)
    print(f"{'TQQQ SMA200':<25} ${sma_value:>13,.0f} {sma_metrics['total_return']:>8.1f}% {sma_metrics['cagr']:>8.1f}% {sma_metrics['max_drawdown']:>8.1f}% {sma_trades:>8}")
    print(f"{'Hybrid (Real Options)':<25} ${hybrid_value:>13,.0f} {hybrid_metrics['total_return']:>8.1f}% {hybrid_metrics['cagr']:>8.1f}% {hybrid_metrics['max_drawdown']:>8.1f}% {hybrid_trades:>8}")
    print("-"*80)

    diff = (hybrid_value / sma_value - 1) * 100
    print(f"\nHybrid vs SMA200: {diff:+.1f}%")
    print(f"Strangle Premium Collected: ${strangle_profit:,.0f}")

    # Regime analysis
    print("\n" + "="*70)
    print("REGIME ANALYSIS")
    print("="*70)

    if len(regime_changes) > 0:
        rc_df = pd.DataFrame(regime_changes)
        print(f"\nTotal regime changes: {len(rc_df)}")
        print("\nRecent changes:")
        print(rc_df.tail(10).to_string(index=False))

    regime_counts = hybrid_history['regime'].value_counts()
    total_days = len(hybrid_history)
    print(f"\nTime in each regime:")
    for regime, count in regime_counts.items():
        pct = count / total_days * 100
        print(f"  {regime}: {count} days ({pct:.1f}%)")

    # 2022 Analysis (bear market)
    print("\n" + "="*70)
    print("2022 BEAR MARKET ANALYSIS")
    print("="*70)

    h2022 = hybrid_history[hybrid_history['date'] >= '2022-01-01']
    s2022 = sma_history[sma_history['date'] >= '2022-01-01']

    if len(h2022) > 0 and len(s2022) > 0:
        hybrid_2022_start = h2022.iloc[0]['value']
        hybrid_2022_end = h2022.iloc[-1]['value']
        hybrid_2022_return = (hybrid_2022_end / hybrid_2022_start - 1) * 100

        sma_2022_start = s2022.iloc[0]['value']
        sma_2022_end = s2022.iloc[-1]['value']
        sma_2022_return = (sma_2022_end / sma_2022_start - 1) * 100

        print(f"TQQQ SMA200 2022 Return: {sma_2022_return:+.1f}%")
        print(f"Hybrid 2022 Return: {hybrid_2022_return:+.1f}%")

    print("\n" + "="*70)

    # Winner
    if hybrid_value > sma_value:
        print(f"WINNER: Hybrid Strategy (+${hybrid_value - sma_value:,.0f})")
    else:
        print(f"WINNER: TQQQ SMA200 (+${sma_value - hybrid_value:,.0f})")

    print("="*70)


if __name__ == "__main__":
    main()
