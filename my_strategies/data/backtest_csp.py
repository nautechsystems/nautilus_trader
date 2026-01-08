#!/usr/bin/env python3
"""
Cash Secured Put (CSP) Backtest on QQQ
QQQ 현금담보풋 매도 전략 백테스트

Strategy:
- Sell OTM puts (5-7% below current price)
- 2-4 week expiration (14-30 DTE)
- If assigned: hold shares until sold or next cycle
- If not assigned: collect premium and repeat
"""

import pandas as pd
import numpy as np
from datetime import timedelta
import warnings
warnings.filterwarnings('ignore')

# Configuration
STARTING_CAPITAL = 100_000
OTM_PERCENT = 0.05  # 5% OTM
TARGET_DTE_MIN = 14
TARGET_DTE_MAX = 30
COMMISSION_PER_CONTRACT = 1.00  # $1 per contract


def load_qqq_options():
    """Load QQQ options data."""
    print("Loading QQQ options data...")
    df = pd.read_csv('qqq_options_2020_2022/qqq_2020_2022.csv')

    # Clean column names
    df.columns = [c.strip().replace('[', '').replace(']', '') for c in df.columns]

    # Parse dates
    df['QUOTE_DATE'] = pd.to_datetime(df['QUOTE_DATE'].str.strip())
    df['EXPIRE_DATE'] = pd.to_datetime(df['EXPIRE_DATE'].str.strip())

    # Convert numeric columns
    numeric_cols = ['UNDERLYING_LAST', 'DTE', 'STRIKE', 'P_BID', 'P_ASK', 'P_DELTA']
    for col in numeric_cols:
        if col in df.columns:
            df[col] = pd.to_numeric(df[col], errors='coerce')

    print(f"  Loaded {len(df):,} rows")
    print(f"  Date range: {df['QUOTE_DATE'].min()} to {df['QUOTE_DATE'].max()}")

    return df


def find_put_to_sell(df, date, underlying_price, target_otm=0.05, min_dte=14, max_dte=30):
    """Find the best put to sell."""
    # Target strike price (OTM)
    target_strike = underlying_price * (1 - target_otm)

    # Filter options for this date with appropriate DTE
    day_opts = df[(df['QUOTE_DATE'] == date) &
                   (df['DTE'] >= min_dte) &
                   (df['DTE'] <= max_dte) &
                   (df['STRIKE'] <= underlying_price)].copy()  # OTM puts only

    if len(day_opts) == 0:
        return None

    # Find strike closest to target
    day_opts['strike_diff'] = abs(day_opts['STRIKE'] - target_strike)
    best = day_opts.loc[day_opts['strike_diff'].idxmin()]

    # Calculate mid price
    p_bid = best['P_BID'] if pd.notna(best['P_BID']) else 0
    p_ask = best['P_ASK'] if pd.notna(best['P_ASK']) else 0

    if p_bid <= 0:
        return None

    premium = (p_bid + p_ask) / 2 if p_ask > 0 else p_bid

    return {
        'date': date,
        'underlying': underlying_price,
        'strike': best['STRIKE'],
        'premium': premium,
        'dte': best['DTE'],
        'expire_date': best['EXPIRE_DATE'],
        'delta': best['P_DELTA'] if pd.notna(best['P_DELTA']) else 0
    }


def check_assignment(df, put_info):
    """Check if put was assigned at expiration."""
    expire_date = put_info['expire_date']
    strike = put_info['strike']

    # Find underlying price at expiration
    expire_data = df[df['QUOTE_DATE'] == expire_date]
    if len(expire_data) == 0:
        # Try nearby dates
        for delta in range(1, 5):
            expire_data = df[df['QUOTE_DATE'] == expire_date + timedelta(days=delta)]
            if len(expire_data) > 0:
                break
            expire_data = df[df['QUOTE_DATE'] == expire_date - timedelta(days=delta)]
            if len(expire_data) > 0:
                break

    if len(expire_data) == 0:
        return None, None

    underlying_at_expire = expire_data.iloc[0]['UNDERLYING_LAST']
    assigned = underlying_at_expire < strike

    return assigned, underlying_at_expire


