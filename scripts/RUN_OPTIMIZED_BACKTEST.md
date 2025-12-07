# Running the Optimized Streaming Backtest

## Quick Start

### Run Command

```powershell
.\.venv\Scripts\Activate.ps1; python scripts/backtest_streaming.py
```

---

## What to Expect

### 1. **Initial Output**
```
================================================================================
NautilusTrader Streaming Backtest - Optimized Orderflow Indicators
================================================================================
✓ Found instrument: ETHUSDT-PERP.BINANCE
✓ Tick size: 0.01

📊 Analyzing dataset...
  - Total ticks: 635,000,000

📊 Backtest Configuration:
  - Date range: 2025-03-01 to 2025-03-07
  - Streaming mode: ENABLED (chunk_size=10,000)
  - Memory usage: ~2-5 GB (constant)
  - Optimized indicators: VolumeProfile, Footprint, StackedImbalance
  - Logging: INFO level (check logs/ directory for detailed logs)

🚀 Running streaming backtest...
--------------------------------------------------------------------------------
```

### 2. **During Execution**
You'll see INFO-level logs from the NautilusTrader engine showing:
- Data loading progress
- Strategy initialization
- Tick processing updates
- Order executions
- Position updates

Example logs:
```
2025-12-07 10:30:00 [INFO] BacktestEngine: Processing chunk 1/63500...
2025-12-07 10:30:05 [INFO] OrderFlowStrategy: Trade signal detected at 2500.50
2025-12-07 10:30:05 [INFO] BacktestEngine: Order submitted: BUY 10.0 ETHUSDT-PERP
```

### 3. **Final Output**
```
--------------------------------------------------------------------------------

📈 Backtest Results:
  - Duration: 0:15:30
  - Instance ID: backtest-001
  - Run started: 2025-12-07 10:30:00
  - Run finished: 2025-12-07 10:45:30
  - Strategy: OrderFlowStrategy

✅ Streaming backtest complete!

💡 Performance Notes:
  - Lazy evaluation: Indicators only recalculate when accessed
  - Incremental POC: O(1) updates instead of O(n) scans
  - Memory efficient: Only one chunk in RAM at a time

📁 Detailed logs saved to: logs/

⚡ Performance: 682,795 ticks/second
```

---

## Monitoring Progress

### 1. **Terminal Output**
Watch the terminal for real-time INFO logs showing:
- Chunk processing progress
- Strategy signals
- Order executions

### 2. **Log Files**
Detailed JSON logs are saved to `logs/` directory:
```powershell
# View latest log file
Get-ChildItem logs\ -Filter *.log | Sort-Object LastWriteTime -Descending | Select-Object -First 1 | Get-Content -Tail 50
```

### 3. **Memory Usage**
Monitor memory in another PowerShell window:
```powershell
# Watch memory usage (refresh every 2 seconds)
while ($true) { 
    $proc = Get-Process python -ErrorAction SilentlyContinue
    if ($proc) { 
        Write-Host "Memory: $([math]::Round($proc.WorkingSet64/1GB, 2)) GB" 
    }
    Start-Sleep -Seconds 2
    Clear-Host
}
```

---

## Expected Performance

### For Your Dataset (9.26 GB, 635M ticks):

| Metric | Expected Value |
|--------|----------------|
| **Duration** | 10-30 minutes |
| **Memory usage** | 2-5 GB (constant) |
| **Processing speed** | 350,000 - 1,000,000 ticks/sec |
| **Chunk size** | 10,000 ticks |
| **Total chunks** | ~63,500 chunks |

### Performance Factors:
- **CPU**: Your 6-core i5-11400H should handle this well
- **Disk I/O**: SSD recommended for Parquet reading
- **Optimizations**: 100-1000x faster indicator calculations vs. original

---

## Troubleshooting

### If backtest seems stuck:
1. **Check logs**: Look in `logs/` directory for error messages
2. **Memory**: Ensure you have 5+ GB free RAM
3. **Date range**: Verify data exists for 2025-03-01 to 2025-03-07

### If you see errors:
1. **ImportError**: Make sure venv is activated
2. **ModuleNotFoundError**: Run `pip install -e .` in the venv
3. **Data not found**: Check `catalog/data/trade_tick/ETHUSDT-PERP.BINANCE/`

### To adjust date range:
Edit `scripts/backtest_streaming.py` lines 100-101:
```python
start_time="2025-03-01",  # Change this
end_time="2025-03-07",    # Change this
```

### To adjust chunk size:
Edit `scripts/backtest_streaming.py` line 145:
```python
chunk_size=10_000,  # Increase for speed, decrease for memory safety
```

---

## What Changed vs. Original

### ✅ Optimizations Applied:

1. **VolumeProfile**: Lazy evaluation for VAH/VAL/HVN/LVN, incremental POC
2. **FootprintAggregator**: Lazy evaluation for imbalanced levels
3. **StackedImbalanceDetector**: Lazy evaluation for stacked imbalance detection
4. **Streaming Mode**: Memory-efficient chunk processing

### 📊 Performance Improvement:

- **Before**: ~1.3 quadrillion operations (would take hours/days)
- **After**: ~19 billion operations (10-30 minutes)
- **Speedup**: 100-1000x for indicator calculations

---

## Next Steps After Backtest

1. **Review results** in the terminal output
2. **Check detailed logs** in `logs/` directory
3. **Analyze performance** - compare with non-streaming if desired
4. **Adjust parameters** - tune strategy config, chunk size, date range
5. **Scale up** - try longer date ranges once validated

---

## Full Command Reference

```powershell
# Activate venv and run backtest
.\.venv\Scripts\Activate.ps1; python scripts/backtest_streaming.py

# Run with output to file
.\.venv\Scripts\Activate.ps1; python scripts/backtest_streaming.py | Tee-Object -FilePath backtest_output.txt

# Run and monitor memory simultaneously (two terminals)
# Terminal 1:
.\.venv\Scripts\Activate.ps1; python scripts/backtest_streaming.py

# Terminal 2:
while ($true) { Get-Process python -ErrorAction SilentlyContinue | Select-Object WorkingSet64 | ForEach-Object { "Memory: $([math]::Round($_.WorkingSet64/1GB, 2)) GB" }; Start-Sleep 2; Clear-Host }
```

---

**Ready to run!** 🚀

