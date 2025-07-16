# Margin Models

Nautilus Trader provides flexible margin calculation models to accommodate different venue types and trading scenarios. This addresses the issue where leverage incorrectly reduced margin requirements in the original implementation.

## Problem Statement

The original margin calculation implementation incorrectly reduced margin requirements based on leverage:

```python
# INCORRECT (original implementation)
adjusted_notional = notional / leverage
margin = adjusted_notional * instrument.margin_init
```

This doesn't match real-world broker behavior. For example:

- **Interactive Brokers**: CME 6E Futures require $3,000 per contract regardless of leverage
- **Traditional Brokers**: Fixed margin percentages independent of account leverage
- **Leverage Effect**: Affects buying power, not margin requirements

## Overview

Different brokers and exchanges have varying approaches to calculating margin requirements:

- **Traditional Brokers** (Interactive Brokers, TD Ameritrade): Fixed margin percentages regardless of leverage
- **Crypto Exchanges** (Binance, some others): Leverage may reduce margin requirements
- **Futures Exchanges** (CME, ICE): Fixed margin amounts per contract

## Available Models

### StandardMarginModel

Uses fixed percentages without leverage division, matching traditional broker behavior.

**Formula:**

```python
# Fixed percentages - leverage ignored
margin = notional * instrument.margin_init
```

- Initial Margin = `notional_value * instrument.margin_init`
- Maintenance Margin = `notional_value * instrument.margin_maint`

**Use Cases:**

- Traditional brokers (Interactive Brokers, TD Ameritrade)
- Futures exchanges (CME, ICE)
- Forex brokers with fixed margin requirements

### LeveragedMarginModel

Divides margin requirements by leverage (current Nautilus behavior).

**Formula:**

```python
# Leverage reduces margin requirements
adjusted_notional = notional / leverage
margin = adjusted_notional * instrument.margin_init
```

- Initial Margin = `(notional_value / leverage) * instrument.margin_init`
- Maintenance Margin = `(notional_value / leverage) * instrument.margin_maint`

**Use Cases:**

- Crypto exchanges that reduce margin with leverage
- Backward compatibility with existing strategies
- Specific venues where leverage affects margin requirements

## Usage

### Programmatic Configuration

```python
from nautilus_trader.accounting.margin_models import StandardMarginModel, LeveragedMarginModel
from nautilus_trader.test_kit.stubs.execution import TestExecStubs

# Create account
account = TestExecStubs.margin_account()

# Set standard model for traditional brokers
standard_model = StandardMarginModel()
account.set_margin_model(standard_model)

# Or use leveraged model for crypto exchanges
leveraged_model = LeveragedMarginModel()
account.set_margin_model(leveraged_model)
```

### Backtest Configuration

```python
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.accounting.margin_config import MarginModelConfig

venue_config = BacktestVenueConfig(
    name="SIM",
    oms_type="NETTING",
    account_type="MARGIN",
    starting_balances=["1_000_000 USD"],
    margin_model=MarginModelConfig(model_type="standard"),  # Options: 'standard', 'leveraged'
)
```

### Available Model Types

- `"leveraged"`: Current Nautilus behavior (margin reduced by leverage)
- `"standard"`: Fixed percentages (traditional brokers)
- Custom class path: `"my_package.my_module.MyMarginModel"`

### Default Behavior

By default, `MarginAccount` uses `LeveragedMarginModel` for backward compatibility with existing strategies.

### Real-World Example

**EUR/USD Trading Scenario:**

- **Instrument**: EUR/USD
- **Quantity**: 100,000 EUR
- **Price**: 1.10000
- **Notional Value**: $110,000
- **Leverage**: 50x
- **Instrument Margin Init**: 3%

**Margin Calculations:**

| Model | Calculation | Result | Percentage |
|-------|-------------|--------|------------|
| Standard | $110,000 × 0.03 | $3,300 | 3.00% |
| Leveraged | ($110,000 ÷ 50) × 0.03 | $66 | 0.06% |

**Account Balance Impact:**

- **Account Balance**: $10,000
- **Standard Model**: Cannot trade (requires $3,300 margin)
- **Leveraged Model**: Can trade (requires only $66 margin)

## Real-World Scenarios

### Interactive Brokers EUR/USD Futures

```python
# IB requires fixed margin regardless of leverage
account.set_margin_model(StandardMarginModel())
margin = account.calculate_margin_init(instrument, quantity, price)
# Result: Fixed percentage of notional value
```

