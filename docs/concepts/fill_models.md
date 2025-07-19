# FillModel - Advanced Order Fill Simulation

## Overview

The enhanced FillModel functionality provides complete control over order fill simulation during backtesting by allowing custom fill models to return simulated OrderBooks that represent expected market liquidity. This enables sophisticated simulation of various market conditions and behaviors while maintaining backward compatibility with existing code.

## Key Features

- **Complete Control**: Define exactly how orders should be filled under any market condition
- **Backward Compatibility**: Existing FillModel usage continues to work unchanged
- **Unified Processing**: All fill simulation uses the same L2 OrderBook processing logic
- **Easy to Understand**: Simple interface - return an OrderBook or None for default behavior
- **Flexible**: Supports any imaginable order-fill scenario

## How It Works

### Core Concept

The enhanced FillModel introduces a single new method:

```python
def get_orderbook_for_fill_simulation(
    self,
    instrument: Instrument,
    order: Order,
    best_bid: Price,
    best_ask: Price,
) -> Optional[OrderBook]:
    """
    Return a simulated OrderBook for fill simulation.

    Returns None to use default probabilistic logic (backward compatibility).
    Returns OrderBook to use custom fill simulation.
    """
```

### Integration Points

The matching engine calls this method at two key points:

1. **Market Order Fills** (`fill_market_order`) - When processing market orders that consume liquidity
2. **Limit Order Fills** (`fill_limit_order`) - When processing limit orders that provide liquidity

If the method returns `None`, the engine falls back to the existing probabilistic fill logic. If it returns an `OrderBook`, the engine uses that simulated book to determine fills using the standard L2 processing.

## Available Fill Models

### 1. BestPriceFillModel
**Use Case**: Optimistic market conditions testing

```python
from nautilus_trader.backtest.fill_models import BestPriceFillModel
fill_model = BestPriceFillModel()
```

- Provides unlimited liquidity at best bid/ask prices
- Every order fills immediately at the best available price
- Ideal for testing basic strategy logic without market impact

### 2. OneTickSlippageFillModel
**Use Case**: Guaranteed slippage simulation

```python
from nautilus_trader.backtest.fill_models import OneTickSlippageFillModel
fill_model = OneTickSlippageFillModel()
```

- Forces exactly one tick of slippage for all orders
- Zero volume at best prices, unlimited volume one tick away
- Deterministic slippage behavior

### 3. TwoTierFillModel
**Use Case**: Basic market depth simulation

```python
from nautilus_trader.backtest.fill_models import TwoTierFillModel
fill_model = TwoTierFillModel()
```

- First 10 contracts at best price
- Remainder at one tick worse price
- Simulates basic market impact

### 4. ThreeTierFillModel
**Use Case**: Realistic market depth

```python
from nautilus_trader.backtest.fill_models import ThreeTierFillModel
fill_model = ThreeTierFillModel()
```

- 50 contracts at best price
- 30 contracts one tick worse
- 20 contracts two ticks worse
- Progressive fill distribution

### 5. SizeAwareFillModel
**Use Case**: Order size-dependent execution

```python
from nautilus_trader.backtest.fill_models import SizeAwareFillModel
fill_model = SizeAwareFillModel()
```

- Small orders (≤10): Good liquidity at best prices
- Large orders: Price impact with partial fills at worse prices
- Realistic size-based market impact

### 6. ProbabilisticFillModel
**Use Case**: Replicating current FillModel behavior

```python
from nautilus_trader.backtest.fill_models import ProbabilisticFillModel
fill_model = ProbabilisticFillModel()
```

- 50% chance of fill at best price
- 50% chance of one tick slippage
- Demonstrates how to implement existing probabilistic behavior

### 7. LimitOrderPartialFillModel
**Use Case**: Limit order queue simulation

```python
from nautilus_trader.backtest.fill_models import LimitOrderPartialFillModel
fill_model = LimitOrderPartialFillModel()
```

- Maximum 5 contracts fill when price touches limit
- Models typical limit order queue behavior
- Simulates price-time priority effects

### 8. MarketHoursFillModel
**Use Case**: Time-dependent liquidity

```python
from nautilus_trader.backtest.fill_models import MarketHoursFillModel
fill_model = MarketHoursFillModel()
fill_model.set_low_liquidity_period(True)  # For testing
```

- Normal hours: Standard liquidity at best prices
- Low liquidity periods: Wider spreads (one tick worse)
- Essential for strategies trading across different sessions

### 9. VolumeSensitiveFillModel
**Use Case**: Volume-based liquidity

```python
from nautilus_trader.backtest.fill_models import VolumeSensitiveFillModel
fill_model = VolumeSensitiveFillModel()
fill_model.set_recent_volume(2000.0)  # For testing
```

