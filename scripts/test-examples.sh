#!/bin/bash
# Fail fast
set -e

# Backtest examples
example_scripts=(
  "crypto_ema_cross_ethusdt_trade_ticks.py"
  "crypto_ema_cross_ethusdt_trailing_stop.py"
  "fx_ema_cross_audusd_bars_from_ticks.py"
  "fx_ema_cross_bracket_gbpusd_bars_external.py"
  "fx_ema_cross_bracket_gbpusd_bars_internal.py"
  "fx_market_maker_gbpusd_bars.py"
)

total_runtime=0
for script in "${example_scripts[@]}"; do
  start_time=$(date +%s)

  # Run the backtest script
  chmod +x "examples/backtest/$script"
  yes | python "examples/backtest/$script"

  # Get the exit status of the last example run
  exit_status=$?

  # Check if the exit status is 0 (success)
  if [ $exit_status -eq 0 ]; then
    end_time=$(date +%s)
    runtime=$((end_time - start_time))
    echo "$script finished successfully in $runtime seconds"
    total_runtime=$((total_runtime + runtime))
  else
    echo "$script failed with exit status $exit_status."
  fi
done
echo "Total runtime of all examples: $total_runtime seconds"
