#!/usr/bin/env python3
"""
Analyze real strangle returns from options data
실제 스트랭글 매도 수익률 분석
"""

import pandas as pd
import numpy as np
from datetime import timedelta

def load_options():
    print("Loading options data...")
    df = pd.read_csv('spy_options_2020_2022/spy_2020_2022.csv')
    df.columns = [c.strip().replace('[', '').replace(']', '') for c in df.columns]
    df['QUOTE_DATE'] = pd.to_datetime(df['QUOTE_DATE'].str.strip())
    df['EXPIRE_DATE'] = pd.to_datetime(df['EXPIRE_DATE'].str.strip())

    numeric_cols = ['UNDERLYING_LAST', 'DTE', 'STRIKE', 'C_BID', 'C_ASK', 'P_BID', 'P_ASK',
                    'C_DELTA', 'P_DELTA', 'C_IV', 'P_IV']
    for col in numeric_cols:
        if col in df.columns:
            df[col] = pd.to_numeric(df[col], errors='coerce')

    return df


def find_strangle(df, date, underlying, target_dte=30, target_delta=0.16):
    """Find 16-delta strangle."""
    day_opts = df[(df['QUOTE_DATE'] == date) &
                   (df['DTE'] >= target_dte - 5) &
                   (df['DTE'] <= target_dte + 5)].copy()

    if len(day_opts) == 0:
        return None

    # Find PUT
    puts = day_opts[day_opts['P_DELTA'].notna() & (day_opts['P_DELTA'] < 0)].copy()
    if len(puts) == 0:
        return None
    puts['delta_diff'] = abs(puts['P_DELTA'] + target_delta)
    best_put = puts.loc[puts['delta_diff'].idxmin()]

    # Find CALL
    calls = day_opts[day_opts['C_DELTA'].notna() & (day_opts['C_DELTA'] > 0)].copy()
    if len(calls) == 0:
        return None
    calls['delta_diff'] = abs(calls['C_DELTA'] - target_delta)
    best_call = calls.loc[calls['delta_diff'].idxmin()]

    return {
        'date': date,
        'underlying': underlying,
        'put_strike': best_put['STRIKE'],
        'put_premium': (best_put['P_BID'] + best_put['P_ASK']) / 2,
        'put_delta': best_put['P_DELTA'],
        'put_dte': best_put['DTE'],
        'call_strike': best_call['STRIKE'],
        'call_premium': (best_call['C_BID'] + best_call['C_ASK']) / 2,
        'call_delta': best_call['C_DELTA'],
        'call_dte': best_call['DTE'],
        'expire_date': best_put['EXPIRE_DATE']
    }


def calculate_strangle_pnl(df, entry, hold_days=21):
    """Calculate actual P&L at exit."""
    exit_date = entry['date'] + timedelta(days=hold_days)

    # Find options at exit with same strikes
    put_at_exit = df[(df['QUOTE_DATE'] >= exit_date - timedelta(days=2)) &
                      (df['QUOTE_DATE'] <= exit_date + timedelta(days=2)) &
                      (df['STRIKE'] == entry['put_strike'])].head(1)

    call_at_exit = df[(df['QUOTE_DATE'] >= exit_date - timedelta(days=2)) &
                       (df['QUOTE_DATE'] <= exit_date + timedelta(days=2)) &
                       (df['STRIKE'] == entry['call_strike'])].head(1)

    if len(put_at_exit) == 0 or len(call_at_exit) == 0:
        return None

    put_exit_price = (put_at_exit.iloc[0]['P_BID'] + put_at_exit.iloc[0]['P_ASK']) / 2
    call_exit_price = (call_at_exit.iloc[0]['C_BID'] + call_at_exit.iloc[0]['C_ASK']) / 2

    entry_premium = entry['put_premium'] + entry['call_premium']
    exit_cost = put_exit_price + call_exit_price

    pnl = entry_premium - exit_cost
    pnl_pct = pnl / entry['underlying'] * 100  # % of underlying

    return {
        'entry_date': entry['date'],
        'exit_date': exit_date,
        'underlying_at_entry': entry['underlying'],
        'put_strike': entry['put_strike'],
        'call_strike': entry['call_strike'],
        'entry_premium': entry_premium,
        'exit_cost': exit_cost,
        'pnl': pnl,
        'pnl_pct': pnl_pct,
        'return_on_margin': pnl / (entry['underlying'] * 0.20) * 100  # 20% margin
    }


def main():
    df = load_options()

    # Get unique dates
    dates = df['QUOTE_DATE'].unique()
    dates = sorted(dates)

    # Sample monthly (first trading day of each month)
    monthly_dates = []
    current_month = None
    for d in dates:
        month = pd.Timestamp(d).month
        if month != current_month:
            monthly_dates.append(pd.Timestamp(d))
            current_month = month

    print(f"\nAnalyzing {len(monthly_dates)} monthly strangles...")
    print("="*80)

    results = []
    for date in monthly_dates[:36]:  # 3 years
        # Get underlying price
        day_data = df[df['QUOTE_DATE'] == date]
        if len(day_data) == 0:
            continue
        underlying = day_data.iloc[0]['UNDERLYING_LAST']

        strangle = find_strangle(df, date, underlying)
        if strangle is None:
            continue

        pnl = calculate_strangle_pnl(df, strangle)
        if pnl is None:
            continue

        results.append(pnl)
        print(f"{pnl['entry_date'].strftime('%Y-%m')}: "
              f"SPY ${underlying:.0f} | "
              f"Put {pnl['put_strike']:.0f} / Call {pnl['call_strike']:.0f} | "
              f"Premium ${pnl['entry_premium']:.2f} | "
              f"P&L ${pnl['pnl']:.2f} ({pnl['return_on_margin']:+.1f}% on margin)")

    if len(results) > 0:
        results_df = pd.DataFrame(results)

        print("\n" + "="*80)
        print("STRANGLE ANALYSIS SUMMARY")
        print("="*80)

        total_pnl = results_df['pnl'].sum()
        avg_pnl = results_df['pnl'].mean()
        win_rate = (results_df['pnl'] > 0).mean() * 100
        avg_return = results_df['return_on_margin'].mean()

        print(f"\nTotal Trades: {len(results_df)}")
        print(f"Win Rate: {win_rate:.1f}%")
        print(f"Average P&L per trade: ${avg_pnl:.2f}")
        print(f"Average Return on Margin: {avg_return:.2f}%")
        print(f"Total P&L: ${total_pnl:.2f}")

        # Yearly breakdown
        print("\n--- Yearly Breakdown ---")
        results_df['year'] = pd.to_datetime(results_df['entry_date']).dt.year
        for year in sorted(results_df['year'].unique()):
            year_data = results_df[results_df['year'] == year]
            year_pnl = year_data['pnl'].sum()
            year_avg = year_data['return_on_margin'].mean()
            year_win = (year_data['pnl'] > 0).mean() * 100
            print(f"{year}: {len(year_data)} trades | "
                  f"Win Rate: {year_win:.0f}% | "
                  f"Avg Return: {year_avg:.1f}% | "
                  f"Total P&L: ${year_pnl:.0f}")


if __name__ == "__main__":
    main()
