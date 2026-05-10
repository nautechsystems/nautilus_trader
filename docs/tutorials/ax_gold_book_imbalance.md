# AX Exchange - Gold Perpetual Book Imbalance

This tutorial walks through backtesting an **order book imbalance** strategy on
**XAU-PERP** (gold perpetual) using [AX Exchange](https://architect.exchange) instrument
definitions and [Databento](https://databento.com) CME gold futures data as a proxy.

## Introduction

Order book imbalance is a canonical microstructure signal used in high-frequency and
short-term trading. When there is significantly more volume resting on one side of the
book than the other, this can signal informed flow and near-term price movement in that
direction. For a deeper dive into the statistical foundations,
see Databento's [blog post on HFT signals with sklearn](https://databento.com/blog/hft-sklearn-python)
which demonstrates the predictive power of book imbalance features.

For demonstration purposes, NautilusTrader ships with an `OrderBookImbalance` example
strategy that is intentionally simple (no alpha advantage).
The strategy monitors the ratio of the smaller to larger side at the top of book, and when
this ratio drops below a configurable threshold it fires a fill-or-kill (FOK) limit order. Because
it only needs top-of-book data, it works with Databento `mbp-1` (market by price best bid/ask) quotes
rather than full depth-of-book, which is significantly cheaper to source.

### Why proxy data?

AX Exchange is a new venue and is not yet covered specifically by data vendors like Databento.
CME gold futures (GC) are the most liquid gold derivatives market globally, and
provide representative price action for backtesting gold strategies. We download CME GC
quote data from Databento and replay it through a NautilusTrader backtest with an AX-style
`PerpetualContract` instrument definition.

## Prerequisites

- **NautilusTrader** installed (see the [installation guide](../getting_started/installation.md)).
- **Databento API key**: Sign up at [databento.com](https://databento.com) and set the
  environment variable:

```bash
export DATABENTO_API_KEY="your-api-key"
```

- **Databento Python client**: Install with `pip install databento`.

## Data preparation

### Download CME gold futures quotes

We use Databento's `mbp-1` schema (top-of-book best bid/ask), which maps directly to
NautilusTrader `QuoteTick` objects. This is simpler and cheaper than downloading full
depth-of-book data.

We use a Databento **continuous contract** (`GC.v.0`) rather than a specific expiration like
`GCZ4`. Continuous contracts stitch together successive contracts based on a roll rule.
`v.0` tracks the highest-volume contract, which closely mirrors how a perpetual follows
liquidity. The `stype_in="continuous"` parameter tells Databento to resolve the symbol
through its continuous contract mapping.

```python
import databento as db
from pathlib import Path

data_path = Path("gc_gold_mbp1.dbn.zst")

if not data_path.exists():
    client = db.Historical()
    data = client.timeseries.get_range(
        dataset="GLBX.MDP3",
        symbols=["GC.v.0"],
        stype_in="continuous",
        schema="mbp-1",
        start="2024-11-15",
        end="2024-11-16",
    )
    data.to_file(data_path)
```

This downloads one day of top-of-book data for the front-month gold contract. The
file is written once and reused on subsequent runs. The `instrument_id` override in the
loading step below is safe because the continuous contract resolves to a single instrument
at any point in time.

### Load the data

Use the `DatabentoDataLoader` to parse the DBN file into Nautilus quote ticks.
We pass `instrument_id` to override the Databento symbology with our AX instrument ID,
so all quote ticks appear to come from XAU-PERP.AX:

```python
from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.model.identifiers import InstrumentId

instrument_id = InstrumentId.from_str("XAU-PERP.AX")

loader = DatabentoDataLoader()
quotes = loader.from_dbn_file(
    path=data_path,
    instrument_id=instrument_id,
)
```

## Instrument definition

Since we are using proxy data, we define the XAU-PERP instrument manually as a
`PerpetualContract`. The price precision and tick size are set to match the CME source
data, while margin and fee parameters reflect AX Exchange conditions:

```python
from decimal import Decimal

from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import PerpetualContract
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

XAU_PERP = PerpetualContract(
    instrument_id=instrument_id,
    raw_symbol=Symbol("XAU-PERP"),
    underlying="XAU",
    asset_class=AssetClass.COMMODITY,
    quote_currency=USD,
    settlement_currency=USD,
    is_inverse=False,
    price_precision=2,
    size_precision=0,
    price_increment=Price.from_str("0.01"),
    size_increment=Quantity.from_int(1),
    multiplier=Quantity.from_int(1),
    lot_size=Quantity.from_int(1),
    margin_init=Decimal("0.08"),
    margin_maint=Decimal("0.04"),
    maker_fee=Decimal("0.0002"),
    taker_fee=Decimal("0.0005"),
    ts_event=0,
    ts_init=0,
)
```

Fees are explicit backtest assumptions and should be set deliberately. Check the
[AX Exchange documentation](https://docs.architect.exchange/) for current rates.

## Strategy overview

The `OrderBookImbalance` strategy works as follows:

1. **Monitor top of book**: On each quote tick update, compute the ratio of the
   smaller side to the larger side (`smaller / larger`).
2. **Check trigger conditions**: If the larger side exceeds `trigger_min_size` and
   the ratio falls below `trigger_imbalance_ratio`, a trigger fires.
3. **Determine direction**: If bid size > ask size, the strategy buys at the ask
   (anticipating upward pressure). If ask size > bid size, it sells at the bid.
4. **Submit FOK order**: A fill-or-kill limit order is submitted at the opposing
   best price, sized to the lesser of the opposing level size and `max_trade_size`.
5. **Cooldown**: A configurable minimum time between triggers prevents overtrading.

### Configuration

We use `use_quote_ticks=True` with `book_type="L1_MBP"` since our data is top-of-book
quotes rather than order book deltas:

```python
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig

strategy_config = OrderBookImbalanceConfig(
    instrument_id=instrument_id,
    max_trade_size=Decimal("10"),
    trigger_min_size=1.0,
    trigger_imbalance_ratio=0.10,
    min_seconds_between_triggers=5.0,
    book_type="L1_MBP",
    use_quote_ticks=True,
)

strategy = OrderBookImbalance(config=strategy_config)
```

| Parameter                      | Value    | Description                                   |
| ------------------------------ | -------- | --------------------------------------------- |
| `max_trade_size`               | `10`     | Maximum 10 contracts per order.               |
| `trigger_min_size`             | `1.0`    | Minimum 1 contract on the larger side.        |
| `trigger_imbalance_ratio`      | `0.10`   | Trigger when ratio drops below 10%.           |
| `min_seconds_between_triggers` | `5.0`    | 5-second cooldown between consecutive trades. |
| `book_type`                    | `L1_MBP` | Top-of-book data only.                        |
| `use_quote_ticks`              | `True`   | Drive the strategy from quote ticks.          |

:::tip
Start with conservative parameters (higher ratio, longer cooldown) and tighten them
as you study the backtest results. A `trigger_imbalance_ratio` of 0.10 means the smaller
side must be less than 10% of the larger side to trigger.
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

```python
engine.add_instrument(XAU_PERP)
engine.add_data(quotes)
engine.add_strategy(strategy)
```

### Run the backtest

```python
engine.run()
```

## Results

After the run completes, generate reports to analyze performance:

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

The complete script is available as
[`architect_ax_book_imbalance.py`](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/backtest/architect_ax_book_imbalance.py)
in the examples directory.

## Next steps

- **Tune parameters**: Experiment with `trigger_imbalance_ratio`, `min_seconds_between_triggers`,
  and `max_trade_size` to understand their effect on fill rates and PnL.
- **Try different data**: Download different time periods or different front-month contracts
  to see how the strategy performs across varying market conditions.
- **Go live on AX sandbox**: Once you are satisfied with backtest results, connect to the
  AX sandbox environment for paper trading. See the
  [AX Exchange integration guide](../integrations/architect_ax.md) for setup instructions.
- **Explore other instruments**: AX offers perpetuals on FX pairs (GBPUSD-PERP, EURUSD-PERP),
  metals (XAG-PERP), and more. Adapt this tutorial by downloading the corresponding CME
  futures data from Databento.

## Running live

The same strategy used in this backtest can be run live with no code changes - only a
launch script is needed. NautilusTrader's architecture means your strategy is
venue-agnostic: switching from backtest to live is a configuration change, not a rewrite.

See the complete live example:
[`ax_book_imbalance.py`](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/architect_ax/ax_book_imbalance.py)

For connection setup and API key configuration, refer to the
[AX Exchange integration guide](../integrations/architect_ax.md).

## Further reading

- [AX Exchange book imbalance backtest example](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/backtest/architect_ax_book_imbalance.py)
- [`OrderBookImbalance` strategy source](https://github.com/nautechsystems/nautilus_trader/tree/develop/nautilus_trader/examples/strategies/orderbook_imbalance.py)
- [Architect Exchange documentation](https://docs.architect.exchange/)
- [Databento: HFT signals with sklearn](https://databento.com/blog/hft-sklearn-python)
