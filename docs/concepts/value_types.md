# Value Types

NautilusTrader provides specialized value types for representing core trading concepts:
`Price`, `Quantity`, and `Money`. These types use fixed-point arithmetic internally
for performant, deterministic calculations across different platforms
and environments.

## Overview

| Type       | Purpose                                  | Signed | Currency |
| ---------- | ---------------------------------------- | ------ | -------- |
| `Quantity` | Trade sizes, order amounts, positions.   | No     | -        |
| `Price`    | Market prices, quotes, price levels.     | Yes    | -        |
| `Money`    | Monetary amounts, P&L, account balances. | Yes    | Yes      |

## Immutability

In Python, all value types are **immutable**. Once a value is constructed, it cannot be changed.
Operations do not mutate the original object.

```python
from nautilus_trader.model import Quantity

qty1 = Quantity(100, precision=0)
qty2 = Quantity(50, precision=0)

# This creates a NEW Quantity; qty1 and qty2 are unchanged
result = qty1 + qty2

print(qty1)  # 100
print(qty2)  # 50
print(result)  # 150
```

This design provides several benefits:

- **Thread safety**: Immutable values can be safely shared across threads without synchronization.
- **Predictability**: Values never change unexpectedly, making debugging easier.
- **Hashability**: Immutable types can be used as dictionary keys and in sets.

## Arithmetic operations

Value types support standard arithmetic operators (`+`, `-`, `*`, `/`, `%`, `//`)
and unary operators (`-`, `+`, `abs`). The return type depends on the operator
and the operand types.

### Same-type binary operations

Addition and subtraction of the same value type return that type, preserving
domain meaning (a price plus a price is still a price):

| Operation             | Result     |
| --------------------- | ---------- |
| `Quantity + Quantity` | `Quantity` |
| `Quantity - Quantity` | `Quantity` |
| `Price + Price`       | `Price`    |
| `Price - Price`       | `Price`    |
| `Money + Money`       | `Money`    |
| `Money - Money`       | `Money`    |

```python
from nautilus_trader.model import Price

price1 = Price(100.50, precision=2)
price2 = Price(0.25, precision=2)

result = price1 + price2  # Returns Price(100.75, precision=2)
print(type(result))  # <class 'nautilus_trader.model.Price'>
```

Multiplication, division, floor division, and modulo between two values of the
same type return `Decimal`:

| Operation        | Result    |
| ---------------- | --------- |
| `Price * Price`  | `Decimal` |
| `Price / Price`  | `Decimal` |
| `Price // Price` | `Decimal` |
| `Price % Price`  | `Decimal` |

The same pattern applies to `Quantity` and `Money`.

These operations return `Decimal` because the result has different dimensional meaning:

- Multiplying a price by a price produces "price squared", not a price.
- Dividing a quantity by a quantity produces a dimensionless ratio, not a quantity.

Returning `Decimal` makes the unit change explicit and prevents misinterpretation of the result as a value
with the original unit.

### Unary operations

Unary operators preserve the value type where the result is valid for that type:

| Operation  | `Price`   | `Quantity` | `Money`   |
| ---------- | --------- | ---------- | --------- |
| `-x` (neg) | `Price`   | `Decimal`  | `Money`   |
| `+x` (pos) | `Price`   | `Quantity` | `Money`   |
| `abs(x)`   | `Price`   | `Quantity` | `Money`   |
| `int(x)`   | `int`     | `int`      | `int`     |
| `float(x)` | `float`   | `float`    | `float`   |
| `round(x)` | `Decimal` | `Decimal`  | `Decimal` |

`Quantity.__neg__` returns `Decimal` rather than `Quantity` because `Quantity` is
unsigned and cannot represent a negative value.

```python
from nautilus_trader.model import Currency, Money, Price, Quantity

USD = Currency.from_str("USD")

price = Price(100.50, precision=2)
print(-price)  # -100.50
print(type(-price))  # <class 'nautilus_trader.model.Price'>

money = Money(-50.00, USD)
print(abs(money))  # 50.00 USD
print(type(abs(money)))  # <class 'nautilus_trader.model.Money'>

qty = Quantity(10, precision=0)
print(+qty)  # 10
print(type(+qty))  # <class 'nautilus_trader.model.Quantity'>
```

### Mixed-type operations