def run_csp_backtest(df):
    """Run CSP backtest."""
    print("\nRunning CSP backtest...")

    # Get unique trading dates
    dates = sorted(df['QUOTE_DATE'].unique())

    balance = STARTING_CAPITAL
    shares_held = 0
    share_cost_basis = 0
    current_put = None

    trades = []
    history = []

    for date in dates:
        day_data = df[df['QUOTE_DATE'] == date]
        if len(day_data) == 0:
            continue

        underlying = day_data.iloc[0]['UNDERLYING_LAST']

        # Check if current put expired
        if current_put is not None and date >= current_put['expire_date']:
            assigned, price_at_expire = check_assignment(df, current_put)

            if assigned is not None:
                if assigned:
                    # Assigned - we buy 100 shares at strike price
                    cost = current_put['strike'] * 100
                    if balance >= cost:
                        balance -= cost
                        shares_held += 100
                        share_cost_basis = current_put['strike']
                        trades.append({
                            'date': current_put['expire_date'],
                            'type': 'ASSIGNED',
                            'strike': current_put['strike'],
                            'underlying': price_at_expire,
                            'pnl': current_put['premium'] * 100 - (current_put['strike'] - price_at_expire) * 100
                        })
                else:
                    # Not assigned - keep full premium
                    trades.append({
                        'date': current_put['expire_date'],
                        'type': 'EXPIRED_WORTHLESS',
                        'strike': current_put['strike'],
                        'underlying': price_at_expire,
                        'pnl': current_put['premium'] * 100
                    })

            current_put = None

        # If holding shares, check if we should sell (price recovered above cost basis + 2%)
        if shares_held > 0 and underlying > share_cost_basis * 1.02:
            proceeds = shares_held * underlying
            balance += proceeds
            pnl = (underlying - share_cost_basis) * shares_held
            trades.append({
                'date': date,
                'type': 'SOLD_SHARES',
                'price': underlying,
                'shares': shares_held,
                'pnl': pnl
            })
            shares_held = 0
            share_cost_basis = 0

        # If no current put and not holding too many shares, sell new put
        if current_put is None and shares_held == 0:
            # Check if we have enough cash to secure the put
            max_strike = balance / 100  # Maximum strike we can afford to secure

            put = find_put_to_sell(df, date, underlying, OTM_PERCENT, TARGET_DTE_MIN, TARGET_DTE_MAX)

            if put is not None and put['strike'] <= max_strike:
                current_put = put
                premium_received = put['premium'] * 100 - COMMISSION_PER_CONTRACT
                balance += premium_received
                trades.append({
                    'date': date,
                    'type': 'SELL_PUT',
                    'strike': put['strike'],
                    'premium': put['premium'],
                    'dte': put['dte'],
                    'delta': put['delta'],
                    'underlying': underlying
                })

        # Calculate total value
        shares_value = shares_held * underlying
        # If we have a current put, account for potential liability
        put_liability = 0
        if current_put is not None:
            intrinsic = max(0, current_put['strike'] - underlying)
            put_liability = intrinsic * 100

        total_value = balance + shares_value - put_liability

        history.append({
            'date': date,
            'balance': balance,
            'shares_held': shares_held,
            'shares_value': shares_value,
            'total_value': total_value,
            'underlying': underlying,
            'has_put': current_put is not None
        })

    return trades, pd.DataFrame(history)


def analyze_results(trades, history):
    """Analyze backtest results."""
    print("\n" + "="*70)
    print("CSP BACKTEST RESULTS (QQQ 2020-2022)")
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

    print(f"\nStarting Capital: ${initial:,.0f}")
    print(f"Final Value: ${final:,.0f}")
    print(f"Total Return: {total_return:.1f}%")
    print(f"CAGR: {cagr:.1f}%")
    print(f"Max Drawdown: {max_dd:.1f}%")
    print(f"Period: {years:.1f} years")

    # Trade stats
    print("\n" + "-"*70)
    print("TRADE STATISTICS")
    print("-"*70)

    put_sells = trades_df[trades_df['type'] == 'SELL_PUT']
    expired_worthless = trades_df[trades_df['type'] == 'EXPIRED_WORTHLESS']
    assigned = trades_df[trades_df['type'] == 'ASSIGNED']

    print(f"Total Puts Sold: {len(put_sells)}")
    print(f"Expired Worthless (Win): {len(expired_worthless)} ({len(expired_worthless)/len(put_sells)*100:.1f}%)")
    print(f"Assigned (Loss risk): {len(assigned)} ({len(assigned)/len(put_sells)*100:.1f}%)")

    if len(expired_worthless) > 0:
        avg_premium_win = expired_worthless['pnl'].mean()
        print(f"Average Premium (Win): ${avg_premium_win:.2f}")

    if len(assigned) > 0:
        avg_assigned_pnl = assigned['pnl'].mean()
        print(f"Average P&L (Assigned): ${avg_assigned_pnl:.2f}")

    # Monthly returns
    print("\n" + "-"*70)
    print("MONTHLY PERFORMANCE")
    print("-"*70)

    history['month'] = history['date'].dt.to_period('M')
    monthly = history.groupby('month').agg({
        'total_value': ['first', 'last']
    })
    monthly.columns = ['start', 'end']
    monthly['return'] = (monthly['end'] / monthly['start'] - 1) * 100

    print(f"Average Monthly Return: {monthly['return'].mean():.2f}%")
    print(f"Best Month: {monthly['return'].max():.1f}%")
    print(f"Worst Month: {monthly['return'].min():.1f}%")
    print(f"Positive Months: {(monthly['return'] > 0).sum()}/{len(monthly)} ({(monthly['return'] > 0).mean()*100:.0f}%)")

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
        print(f"{year}: {year_return:+.1f}% (MaxDD: {year_dd:.1f}%)")

    # Compare with buy and hold QQQ
    print("\n" + "-"*70)
    print("VS BUY & HOLD QQQ")
    print("-"*70)

    qqq_start = history.iloc[0]['underlying']
    qqq_end = history.iloc[-1]['underlying']
    qqq_return = (qqq_end / qqq_start - 1) * 100
    qqq_cagr = ((qqq_end / qqq_start) ** (1/years) - 1) * 100

    print(f"QQQ Buy & Hold: {qqq_return:.1f}% ({qqq_cagr:.1f}% CAGR)")
    print(f"CSP Strategy: {total_return:.1f}% ({cagr:.1f}% CAGR)")

    if cagr > qqq_cagr:
        print(f"CSP WINS by {cagr - qqq_cagr:.1f}%p CAGR")
    else:
        print(f"QQQ B&H WINS by {qqq_cagr - cagr:.1f}%p CAGR")

    return {
        'total_return': total_return,
        'cagr': cagr,
        'max_dd': max_dd,
        'win_rate': len(expired_worthless) / len(put_sells) * 100 if len(put_sells) > 0 else 0
    }


def main():
    df = load_qqq_options()
    trades, history = run_csp_backtest(df)
    results = analyze_results(trades, history)

    print("\n" + "="*70)
    print("CONCLUSION")
    print("="*70)
    print(f"Win Rate: {results['win_rate']:.1f}%")
    print(f"CAGR: {results['cagr']:.1f}%")
    print(f"Max Drawdown: {results['max_dd']:.1f}%")
    print("="*70)


if __name__ == "__main__":
    main()
