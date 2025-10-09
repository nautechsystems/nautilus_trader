
"""
Validate MFI against TradingView values
"""
import pandas as pd
from nautilus_trader.indicators.mfi import MoneyFlowIndex


tradingview_data = [
    # (high, low, close, volume, expected_mfi_from_tradingview)
    (94058,93600,94031,340,50.00),
    (94450,93671,94175,752,52.09),
    (94500,93900,94432,486,62.09),
    (94492,93922,94090,580,53.91),
    (94189,93760,94143,304,53.13),
    (94380,93936,94340,292,58.05),
    (94823,94148,94627,545,60.38),
    (94852,94579,94786,424,65.68),
    (95151,94687,94978,637,67.96),
    (94997,94685, 94802,410,68.63),
    (94952,94542,94591,437,72.45),
    (95381,94392,95128,1180,75.99),
    (95516, 94699, 94862,1140,78.48),
    (95234, 94803, 95234,456,78.38),
    (95299,94913,95010,290,74.58),
    (96000,94984,95708,926,75.16),
    (95800,95395,95596,702,75.83),
    (95697, 95380,95574,517,76.33),
    (95880,95505,95822,453,80.34),
    (96211,95640,96150,845,81.58),
    (96799,96036,96755,976,82.45),
    (96808,96538,96708,827,83.20),
    (96876,96469,96732,534,83.04),
    (96895,96609,96850,677,87.59),
    (96988,96436,96458,903,83.56),
    (97440,95924,97256,3010,86.06),
    (97323,96164,96455,2010,71.66),
    (97650,96040,97229,1730,74.19),
    (97591,96325,96757,1270,69.40)
  
    # Add more rows to match TradingView data
]

# Create MFI indicator
mfi = MoneyFlowIndex(period=14)

print("Validating MFI against TradingView...")
print("-" * 70)
print(f"{'Bar #':>6} {'High':>8} {'Low':>8} {'Close':>8} {'Volume':>10} {'Our MFI':>10} {'TV MFI':>10} {'Diff':>8}")
print("-" * 70)

for i, (high, low, close, volume, tv_mfi) in enumerate(tradingview_data, 1):
    # Calculate our MFI
    our_mfi = mfi.update(close=float(close), high=float(high), low=float(low), volume=float(volume))
    
    # Convert to percentage (0-100 scale) to match TradingView display
    our_mfi_pct = our_mfi * 100
    
    # Compare if TradingView value is provided
    diff = ""
    if tv_mfi is not None:
        diff = f"{abs(our_mfi_pct - tv_mfi):.2f}"
    
    print(f"{i:>6} {high:>8} {low:>8} {close:>8} {volume:>10} {our_mfi_pct:>10.2f} {tv_mfi or 'N/A':>10} {diff:>8}")

print("-" * 70)
print("\nNote: MFI needs 14 bars to be fully initialized with period=14")
print("First value should be 50.0 (neutral)")

# Alternative: Load from CSV
print("\n\nAlternative: Load data from CSV file")
print("CSV format: timestamp,open,high,low,close,volume")

def validate_from_csv(filename, period=14):
    """
    Load OHLCV data from CSV and calculate MFI
    Compare with TradingView by manually checking values
    """
    df = pd.read_csv(filename, parse_dates=['timestamp'])
    mfi = MoneyFlowIndex(period=period)
    
    mfi_values = []
    for _, row in df.iterrows():
        value = mfi.update(
            close=float(row['close']),
            high=float(row['high']),
            low=float(row['low']),
            volume=float(row['volume'])
        )
        mfi_values.append(value * 100)  # Convert to percentage
    
    df['mfi'] = mfi_values
    
    # Display last 20 values for comparison
    print(f"\nLast 20 MFI values from {filename}:")
    print(df[['timestamp', 'close', 'volume', 'mfi']].tail(20))
    
    return df

