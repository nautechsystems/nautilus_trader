#!/usr/bin/env python3
"""
Wheel Strategy Backtest on SPY (Real Options Data)
SPY 주식 옵션 Wheel 전략 백테스트

Strategy:
1. Sell OTM Cash-Secured Puts (5% OTM, 30 DTE)
2. If assigned: Hold 100 shares of SPY
3. Sell OTM Covered Calls (5% OTM, 30 DTE)
4. If called away: Go back to step 1
"""

import pandas as pd
import numpy as np
from datetime import timedelta
import warnings
warnings.filterwarnings('ignore')

# Configuration
STARTING_CAPITAL = 100_000
PUT_OTM_PCT = 0.05      # 5% OTM for puts
CALL_OTM_PCT = 0.05     # 5% OTM for calls
TARGET_DTE = 30         # 30 days to expiration
COMMISSION = 1.00       # $1 per contract


def load_spy_options():
    """Load SPY options data."""
    print("Loading SPY options data...")

    # Try the 2020-2022 dataset
    try:
        df = pd.read_csv('spy_options_2020_2022/spy_2020_2022.csv')
    except:
        # Try alternative path
        df = pd.read_csv('spy_2020_2022.csv')

    # Clean column names
    df.columns = [c.strip().replace('[', '').replace(']', '') for c in df.columns]

    # Parse dates
    df['QUOTE_DATE'] = pd.to_datetime(df['QUOTE_DATE'].str.strip())
    df['EXPIRE_DATE'] = pd.to_datetime(df['EXPIRE_DATE'].str.strip())

    # Convert numeric columns
    numeric_cols = ['UNDERLYING_LAST', 'DTE', 'STRIKE',
                    'C_BID', 'C_ASK', 'C_DELTA',
                    'P_BID', 'P_ASK', 'P_DELTA']
    for col in numeric_cols:
        if col in df.columns:
            df[col] = pd.to_numeric(df[col], errors='coerce')

    print(f"  Loaded {len(df):,} rows")
    print(f"  Date range: {df['QUOTE_DATE'].min().date()} to {df['QUOTE_DATE'].max().date()}")

    return df


def find_put_option(df, date, underlying, otm_pct=0.05, target_dte=30):
    """Find OTM put to sell."""
    target_strike = underlying * (1 - otm_pct)

    day_opts = df[(df['QUOTE_DATE'] == date) &
                   (df['DTE'] >= target_dte - 7) &
                   (df['DTE'] <= target_dte + 7) &
                   (df['STRIKE'] < underlying) &
                   (df['P_BID'] > 0)].copy()

    if len(day_opts) == 0:
        return None

    day_opts['strike_diff'] = abs(day_opts['STRIKE'] - target_strike)
    best = day_opts.loc[day_opts['strike_diff'].idxmin()]

    return {
        'type': 'PUT',
        'date': date,
        'underlying': underlying,
        'strike': best['STRIKE'],
        'premium': (best['P_BID'] + best['P_ASK']) / 2 if best['P_ASK'] > 0 else best['P_BID'],
        'bid': best['P_BID'],
        'dte': best['DTE'],
        'expire_date': best['EXPIRE_DATE'],
        'delta': best['P_DELTA'] if pd.notna(best['P_DELTA']) else -0.16
    }


def find_call_option(df, date, underlying, otm_pct=0.05, target_dte=30):
    """Find OTM call to sell."""
    target_strike = underlying * (1 + otm_pct)

    day_opts = df[(df['QUOTE_DATE'] == date) &
                   (df['DTE'] >= target_dte - 7) &
                   (df['DTE'] <= target_dte + 7) &
                   (df['STRIKE'] > underlying) &
                   (df['C_BID'] > 0)].copy()

    if len(day_opts) == 0:
        return None

    day_opts['strike_diff'] = abs(day_opts['STRIKE'] - target_strike)
    best = day_opts.loc[day_opts['strike_diff'].idxmin()]

    return {
        'type': 'CALL',
        'date': date,
        'underlying': underlying,
        'strike': best['STRIKE'],
        'premium': (best['C_BID'] + best['C_ASK']) / 2 if best['C_ASK'] > 0 else best['C_BID'],
        'bid': best['C_BID'],
        'dte': best['DTE'],
        'expire_date': best['EXPIRE_DATE'],
        'delta': best['C_DELTA'] if pd.notna(best['C_DELTA']) else 0.16
    }


def get_underlying_at_date(df, date):
    """Get underlying price at a specific date."""
    day_data = df[df['QUOTE_DATE'] == date]
    if len(day_data) > 0:
        return day_data.iloc[0]['UNDERLYING_LAST']

    # Try nearby dates
    for delta in range(1, 5):
        day_data = df[df['QUOTE_DATE'] == date + timedelta(days=delta)]
        if len(day_data) > 0:
            return day_data.iloc[0]['UNDERLYING_LAST']
        day_data = df[df['QUOTE_DATE'] == date - timedelta(days=delta)]
        if len(day_data) > 0:
            return day_data.iloc[0]['UNDERLYING_LAST']

    return None


