# Value Types

NautilusTrader provides specialized value types for representing core trading concepts:
`Price`, `Quantity`, and `Money`. These types use fixed-point arithmetic internally
to ensure highly performant and deterministic calculations across different platforms
and environments.

## Overview

| Type       | Purpose                                  | Signed | Currency |
|------------|------------------------------------------|--------|----------|
| `Quantity` | Trade sizes, order amounts, positions.   | No     | -        |
| `Price`    | Market prices, quotes, price levels.     | Yes    | -        |
| `Money`    | Monetary amounts, P&L, account balances. | Yes    | Yes      |

## Immutability

All value types are **immutable**. Once a value is constructed, it cannot be changed.
Arithmetic operations always return new instances rather than modifying existing ones.

```python
from nautilus_trader.model.objects import Quantity

qty1 = Quantity(100, precision=0)
qty2 = Quantity(50, precision=0)

# This creates a NEW Quantity; qty1 and qty2 are unchanged
result = qty1 + qty2

print(qty1)    # 100
print(qty2)    # 50
print(result)  # 150
```

This design provides several benefits:

- **Thread safety**: Immutable values can be safely shared across threads without synchronization.
- **Predictability**: Values never change unexpectedly, making debugging easier.
- **Hashability**: Immutable types can be used as dictionary keys and in sets.

## Arithmetic operations

Value types support standard arithmetic operators (`+`, `-`, `*`, `/`, `%`, `//`).
The return type depends on the operand types.

### Same-type operations

When both operands are the same value type, the result is also that type:

| Operation             | Result     |
|-----------------------|------------|
| `Quantity + Quantity` | `Quantity` |
| `Quantity - Quantity` | `Quantity` |
| `Price + Price`       | `Price`    |
| `Price - Price`       | `Price`    |
| `Money + Money`       | `Money`    |
| `Money - Money`       | `Money`    |

```python
from nautilus_trader.model.objects import Price

price1 = Price(100.50, precision=2)
price2 = Price(0.25, precision=2)

result = price1 + price2  # Returns Price(100.75, precision=2)
print(type(result))       # <class 'Price'>
```

### Mixed-type operations

When operating with other numeric types, the result type follows Python's
[numeric tower](https://docs.python.org/3/library/numbers.html) conventions. The general
principle is that operations widen to the more general type: `float` operations return
`float`, while `int` and `Decimal` operations return `Decimal` for precision preservation.

| Left operand | Right operand | Result type |
|--------------|---------------|-------------|
| Value type   | `int`         | `Decimal`   |
| Value type   | `float`       | `float`     |
| Value type   | `Decimal`     | `Decimal`   |
| `int`        | Value type    | `Decimal`   |
| `float`      | Value type    | `float`     |
| `Decimal`    | Value type    | `Decimal`   |

```python
from decimal import Decimal
from nautilus_trader.model.objects import Quantity

qty = Quantity(100, precision=0)

# Quantity + int → Decimal
result1 = qty + 50
print(type(result1))  # <class 'decimal.Decimal'>

# Quantity + float → float
result2 = qty + 50.5
print(type(result2))  # <class 'float'>

# Quantity + Decimal → Decimal
result3 = qty + Decimal("50")
print(type(result3))  # <class 'decimal.Decimal'>
```

## Precision handling

Each value type stores a precision field indicating the number of decimal places.
Precision is set at construction and is immutable. There is no "unspecified" precision.

### Fixed-point representation

Value types are stored internally as integers scaled to a global fixed precision
(e.g., 10^16 in high-precision mode), not floating-point numbers. The `precision`
field tracks the number of decimal places used at construction, controlling display
formatting and serialization, but the underlying raw value always uses the global scale.

```python
from nautilus_trader.model.objects import Price

p1 = Price(1.23, precision=2)   # displays as "1.23"
p2 = Price(1.230, precision=3)  # displays as "1.230"

p1 == p2  # True: same underlying value
str(p1)   # "1.23"
str(p2)   # "1.230"
```

**Precision controls display, not identity.** Two prices with the same decimal value but
different precisions are equal. The `precision` field determines string formatting and
how many decimal places are shown, but equality is based on the underlying numeric value.

**Market data serialization uses precision metadata.** When market data types (quotes,
trades, order book deltas) are written to Parquet or Arrow format, precision is stored in
the file metadata so that values can be correctly decoded. All market data values within
a single file must share the same precision.

:::note
If a venue changes an instrument's tick size (and thus its precision), data files written
before and after the change will have different precision metadata and should not be
consolidated into a single file.
:::

For how instrument-level precision constrains valid prices and quantities, see the
[Precision](instruments.md#precision) section of the Instruments guide.

### Arithmetic precision

When performing arithmetic between values with different precisions, the result
uses the maximum precision of the operands.

```python
from nautilus_trader.model.objects import Price

price1 = Price(100.5, precision=1)    # 1 decimal place
price2 = Price(0.125, precision=3)    # 3 decimal places

result = price1 + price2
print(result)            # 100.625
print(result.precision)  # 3 (max of 1 and 3)
```

## Type-specific constraints

### Quantity

`Quantity` represents non-negative amounts. Attempting to create a negative quantity
or subtract a larger quantity from a smaller one raises an error:

```python
from nautilus_trader.model.objects import Quantity

# This raises ValueError: Quantity cannot be negative
qty = Quantity(-100, precision=0)

# This also raises ValueError
qty1 = Quantity(50, precision=0)
qty2 = Quantity(100, precision=0)
result = qty1 - qty2  # Would be -50, which is invalid
```

### Money

`Money` values include a currency. Arithmetic between `Money` values requires
matching currencies:

```python
from nautilus_trader.model.objects import Money
from nautilus_trader.model.currencies import USD, EUR

usd_amount = Money(100.00, USD)
eur_amount = Money(50.00, EUR)

# This works - same currency
result = usd_amount + Money(25.00, USD)

# This raises ValueError - currency mismatch
result = usd_amount + eur_amount
```

## Common patterns

### Accumulating values

Since value types are immutable, accumulate by reassigning:

```python
from nautilus_trader.model.objects import Money
from nautilus_trader.model.currencies import USD

total = Money(0.00, USD)
amounts = [Money(100.00, USD), Money(50.00, USD), Money(25.00, USD)]

for amount in amounts:
    total = total + amount  # Reassign to new Money instance

print(total)  # 175.00 USD
```

### Converting to other types

Value types provide conversion methods:

```python
from nautilus_trader.model.objects import Price

price = Price(123.456, precision=3)

# Convert to Decimal (preserves precision)
decimal_value = price.as_decimal()

# Convert to float
float_value = price.as_double()

# Convert to string
string_value = str(price)  # "123.456"
```

### Creating from strings

Parse value types from string representations:

```python
from nautilus_trader.model.objects import Quantity, Price, Money

qty = Quantity.from_str("100.5")
price = Price.from_str("99.95")
money = Money.from_str("1000.00 USD")
```
