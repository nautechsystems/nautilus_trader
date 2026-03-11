from __future__ import annotations

from decimal import Decimal
from decimal import getcontext
from typing import Protocol


ZERO = Decimal(0)
TWO = Decimal(2)

# Decimal sqrt chains need higher precision than the default context.
getcontext().prec = max(getcontext().prec, 50)


class PoolPriceSource(Protocol):
    def get_price_token1_per_token0(self, pool_address: str) -> Decimal: ...


def resolve_pool_price_token1_per_token0(
    price_helper: object,
    *,
    pool_address: str,
) -> Decimal:
    getter = getattr(price_helper, "get_price_token1_per_token0", None)
    if callable(getter):
        return Decimal(str(getter(pool_address)))

    legacy_getter = getattr(price_helper, "get_price_plume_per_eth", None)
    if callable(legacy_getter):
        return Decimal(str(legacy_getter(pool_address)))

    raise AttributeError("price helper must expose get_price_token1_per_token0()")


def price_to_sqrt_price(price: Decimal) -> Decimal:
    price = Decimal(price)
    if price <= ZERO:
        raise ValueError("price must be positive")
    return price.sqrt()


def compute_liquidity_from_initial_deposit(
    amount_token0: Decimal,
    amount_token1: Decimal,
    sqrt_price_lower: Decimal,
    sqrt_price_upper: Decimal,
) -> Decimal:
    amount_token0 = Decimal(amount_token0)
    amount_token1 = Decimal(amount_token1)
    sqrt_price_lower = Decimal(sqrt_price_lower)
    sqrt_price_upper = Decimal(sqrt_price_upper)

    if sqrt_price_lower <= ZERO or sqrt_price_upper <= sqrt_price_lower:
        raise ValueError("invalid sqrt price bounds")
    if amount_token0 < ZERO or amount_token1 < ZERO:
        raise ValueError("token amounts must be non-negative")
    if amount_token0 == ZERO and amount_token1 == ZERO:
        raise ValueError("at least one token amount must be positive")

    span = sqrt_price_upper - sqrt_price_lower
    if amount_token1 == ZERO:
        return amount_token0 * sqrt_price_lower * sqrt_price_upper / span
    if amount_token0 == ZERO:
        return amount_token1 / span

    a = amount_token0 * sqrt_price_upper
    b = amount_token1 - amount_token0 * sqrt_price_upper * sqrt_price_lower
    c = -amount_token1 * sqrt_price_upper

    discriminant = b * b - Decimal(4) * a * c
    if discriminant <= ZERO:
        raise ValueError("invalid deposit combination for liquidity solve")

    sqrt_discriminant = discriminant.sqrt()
    two_a = TWO * a
    candidates = (
        (-b + sqrt_discriminant) / two_a,
        (-b - sqrt_discriminant) / two_a,
    )
    for candidate in candidates:
        if sqrt_price_lower < candidate < sqrt_price_upper:
            return amount_token1 / (candidate - sqrt_price_lower)

    raise ValueError("could not infer sqrt price from deposits")


def compute_amounts_from_liquidity(
    liquidity: Decimal,
    sqrt_price: Decimal,
    sqrt_price_lower: Decimal,
    sqrt_price_upper: Decimal,
) -> tuple[Decimal, Decimal]:
    liquidity = Decimal(liquidity)
    sqrt_price = Decimal(sqrt_price)
    sqrt_price_lower = Decimal(sqrt_price_lower)
    sqrt_price_upper = Decimal(sqrt_price_upper)

    if liquidity < ZERO:
        raise ValueError("liquidity must be non-negative")
    if sqrt_price_lower <= ZERO or sqrt_price_upper <= sqrt_price_lower:
        raise ValueError("invalid sqrt price bounds")
    if sqrt_price <= ZERO:
        raise ValueError("sqrt price must be positive")

    span = sqrt_price_upper - sqrt_price_lower
    if sqrt_price <= sqrt_price_lower:
        amount_token0 = liquidity * span / (sqrt_price_lower * sqrt_price_upper)
        return amount_token0, ZERO
    if sqrt_price >= sqrt_price_upper:
        amount_token1 = liquidity * span
        return ZERO, amount_token1

    amount_token0 = liquidity * (sqrt_price_upper - sqrt_price) / (
        sqrt_price * sqrt_price_upper
    )
    amount_token1 = liquidity * (sqrt_price - sqrt_price_lower)
    return amount_token0, amount_token1


__all__ = [
    "PoolPriceSource",
    "compute_amounts_from_liquidity",
    "compute_liquidity_from_initial_deposit",
    "price_to_sqrt_price",
    "resolve_pool_price_token1_per_token0",
]