def run_wheel_backtest(df):
    """Run Wheel strategy backtest."""
    print("\nRunning Wheel strategy backtest...")
    print("="*70)

    dates = sorted(df['QUOTE_DATE'].unique())

    # State
    cash = STARTING_CAPITAL
    shares = 0
    share_cost_basis = 0
    current_option = None

    trades = []
    history = []

    for date in dates:
        day_data = df[df['QUOTE_DATE'] == date]
        if len(day_data) == 0:
            continue

        underlying = day_data.iloc[0]['UNDERLYING_LAST']

        # Check if current option expired
        if current_option is not None and date >= current_option['expire_date']:
            price_at_expire = get_underlying_at_date(df, current_option['expire_date'])
            if price_at_expire is None:
                price_at_expire = underlying

            if current_option['type'] == 'PUT':
                # Put expiration
                if price_at_expire < current_option['strike']:
                    # Assigned - buy 100 shares at strike
                    cost = current_option['strike'] * 100
                    if cash >= cost:
                        cash -= cost
                        shares = 100
                        share_cost_basis = current_option['strike']
                        trades.append({
                            'date': current_option['expire_date'],
                            'type': 'PUT_ASSIGNED',
                            'strike': current_option['strike'],
                            'underlying': price_at_expire,
                            'pnl': current_option['premium'] * 100 - COMMISSION,
                            'note': f'Bought 100 shares at ${current_option["strike"]:.2f}'
                        })
                    else:
                        # Not enough cash - just take the loss
                        loss = (current_option['strike'] - price_at_expire) * 100
                        cash -= loss
                        trades.append({
                            'date': current_option['expire_date'],
                            'type': 'PUT_CASH_SETTLED',
                            'strike': current_option['strike'],
                            'underlying': price_at_expire,
                            'pnl': current_option['premium'] * 100 - loss - COMMISSION,
                        })
                else:
                    # Expired worthless - keep premium
                    trades.append({
                        'date': current_option['expire_date'],
                        'type': 'PUT_EXPIRED',
                        'strike': current_option['strike'],
                        'underlying': price_at_expire,
                        'pnl': current_option['premium'] * 100 - COMMISSION,
                    })

            elif current_option['type'] == 'CALL':
                # Call expiration
                if price_at_expire > current_option['strike']:
                    # Called away - sell shares at strike
                    proceeds = current_option['strike'] * 100
                    cash += proceeds
                    stock_pnl = (current_option['strike'] - share_cost_basis) * 100
                    trades.append({
                        'date': current_option['expire_date'],
                        'type': 'CALL_ASSIGNED',
                        'strike': current_option['strike'],
                        'underlying': price_at_expire,
                        'pnl': current_option['premium'] * 100 + stock_pnl - COMMISSION,
                        'note': f'Sold 100 shares at ${current_option["strike"]:.2f}'
                    })
                    shares = 0
                    share_cost_basis = 0
                else:
                    # Expired worthless - keep shares and premium
                    trades.append({
                        'date': current_option['expire_date'],
                        'type': 'CALL_EXPIRED',
                        'strike': current_option['strike'],
                        'underlying': price_at_expire,
                        'pnl': current_option['premium'] * 100 - COMMISSION,
                    })

            current_option = None

        # Open new position if no current option
        if current_option is None:
            if shares == 0:
                # No shares - sell put
                max_strike = cash / 100  # Max strike we can afford
                put = find_put_option(df, date, underlying, PUT_OTM_PCT, TARGET_DTE)

                if put is not None and put['strike'] <= max_strike:
                    current_option = put
                    premium_received = put['premium'] * 100 - COMMISSION
                    cash += premium_received
                    trades.append({
                        'date': date,
                        'type': 'SELL_PUT',
                        'strike': put['strike'],
                        'premium': put['premium'],
                        'dte': put['dte'],
                        'underlying': underlying,
                        'delta': put['delta']
                    })
            else:
                # Have shares - sell call
                call = find_call_option(df, date, underlying, CALL_OTM_PCT, TARGET_DTE)

                if call is not None:
                    current_option = call
                    premium_received = call['premium'] * 100 - COMMISSION
                    cash += premium_received
                    trades.append({
                        'date': date,
                        'type': 'SELL_CALL',
                        'strike': call['strike'],
                        'premium': call['premium'],
                        'dte': call['dte'],
                        'underlying': underlying,
                        'delta': call['delta']
                    })

        # Calculate total value
        shares_value = shares * underlying

        # Account for option liability if ITM
        option_liability = 0
        if current_option is not None:
            if current_option['type'] == 'PUT':
                intrinsic = max(0, current_option['strike'] - underlying)
                option_liability = intrinsic * 100
            elif current_option['type'] == 'CALL':
                intrinsic = max(0, underlying - current_option['strike'])
                option_liability = intrinsic * 100

        total_value = cash + shares_value - option_liability

        history.append({
            'date': date,
            'cash': cash,
            'shares': shares,
            'shares_value': shares_value,
            'total_value': total_value,
            'underlying': underlying,
            'has_put': current_option is not None and current_option['type'] == 'PUT',
            'has_call': current_option is not None and current_option['type'] == 'CALL',
        })

    return trades, pd.DataFrame(history)


