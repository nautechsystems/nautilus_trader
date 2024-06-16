# Examples

The following code examples are organized by system environment context:
- `backtest` - Historical data with simulated venues
- `live` - Real-time data with live venues (paper trading or real accounts)
- `sandbox` - Real-time data with simulated venues

Within each environment context directory, are scripts organized by integration.

Ensure that the `nautilus_trader` package is either compiled from source or installed via pip before 
running the examples. See the [installation guide](https://nautilustrader.io/docs/getting_started/installation) 
for more information.

To execute an example script from the `examples` directory, use a command similar to the following:
```
python backtest/crypto_ema_cross_ethusdt_trade_ticks.py
```
