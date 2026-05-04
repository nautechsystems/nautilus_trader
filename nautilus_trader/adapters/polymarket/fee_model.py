# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Provides a Polymarket-aware fee model for backtests, including maker rebates.
"""

from __future__ import annotations

from collections.abc import Iterable
from collections.abc import Mapping
from decimal import ROUND_HALF_UP
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.polymarket.common.parsing import calculate_commission
from nautilus_trader.backtest.config import FeeModelConfig
from nautilus_trader.backtest.models import FeeModel
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order


_CRYPTO_MAKER_REBATE_RATE = Decimal("0.20")
_DEFAULT_FEE_ENABLED_MAKER_REBATE_RATE = Decimal("0.25")
_REBATE_QUANTUM = Decimal("0.00001")

_CRYPTO_LABELS: frozenset[str] = frozenset({"crypto"})
_FEE_ENABLED_NON_CRYPTO_LABELS: frozenset[str] = frozenset(
    {
        "culture",
        "economics",
        "finance",
        "general",
        "mentions",
        "other",
        "other general",
        "politics",
        "sports",
        "tech",
        "weather",
    },
)

# Fallback classification when category labels are absent. Rates match
# Polymarket's documented schedule at docs.polymarket.com/trading/fees.
_CRYPTO_FEE_RATES: frozenset[Decimal] = frozenset({Decimal("0.072")})
_NON_CRYPTO_FEE_RATES: frozenset[Decimal] = frozenset(
    {
        Decimal("0.030"),
        Decimal("0.040"),
        Decimal("0.050"),
    },
)


def _normalize_label(value: object) -> str | None:
    if value is None:
        return None
    label = str(value).strip().casefold()
    if not label:
        return None
    return " ".join(label.replace("_", " ").replace("-", " ").split())


def _iter_tag_labels(tags: object) -> Iterable[str]:
    if isinstance(tags, str):
        label = _normalize_label(tags)
        if label is not None:
            yield label
        return

    if not isinstance(tags, Iterable):
        return

    for tag in tags:
        if isinstance(tag, Mapping):
            for key in ("label", "name", "slug", "title"):
                label = _normalize_label(tag.get(key))
                if label is not None:
                    yield label
        else:
            label = _normalize_label(tag)
            if label is not None:
                yield label


_LABEL_FIELDS: tuple[str, ...] = ("category", "category_slug", "tag", "tag_slug")
_TAG_LIST_FIELDS: tuple[str, ...] = ("categories", "tags")


def _collect_labels(source: Mapping[str, Any], labels: set[str]) -> None:
    for key in _LABEL_FIELDS:
        label = _normalize_label(source.get(key))
        if label is not None:
            labels.add(label)

    for key in _TAG_LIST_FIELDS:
        labels.update(_iter_tag_labels(source.get(key)))


def _market_labels(info: Mapping[str, Any] | None) -> set[str]:
    if not info:
        return set()

    labels: set[str] = set()
    _collect_labels(info, labels)

    raw_events = info.get("events")
    if isinstance(raw_events, Iterable) and not isinstance(raw_events, (str, bytes)):
        for event in raw_events:
            if isinstance(event, Mapping):
                _collect_labels(event, labels)

    return labels


def infer_maker_rebate_rate(
    market_info: Mapping[str, Any] | None,
    fee_rate: Decimal,
) -> Decimal:
    """
    Infer the maker rebate share for a fee-enabled Polymarket market.

    Polymarket's current fee schedule pays a 20% maker rebate for crypto markets
    and 25% for other fee-enabled categories. Fee-free or unclassified markets
    receive no rebate credit because there is no reliable way to derive a
    rebate share.

    Parameters
    ----------
    market_info : Mapping[str, Any] or None
        The instrument info payload. Used to read category and tag labels.
    fee_rate : Decimal
        The effective taker fee rate as a decimal fraction (matches
        ``feeSchedule.rate`` and ``instrument.taker_fee``).

    Returns
    -------
    Decimal
        The rebate share in [0, 1]. Returns 0 when the market is fee-free or
        cannot be classified.

    References
    ----------
    https://docs.polymarket.com/market-makers/maker-rebates

    """
    if fee_rate <= 0:
        return Decimal(0)

    labels = _market_labels(market_info)
    if labels & _CRYPTO_LABELS:
        return _CRYPTO_MAKER_REBATE_RATE
    if labels & _FEE_ENABLED_NON_CRYPTO_LABELS:
        return _DEFAULT_FEE_ENABLED_MAKER_REBATE_RATE

    if fee_rate in _CRYPTO_FEE_RATES:
        return _CRYPTO_MAKER_REBATE_RATE
    if fee_rate in _NON_CRYPTO_FEE_RATES:
        return _DEFAULT_FEE_ENABLED_MAKER_REBATE_RATE

    return Decimal(0)


def calculate_maker_rebate(
    quantity: Decimal,
    price: Decimal,
    fee_rate: Decimal,
    maker_rebate_rate: Decimal,
) -> float:
    """
    Calculate a fill-level maker rebate estimate in quote currency.

    Polymarket distributes actual rebates daily from each market's rebate pool.
    For backtests, a per-fill credit equal to the documented rebate share of
    the fill's fee-equivalent value preserves the aggregate economics without
    pretending to know other makers' wallet-level state.

    Parameters
    ----------
    quantity : Decimal
        The fill quantity (shares).
    price : Decimal
        The fill price.
    fee_rate : Decimal
        The effective taker fee rate as a decimal fraction.
    maker_rebate_rate : Decimal
        The rebate share in [0, 1].

    Returns
    -------
    float
        The rebate amount as a positive quote-currency value, rounded to 5
        decimal places. Callers negate this when constructing a maker
        commission.

    """
    if fee_rate <= 0 or maker_rebate_rate <= 0:
        return 0.0

    fee_equivalent = Decimal(
        str(
            calculate_commission(
                quantity=quantity,
                price=price,
                fee_rate=fee_rate,
                liquidity_side=LiquiditySide.TAKER,
            ),
        ),
    )
    rebate = fee_equivalent * maker_rebate_rate
    return float(rebate.quantize(_REBATE_QUANTUM, rounding=ROUND_HALF_UP))


class PolymarketFeeModelConfig(FeeModelConfig, frozen=True):
    """
    Configuration for ``PolymarketFeeModel`` instances.

    Parameters
    ----------
    maker_rebates_enabled : bool, default True
        Whether to credit passive maker fills with a rebate. Disable when
        modelling pure taker strategies.

    """

    maker_rebates_enabled: bool = True


class PolymarketFeeModel(FeeModel):
    """
    Polymarket fee model for backtesting with optional maker rebates.

    Applies Polymarket's taker fee formula per fill::

        fee = qty * fee_rate * p * (1 - p)

    Where ``fee_rate`` is taken from ``instrument.taker_fee`` and ``p`` is the
    fill price in [0, 1]. Maker fees remain zero. When ``maker_rebates_enabled``
    is true, fills that the matching engine routes through ``LiquiditySide.MAKER``
    receive a rebate credit modeled as a negative commission inferred from the
    market category and the documented rebate share.

    Parameters
    ----------
    maker_rebates_enabled : bool, default True
        Whether to credit passive maker fills with a rebate. Disable when
        modelling pure taker strategies. Ignored when ``config`` is provided.
    config : PolymarketFeeModelConfig, optional
        The configuration for the fee model. When provided, takes precedence
        over the positional argument so the standard ``FeeModelFactory`` path
        works.

    References
    ----------
    https://docs.polymarket.com/trading/fees
    https://docs.polymarket.com/market-makers/maker-rebates

    """

    def __init__(
        self,
        maker_rebates_enabled: bool = True,
        config: PolymarketFeeModelConfig | None = None,
    ) -> None:
        if config is not None:
            maker_rebates_enabled = config.maker_rebates_enabled
        self._maker_rebates_enabled = maker_rebates_enabled

    def get_commission(
        self,
        order: Order,
        fill_qty: Quantity,
        fill_px: Price,
        instrument: Instrument,
    ) -> Money:
        quote_currency = instrument.quote_currency
        taker_fee = instrument.taker_fee
        if taker_fee is None or taker_fee <= 0:
            return Money(Decimal(0), quote_currency)

        fill_quantity = Decimal(str(fill_qty))
        fill_price = Decimal(str(fill_px))
        fee_rate = Decimal(str(taker_fee))

        if order.liquidity_side == LiquiditySide.MAKER:
            if not self._maker_rebates_enabled:
                return Money(Decimal(0), quote_currency)

            rebate_rate = infer_maker_rebate_rate(
                market_info=getattr(instrument, "info", None),
                fee_rate=fee_rate,
            )
            rebate = calculate_maker_rebate(
                quantity=fill_quantity,
                price=fill_price,
                fee_rate=fee_rate,
                maker_rebate_rate=rebate_rate,
            )
            return Money(Decimal(str(-rebate)), quote_currency)

        # Non-MAKER (incl. NO_LIQUIDITY_SIDE) charges the taker fee
        commission = calculate_commission(
            quantity=fill_quantity,
            price=fill_price,
            fee_rate=fee_rate,
            liquidity_side=LiquiditySide.TAKER,
        )
        return Money(Decimal(str(commission)), quote_currency)