def analyze_results(trades, history):
    """Analyze backtest results."""
    print("\n" + "="*70)
    print("WHEEL STRATEGY BACKTEST RESULTS (SPY 2020-2022)")
    print("="*70)

    trades_df = pd.DataFrame(trades)

    # Basic stats
    initial = STARTING_CAPITAL
    final = history.iloc[-1]['total_value']
    years = (history.iloc[-1]['date'] - history.iloc[0]['date']).days / 365.25
    total_return = (final - initial) / initial * 100
    cagr = ((final / initial) ** (1/years) - 1) * 100

    # Drawdown
    history['peak'] = history['total_value'].cummax()
    history['drawdown'] = (history['total_value'] - history['peak']) / history['peak']
    max_dd = history['drawdown'].min() * 100

    print(f"\n{'Metric':<25} {'Value':>15}")
    print("-"*42)
    print(f"{'Starting Capital':<25} ${initial:>14,}")
    print(f"{'Final Value':<25} ${final:>14,.0f}")
    print(f"{'Total Return':<25} {total_return:>14.1f}%")
    print(f"{'CAGR':<25} {cagr:>14.1f}%")
    print(f"{'Max Drawdown':<25} {max_dd:>14.1f}%")
    print(f"{'Period':<25} {years:>14.1f} years")

    # Trade statistics
    print("\n" + "-"*70)
    print("TRADE STATISTICS")
    print("-"*70)

    trade_types = trades_df['type'].value_counts()
    for t, count in trade_types.items():
        print(f"  {t}: {count}")

    # P&L by trade type
    print("\n" + "-"*70)
    print("P&L BY TRADE TYPE")
    print("-"*70)

    pnl_trades = trades_df[trades_df['pnl'].notna()]
    for trade_type in pnl_trades['type'].unique():
        type_trades = pnl_trades[pnl_trades['type'] == trade_type]
        total_pnl = type_trades['pnl'].sum()
        avg_pnl = type_trades['pnl'].mean()
        print(f"  {trade_type}: Total ${total_pnl:,.0f}, Avg ${avg_pnl:.0f}")

    # Win rate
    wins = len(pnl_trades[pnl_trades['pnl'] > 0])
    total = len(pnl_trades)
    win_rate = wins / total * 100 if total > 0 else 0

    print(f"\n  Win Rate: {win_rate:.1f}% ({wins}/{total})")
    print(f"  Total Premium Collected: ${pnl_trades['pnl'].sum():,.0f}")

    # Yearly breakdown
    print("\n" + "-"*70)
    print("YEARLY BREAKDOWN")
    print("-"*70)

    history['year'] = history['date'].dt.year
    for year in sorted(history['year'].unique()):
        year_data = history[history['year'] == year]
        year_start = year_data.iloc[0]['total_value']
        year_end = year_data.iloc[-1]['total_value']
        year_return = (year_end / year_start - 1) * 100
        year_dd = year_data['drawdown'].min() * 100
        print(f"  {year}: {year_return:+.1f}% (MaxDD: {year_dd:.1f}%)")

    # Compare with SPY buy & hold
    print("\n" + "-"*70)
    print("VS SPY BUY & HOLD")
    print("-"*70)

    spy_start = history.iloc[0]['underlying']
    spy_end = history.iloc[-1]['underlying']
    spy_return = (spy_end / spy_start - 1) * 100
    spy_cagr = ((spy_end / spy_start) ** (1/years) - 1) * 100

    # Calculate SPY max drawdown
    spy_series = history[['date', 'underlying']].copy()
    spy_series['peak'] = spy_series['underlying'].cummax()
    spy_series['dd'] = (spy_series['underlying'] - spy_series['peak']) / spy_series['peak']
    spy_max_dd = spy_series['dd'].min() * 100

    print(f"  SPY Buy & Hold: {spy_return:.1f}% ({spy_cagr:.1f}% CAGR), MaxDD: {spy_max_dd:.1f}%")
    print(f"  Wheel Strategy: {total_return:.1f}% ({cagr:.1f}% CAGR), MaxDD: {max_dd:.1f}%")

    if cagr > spy_cagr:
        print(f"\n  >>> WHEEL WINS by {cagr - spy_cagr:.1f}%p CAGR")
    else:
        print(f"\n  >>> SPY B&H WINS by {spy_cagr - cagr:.1f}%p CAGR")

    return {
        'total_return': total_return,
        'cagr': cagr,
        'max_dd': max_dd,
        'win_rate': win_rate,
        'spy_cagr': spy_cagr
    }


def main():
    df = load_spy_options()
    trades, history = run_wheel_backtest(df)
    results = analyze_results(trades, history)

    print("\n" + "="*70)
    print("CONCLUSION")
    print("="*70)
    print(f"Wheel Strategy CAGR: {results['cagr']:.1f}%")
    print(f"Win Rate: {results['win_rate']:.1f}%")
    print(f"Max Drawdown: {results['max_dd']:.1f}%")
    print("="*70)


if __name__ == "__main__":
    main()
