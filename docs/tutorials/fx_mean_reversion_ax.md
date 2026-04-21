# Mean Reversion with Proxy FX Data (AX Exchange)

This tutorial backtests a **Bollinger Band mean reversion** strategy on
**EURUSD-PERP** (EUR/USD perpetual) using [AX Exchange](https://architect.exchange)
instrument definitions and [TrueFX](https://www.truefx.com) spot FX data as a proxy.

## Introduction

Mean reversion strategies assume prices return to a statistical average after deviating from it.
**Bollinger Bands** give a volatility-adaptive envelope around a moving average: the upper and
lower bands expand in volatile markets and contract in quiet ones. A touch of a band flags price
as overextended relative to recent history.

The strategy adds a **Relative Strength Index (RSI)** filter as confirmation. A lower-band touch
alone does not trigger a buy: RSI must also read oversold. The two-indicator gate cuts whipsaws
in trending markets.

The `BBMeanReversion` strategy shipped with NautilusTrader is intentionally simple (no alpha
advantage).

### Why proxy data?

AX Exchange is a new venue not yet covered by most historical data vendors.
[TrueFX](https://www.truefx.com) provides free institutional-grade spot FX tick data sourced
from Integral and Jefferies liquidity pools. EUR/USD spot data serves as a proxy for
backtesting an AX EURUSD-PERP strategy.

## Prerequisites

- **NautilusTrader** installed (see the [installation guide](../getting_started/installation.md)).
- **TrueFX account** (free): Sign up at [truefx.com](https://www.truefx.com) to access
  historical tick data downloads.

## Data preparation

### Download TrueFX EUR/USD tick data

1. Go to the [TrueFX historical downloads page](https://www.truefx.com/truefx-historical-downloads/).
2. Select **EUR/USD** and your desired month (e.g., December 2025).
3. Download and extract the CSV file (e.g., `EURUSD-2025-12.csv`).

The raw TrueFX format has **no headers**. Columns are: `pair, timestamp, bid, ask`.

### Load and prepare the data

Use pandas to load the CSV and parse timestamps, then process through
`QuoteTickDataWrangler`, which auto-renames `bid` and `ask` columns:

```python
from pathlib import Path

import pandas as pd

from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler

data_path = Path("EURUSD-2025-12.csv")

df = pd.read_csv(
    data_path,
    header=None,
    names=["pair", "timestamp", "bid", "ask"],
)
df["timestamp"] = pd.to_datetime(df["timestamp"], format="%Y%m%d %H:%M:%S.%f")
df = df.set_index("timestamp")
df = df[["bid", "ask"]]

wrangler = QuoteTickDataWrangler(instrument=EURUSD_PERP)  # defined below
ticks = wrangler.process(df)
```

The wrangler produces `QuoteTick` objects tagged with the instrument ID. These ticks drive
bar aggregation internally: 1-minute MID bars are built from the quote tick stream.

## Instrument definition

With proxy data, we define the EURUSD-PERP instrument manually as a `PerpetualContract`. The
multiplier of 1000 means each contract represents 1000 EUR notional:

```python
from decimal import Decimal

from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import PerpetualContract
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

instrument_id = InstrumentId.from_str("EURUSD-PERP.AX")

EURUSD_PERP = PerpetualContract(
    instrument_id=instrument_id,
    raw_symbol=Symbol("EURUSD-PERP"),
    underlying="EUR",
    asset_class=AssetClass.FX,
    quote_currency=USD,
    settlement_currency=USD,
    is_inverse=False,
    price_precision=5,
    size_precision=0,
    price_increment=Price.from_str("0.00001"),
    size_increment=Quantity.from_int(1),
    multiplier=Quantity.from_int(1000),
    lot_size=Quantity.from_int(1),
    margin_init=Decimal("0.05"),
    margin_maint=Decimal("0.025"),
    maker_fee=Decimal("0.0002"),
    taker_fee=Decimal("0.0005"),
    ts_event=0,
    ts_init=0,
)
```

Fees are explicit backtest assumptions and should be set deliberately. Check the
[AX Exchange documentation](https://docs.architect.exchange/) for current rates.

## Strategy overview

The `BBMeanReversion` strategy works as follows:

1. **Wait for warm-up**: Both indicators must be initialized before trading.
2. **Exit check (first)**: If long and close >= BB middle band, close the position
   (mean reversion target reached). If short and close <= BB middle band, close the
   position.
3. **Entry signals**: If close <= BB lower band AND RSI < buy threshold, buy. If
   close >= BB upper band AND RSI > sell threshold, sell. Existing positions in the
   opposite direction are closed before entering.

### Configuration

| Parameter            | Value  | Description                                     |
|----------------------|--------|-------------------------------------------------|
| `bb_period`          | `20`   | 20-bar lookback for Bollinger Bands.            |
| `bb_std`             | `2.0`  | 2 standard deviations for band width.           |
| `rsi_period`         | `14`   | 14-bar lookback for RSI.                        |
| `rsi_buy_threshold`  | `0.30` | RSI below 0.30 confirms oversold (range 0-1).   |
| `rsi_sell_threshold` | `0.70` | RSI above 0.70 confirms overbought (range 0-1). |
| `trade_size`         | `1`    | 1 contract per trade (1000 EUR notional).       |

:::tip
NautilusTrader RSI outputs values in the range [0.0, 1.0], not [0, 100]. Set thresholds
accordingly: 0.30 corresponds to the traditional RSI level of 30.
:::

## Backtest setup

### Configure the engine

```python
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money

config = BacktestEngineConfig(
    trader_id=TraderId("BACKTESTER-001"),
    logging=LoggingConfig(log_level="INFO"),
)

engine = BacktestEngine(config=config)
```

### Add the venue

AX Exchange uses margin accounts with netting position management:

```python
AX = Venue("AX")

engine.add_venue(
    venue=AX,
    oms_type=OmsType.NETTING,
    account_type=AccountType.MARGIN,
    base_currency=USD,
    starting_balances=[Money(100_000, USD)],
)
```

### Add instrument, data, and strategy

The bar type `EURUSD-PERP.AX-1-MINUTE-MID-INTERNAL` tells the engine to build 1-minute
bars from the mid-price of the quote tick stream:

```python
from nautilus_trader.examples.strategies.bb_mean_reversion import BBMeanReversion
from nautilus_trader.examples.strategies.bb_mean_reversion import BBMeanReversionConfig
from nautilus_trader.model.data import BarType

bar_type = BarType.from_str("EURUSD-PERP.AX-1-MINUTE-MID-INTERNAL")

strategy_config = BBMeanReversionConfig(
    instrument_id=instrument_id,
    bar_type=bar_type,
    trade_size=Decimal("1"),
    bb_period=20,
    bb_std=2.0,
    rsi_period=14,
    rsi_buy_threshold=0.30,
    rsi_sell_threshold=0.70,
)

strategy = BBMeanReversion(config=strategy_config)

engine.add_instrument(EURUSD_PERP)
engine.add_data(ticks)
engine.add_strategy(strategy)
```

### Run the backtest

```python
engine.run()
```

## Results

Generate reports to analyze performance:

```python
import pandas as pd

with pd.option_context(
    "display.max_rows", 100,
    "display.max_columns", None,
    "display.width", 300,
):
    print(engine.trader.generate_account_report(AX))
    print(engine.trader.generate_order_fills_report())
    print(engine.trader.generate_positions_report())
```

Clean up when done:

```python
engine.reset()
engine.dispose()
```

## Complete script

The full script lives at
[`architect_ax_mean_reversion.py`](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/backtest/architect_ax_mean_reversion.py).

## Next steps

- **Tune parameters**: vary `bb_period`, `bb_std`, and RSI thresholds to see their effect on
  trade frequency and PnL.
- **Try different pairs**: download GBP/USD or USD/JPY data from TrueFX and define the
  matching perpetual contract.
- **Add stop losses**: extend the strategy with stop-loss orders to limit downside on
  positions that move against you.
- **Go live on AX sandbox**: connect to the AX sandbox for paper trading. See the
  [AX Exchange integration guide](../integrations/architect_ax.md) for setup.

## Running live

The same `BBMeanReversion` strategy runs live against AX Exchange. The launch script swaps
the `BacktestEngine` for a `TradingNode` with the AX data and execution clients configured.
See the live example:
[`ax_mean_reversion.py`](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/architect_ax/ax_mean_reversion.py).

For connection setup and API key configuration, see the
[AX Exchange integration guide](../integrations/architect_ax.md).

## Further reading

- [AX Exchange mean reversion backtest example](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/backtest/architect_ax_mean_reversion.py)
- [`BBMeanReversion` strategy source](https://github.com/nautechsystems/nautilus_trader/tree/develop/nautilus_trader/examples/strategies/bb_mean_reversion.py)
- [Gold perpetual book imbalance tutorial](gold_book_imbalance_ax.md)
- [Architect Exchange documentation](https://docs.architect.exchange/)