When operating with other numeric types, the result type follows Python's
[numeric tower](https://docs.python.org/3/library/numbers.html) conventions. Operations widen to the more
general type. `float` operations return `float`, while `int` and `Decimal` operations return `Decimal` for
precision preservation.

This applies to all six binary operators (`+`, `-`, `*`, `/`, `//`, `%`) and works
in both directions (`value op scalar` and `scalar op value`):

| Left operand | Right operand | Result type |
| ------------ | ------------- | ----------- |
| Value type   | `int`         | `Decimal`   |
| Value type   | `float`       | `float`     |
| Value type   | `Decimal`     | `Decimal`   |
| `int`        | Value type    | `Decimal`   |
| `float`      | Value type    | `float`     |
| `Decimal`    | Value type    | `Decimal`   |

```python
from decimal import Decimal

from nautilus_trader.model import Quantity

qty = Quantity(100, precision=0)

# Quantity + int -> Decimal
result1 = qty + 50
print(type(result1))  # <class 'decimal.Decimal'>

# Quantity + float -> float
result2 = qty + 50.5
print(type(result2))  # <class 'float'>

# Quantity + Decimal -> Decimal
result3 = qty + Decimal("50")
print(type(result3))  # <class 'decimal.Decimal'>
```

## Precision handling

`Price` and `Quantity` store an explicit, immutable `precision` field that indicates the number of decimal
places. `Money` uses its currency's precision for default rounding and minimum display precision. Its exact
constructors can retain finer amounts without changing the currency. The types have no "unspecified" precision.

### Fixed-point representation

Value types use scaled integers instead of floating-point numbers. The global scale is 10^9 in standard
mode and 10^16 in high-precision mode. DeFi values with higher supported precision use that precision as
their scale. For `Price` and `Quantity`, the explicit precision controls display formatting and serialization.
`Money` displays at least its currency precision and retains finer stored digits.

```python
from nautilus_trader.model import Price

p1 = Price(1.23, precision=2)  # displays as "1.23"
p2 = Price(1.230, precision=3)  # displays as "1.230"

p1 == p2  # True: same underlying value
str(p1)  # "1.23"
str(p2)  # "1.230"
```

**Precision and identity**: two prices with the same decimal value but different precisions are equal.
Precision controls display, not identity. The `precision` field determines string formatting and how many
decimal places are shown. Equality is based on the underlying numeric value.

**Market data serialization**: when market data types (such as quotes, trades, and order book deltas) are
written to Parquet or Arrow, file metadata stores their precision. This lets readers decode the values
correctly. All market data values within a single file must share the same precision.

:::warning
If a venue changes an instrument's tick size (and thus its precision), data files written before and after the
change have different precision metadata. Do not consolidate them into a single file.
:::

For how instrument-level precision constrains valid prices and quantities, see the
[Precision](instruments/index.md#precision) section of the Instruments guide.

### Arithmetic precision

When performing arithmetic between values with different precisions, the result
uses the maximum precision of the operands.

```python
from nautilus_trader.model import Price

price1 = Price(100.5, precision=1)  # 1 decimal place
price2 = Price(0.125, precision=3)  # 3 decimal places

result = price1 + price2
print(result)  # 100.625
print(result.precision)  # 3 (max of 1 and 3)
```

## Type-specific constraints

### Quantity

`Quantity` represents non-negative amounts. Attempting to create a negative quantity
or subtract a larger quantity from a smaller one raises an error:

```python
from nautilus_trader.model import Quantity

# This raises ValueError: Quantity cannot be negative
qty = Quantity(-100, precision=0)

# This also raises ValueError
qty1 = Quantity(50, precision=0)
qty2 = Quantity(100, precision=0)
result = qty1 - qty2  # Would be -50, which is invalid
```

### Money

`Money` values include a currency. Addition and subtraction between `Money` values
require **matching currencies**:

```python
from nautilus_trader.model import Currency, Money

USD = Currency.from_str("USD")
EUR = Currency.from_str("EUR")

usd_amount = Money(100.00, USD)
eur_amount = Money(50.00, EUR)

# This works - same currency
result = usd_amount + Money(25.00, USD)

# This raises ValueError - currency mismatch
result = usd_amount + eur_amount
```

Use `Money.from_decimal_exact` or `Money.from_str_exact` to preserve amounts finer than the currency precision:

```python
from decimal import Decimal

from nautilus_trader.model import Currency, Money

USD = Currency.from_str("USD")
exact = Money.from_decimal_exact(Decimal("1.0001"), USD)

assert str(exact) == "1.0001 USD"
assert exact.currency.precision == 2
assert Money.from_str("1.0001 USD") == Money.from_str("1.00 USD")
```

Exact constructors reject precision loss and out-of-range amounts. USD supports 9 decimal places in standard
mode and 16 in high-precision mode. DeFi currencies use their currency scale when it is higher.
`Money.from_decimal_exact` requires a `Decimal` and raises `TypeError` for other types. `Money(...)`,
`Money.from_str`, and `Money.from_decimal` round to the currency precision.
String serialization preserves stored finer digits; deserialization restores them without currency rounding.

## Common patterns

### Accumulating values

Since value types are immutable, accumulate by reassigning:

```python
from nautilus_trader.model import Currency, Money

USD = Currency.from_str("USD")

total = Money(0.00, USD)
amounts = [Money(100.00, USD), Money(50.00, USD), Money(25.00, USD)]

for amount in amounts:
    total = total + amount  # Reassign to new Money instance

print(total)  # 175.00 USD
```

### Converting to other types

Value types provide conversion methods:

```python
from nautilus_trader.model import Price

price = Price(123.456, precision=3)

# Convert to Decimal (preserves precision)
decimal_value = price.as_decimal()

# Convert to float
float_value = price.as_double()

# Convert to string
string_value = str(price)  # "123.456"
```

Float conversion supports precision up to 16. `float()`, `as_double()`, and arithmetic with a
`float` operand raise `ValueError` for precision 17 or 18 rather than round away the extra digits.
Conversions at precision 16 or below can still round, since a float holds about 16 significant
digits. Use `as_decimal()` to avoid binary-float conversion. The conversion uses Rust `Decimal`, whose
96-bit mantissa can lose fractional digits when a value has too many significant digits. String
serialization preserves the full stored `Money` amount.

### Creating from strings

Parse value types from string representations:

```python
from nautilus_trader.model import Money, Price, Quantity

qty = Quantity.from_str("100.5")
price = Price.from_str("99.95")
money = Money.from_str("1000.00 USD")
```