- Available liquidity = 25% of recent trading volume
- Scales market depth based on actual market activity
- More realistic liquidity modeling

### 10. CompetitionAwareFillModel
**Use Case**: Market competition simulation

```python
from nautilus_trader.backtest.fill_models import CompetitionAwareFillModel
fill_model = CompetitionAwareFillModel(liquidity_factor=0.3)
```

- Only 30% of visible liquidity actually available
- Simulates competition from other market participants
- Reflects realistic trading conditions

## Creating Custom Fill Models

### Basic Template

```python
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import BookOrder
from nautilus_trader.core.rust.model import BookType
from typing import Optional

class CustomFillModel(FillModel):
    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> Optional[OrderBook]:
        # Create custom OrderBook
        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Add custom liquidity levels
        bid_order = BookOrder(
            side=OrderSide.BUY,
            price=best_bid,
            size=Quantity(100, instrument.size_precision),
            order_id=1,
        )

        book.add(bid_order, 0, 0)
        return book
```

### Advanced Example: Dynamic Spread Model

```python
class DynamicSpreadFillModel(FillModel):
    def __init__(self, base_spread_factor=1.0, volatility_factor=0.1):
        super().__init__()
        self.base_spread_factor = base_spread_factor
        self.volatility_factor = volatility_factor

    def get_orderbook_for_fill_simulation(
        self,
        instrument: Instrument,
        order: Order,
        best_bid: Price,
        best_ask: Price,
    ) -> Optional[OrderBook]:
        # Calculate dynamic spread based on market conditions
        base_spread = best_ask.as_double() - best_bid.as_double()
        dynamic_spread = base_spread * (1 + self.volatility_factor)

        # Adjust prices
        mid_price = (best_bid.as_double() + best_ask.as_double()) / 2
        new_bid = Price(mid_price - dynamic_spread/2, instrument.price_precision)
        new_ask = Price(mid_price + dynamic_spread/2, instrument.price_precision)

        # Create OrderBook with dynamic spread
        book = OrderBook(instrument_id=instrument.id, book_type=BookType.L2_MBP)

        # Add liquidity at dynamic prices
        # ... implementation details

        return book
```

## Integration with Backtesting

### Basic Usage

```python
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.fill_models import OneTickSlippageFillModel

# Create custom fill model
fill_model = OneTickSlippageFillModel()

# Configure and run backtest
config = BacktestEngineConfig()
engine = BacktestEngine(config=config)

engine.add_venue(
    venue=venue,
    oms_type=OmsType.HEDGING,
    account_type=AccountType.MARGIN,
    base_currency=USD,
    starting_balances=[Money(1_000_000, USD)],
    fill_model=fill_model,  # Use custom fill model
)

engine.run()
```

### Per-Instrument Fill Models

```python
# Different fill models for different instruments
forex_fill_model = TwoTierFillModel()
crypto_fill_model = OneTickSlippageFillModel()

# Apply to specific venues/instruments
engine.add_venue(venue_forex, fill_model=forex_fill_model)
engine.add_venue(venue_crypto, fill_model=crypto_fill_model)
```

## Migration Guide

### Existing Code Compatibility

All existing FillModel usage continues to work unchanged:

```python
# This still works exactly as before
fill_model = FillModel(
    prob_fill_on_limit=0.2,
    prob_slippage=0.5,
    random_seed=42
)
```

The new functionality is completely opt-in - only custom fill models that implement `get_orderbook_for_fill_simulation` will use the new behavior.

### Migrating from Probabilistic to Deterministic

```python
# Old probabilistic approach
old_model = FillModel(prob_slippage=0.5)

# New deterministic equivalent
new_model = ProbabilisticFillModel()  # Implements same 50% logic

# Or fully deterministic
deterministic_model = OneTickSlippageFillModel()  # Always slips
```

## Best Practices

1. **Start Simple**: Begin with existing models like `BestPriceFillModel` or `TwoTierFillModel`
2. **Test Thoroughly**: Verify fill behavior matches your expectations
3. **Consider Market Conditions**: Use different models for different market regimes
4. **Validate Results**: Compare with real market data when possible
5. **Document Assumptions**: Clearly document the market assumptions in your custom models

## Performance Considerations

- OrderBook creation is lightweight and fast
- Fill simulation uses the same optimized L2 processing as real market data
- Custom models should avoid heavy computations in the simulation method
- Consider caching complex calculations outside the simulation method

## Conclusion

The enhanced FillModel functionality provides unprecedented control over order fill simulation while maintaining simplicity and backward compatibility. Whether you need basic market impact modeling or sophisticated multi-level liquidity simulation, the new system can accommodate any requirement through its flexible OrderBook-based approach.