### Binance Crypto Trading

```python
# Binance may reduce margin with leverage
account.set_margin_model(LeveragedMarginModel())
margin = account.calculate_margin_init(instrument, quantity, price)
# Result: Margin reduced by leverage factor
```

## Migration Guide

### From Current Implementation

Existing code continues to work unchanged as `LeveragedMarginModel` is the default:

```python
# This continues to work as before
account = TestExecStubs.margin_account()
margin = account.calculate_margin_init(instrument, quantity, price)
```

### To Standard Model

For traditional broker behavior:

```python
# Switch to standard model
account.set_margin_model(StandardMarginModel())
margin = account.calculate_margin_init(instrument, quantity, price)
# Now uses fixed percentages
```

## Custom Models

You can create custom margin models by inheriting from `MarginModel`. Custom models receive configuration through the `MarginModelConfig`:

```python
from nautilus_trader.accounting.margin_models import MarginModel
from nautilus_trader.accounting.margin_config import MarginModelConfig

class RiskAdjustedMarginModel(MarginModel):
    def __init__(self, config: MarginModelConfig):
        """Initialize with configuration parameters."""
        self.risk_multiplier = Decimal(str(config.config.get("risk_multiplier", 1.0)))
        self.use_leverage = config.config.get("use_leverage", False)

    def calculate_margin_init(self, instrument, quantity, price, leverage, use_quote_for_inverse=False):
        notional = instrument.notional_value(quantity, price, use_quote_for_inverse)
        if self.use_leverage:
            adjusted_notional = notional.as_decimal() / leverage
        else:
            adjusted_notional = notional.as_decimal()
        margin = adjusted_notional * instrument.margin_init * self.risk_multiplier
        return Money(margin, instrument.quote_currency)

    def calculate_margin_maint(self, instrument, side, quantity, price, leverage, use_quote_for_inverse=False):
        return self.calculate_margin_init(instrument, quantity, price, leverage, use_quote_for_inverse)
```

### Using Custom Models

**Programmatic:**

```python
from nautilus_trader.accounting.margin_config import MarginModelConfig, MarginModelFactory

config = MarginModelConfig(
    model_type="my_package.my_module:RiskAdjustedMarginModel",
    config={"risk_multiplier": 1.5, "use_leverage": False}
)

custom_model = MarginModelFactory.create(config)
account.set_margin_model(custom_model)
```

## High-Level Backtest API Configuration

When using the high-level backtest API, you can specify margin models in your venue configuration using `MarginModelConfig`:

```python
from nautilus_trader.accounting.margin_config import MarginModelConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.config import BacktestRunConfig

# Configure venue with specific margin model
venue_config = BacktestVenueConfig(
    name="SIM",
    oms_type="NETTING",
    account_type="MARGIN",
    starting_balances=["1_000_000 USD"],
    margin_model=MarginModelConfig(
        model_type="standard"  # Use standard model for traditional broker simulation
    ),
)

# Use in backtest configuration
config = BacktestRunConfig(
    venues=[venue_config],
    # ... other config
)
```

### Configuration Examples

**Standard Model (Traditional Brokers):**

```python
margin_model=MarginModelConfig(model_type="standard")
```

**Leveraged Model (Current Nautilus Behavior):**

```python
margin_model=MarginModelConfig(model_type="leveraged")  # Default
```

**Custom Model with Configuration:**

```python
margin_model=MarginModelConfig(
    model_type="my_package.my_module:CustomMarginModel",
    config={
        "risk_multiplier": 1.5,
        "use_leverage": False,
        "volatility_threshold": 0.02,
    }
)
```

The margin model will be automatically applied to the simulated exchange during backtest execution.

## Implementation Details

### Core Architecture

- **Abstract Base Class**: `MarginModel` with pluggable calculation strategies
- **Integration**: `MarginAccount` delegates calculations to selected model
- **Configuration**: Support for both programmatic and configuration-driven setup
- **Backward Compatibility**: Default to `LeveragedMarginModel` for existing code

### Test Coverage

- **11 new tests** for margin model functionality
- **26 existing tests** continue to pass (backward compatibility)
- **Integration tests** with MarginAccount
- **Configuration tests** for backtest setup

## Recommendations

- **Use StandardMarginModel** for traditional brokers and futures exchanges
- **Use LeveragedMarginModel** for crypto exchanges or backward compatibility
- **Test thoroughly** when switching models as margin requirements will change
- **Consider account balance** requirements when changing from leveraged to standard models
