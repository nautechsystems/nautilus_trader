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

from decimal import Decimal
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.polymarket.fee_model import PolymarketFeeModel
from nautilus_trader.adapters.polymarket.fee_model import PolymarketFeeModelConfig
from nautilus_trader.adapters.polymarket.fee_model import calculate_maker_rebate
from nautilus_trader.adapters.polymarket.fee_model import infer_maker_rebate_rate
from nautilus_trader.backtest.config import FeeModelFactory
from nautilus_trader.backtest.config import ImportableFeeModelConfig
from nautilus_trader.model.currencies import pUSD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def _make_instrument(taker_fee: Decimal | None, info: dict | None = None) -> MagicMock:
    instrument = MagicMock()
    instrument.quote_currency = pUSD
    instrument.taker_fee = taker_fee
    instrument.info = info or {}
    return instrument


def _make_order(liquidity_side: LiquiditySide) -> MagicMock:
    order = MagicMock()
    order.liquidity_side = liquidity_side
    return order


@pytest.mark.parametrize(
    ("qty", "px", "fee_rate", "side", "expected"),
    [
        # fee = qty * px * (1 - px) * fee_rate
        ("100", "0.50", "0.072", LiquiditySide.TAKER, Decimal("1.80")),
        ("100", "0.30", "0.072", LiquiditySide.TAKER, Decimal("1.512")),
        ("50", "0.10", "0.030", LiquiditySide.TAKER, Decimal("0.135")),
        # Any non-MAKER side falls through to the taker path; the engine
        # uses NO_LIQUIDITY_SIDE for marketable orders before tagging.
        ("100", "0.50", "0.072", LiquiditySide.NO_LIQUIDITY_SIDE, Decimal("1.80")),
    ],
)
def test_taker_fill_pays_documented_taker_fee(qty, px, fee_rate, side, expected):
    # Arrange
    instrument = _make_instrument(Decimal(fee_rate))
    order = _make_order(side)
    model = PolymarketFeeModel()

    # Act
    commission = model.get_commission(
        order,
        Quantity.from_str(qty),
        Price.from_str(px),
        instrument,
    )

    # Assert
    assert commission.currency == pUSD
    assert commission.as_decimal() == expected


@pytest.mark.parametrize(
    ("info", "fee_rate", "expected_rebate"),
    [
        # taker fee at qty=100 px=0.50: qty*px*(1-px)*rate = 25 * rate
        # rebate = -taker_fee * rebate_rate
        # crypto label -> 20%: 25 * 0.072 * 0.20 = 0.36
        ({"category": "crypto"}, "0.072", Decimal("-0.36")),
        # non-crypto fee-enabled label -> 25%: 25 * 0.030 * 0.25 = 0.1875
        ({"category": "politics"}, "0.030", Decimal("-0.1875")),
        # no labels but fee_rate matches crypto -> 20%
        ({}, "0.072", Decimal("-0.36")),
        # no labels but fee_rate matches non-crypto fallback -> 25%
        ({}, "0.030", Decimal("-0.1875")),
    ],
)
def test_maker_fill_credits_rebate_for_classified_market(info, fee_rate, expected_rebate):
    # Arrange
    instrument = _make_instrument(Decimal(fee_rate), info=info)
    order = _make_order(LiquiditySide.MAKER)
    model = PolymarketFeeModel()

    # Act
    commission = model.get_commission(
        order,
        Quantity.from_str("100"),
        Price.from_str("0.50"),
        instrument,
    )

    # Assert - negative commission (rebate credit)
    assert commission.as_decimal() == expected_rebate


def test_maker_fill_zero_when_unclassified():
    # Arrange - no labels, fee rate not in known sets
    instrument = _make_instrument(Decimal("0.999"), info={})
    order = _make_order(LiquiditySide.MAKER)
    model = PolymarketFeeModel()

    # Act
    commission = model.get_commission(
        order,
        Quantity.from_str("100"),
        Price.from_str("0.50"),
        instrument,
    )

    # Assert - rebate inferred as zero, commission is zero
    assert commission == Money(Decimal(0), pUSD)


def test_maker_fill_zero_when_rebates_disabled():
    # Arrange
    instrument = _make_instrument(Decimal("0.072"), info={"category": "crypto"})
    order = _make_order(LiquiditySide.MAKER)
    model = PolymarketFeeModel(maker_rebates_enabled=False)

    # Act
    commission = model.get_commission(
        order,
        Quantity.from_str("100"),
        Price.from_str("0.50"),
        instrument,
    )

    # Assert
    assert commission == Money(Decimal(0), pUSD)


def test_zero_fee_rate_returns_zero_money():
    # Arrange - fee-free market: no taker fee, no rebate
    instrument = _make_instrument(Decimal(0))
    model = PolymarketFeeModel()

    for side in (LiquiditySide.TAKER, LiquiditySide.MAKER):
        order = _make_order(side)
        # Act
        commission = model.get_commission(
            order,
            Quantity.from_str("100"),
            Price.from_str("0.50"),
            instrument,
        )

        # Assert
        assert commission == Money(Decimal(0), pUSD)


def test_none_taker_fee_returns_zero_money():
    # Arrange - some instruments may carry no taker_fee; treat as fee-free
    instrument = _make_instrument(None)
    model = PolymarketFeeModel()
    order = _make_order(LiquiditySide.TAKER)

    # Act
    commission = model.get_commission(
        order,
        Quantity.from_str("100"),
        Price.from_str("0.50"),
        instrument,
    )

    # Assert
    assert commission == Money(Decimal(0), pUSD)


def test_factory_path_constructs_via_importable_config():
    # Arrange - the documented BacktestVenueConfig.fee_model path
    cfg = ImportableFeeModelConfig(
        fee_model_path="nautilus_trader.adapters.polymarket.fee_model:PolymarketFeeModel",
        config_path=("nautilus_trader.adapters.polymarket.fee_model:PolymarketFeeModelConfig"),
        config={"maker_rebates_enabled": False},
    )

    # Act
    model = FeeModelFactory.create(cfg)

    # Assert - the model honors the config flag end-to-end
    assert isinstance(model, PolymarketFeeModel)
    instrument = _make_instrument(Decimal("0.072"), info={"category": "crypto"})
    order = _make_order(LiquiditySide.MAKER)
    commission = model.get_commission(
        order,
        Quantity.from_str("100"),
        Price.from_str("0.50"),
        instrument,
    )
    assert commission == Money(Decimal(0), pUSD)


def test_factory_path_default_config_enables_rebates():
    # Arrange - empty config dict -> defaults preserved
    cfg = ImportableFeeModelConfig(
        fee_model_path="nautilus_trader.adapters.polymarket.fee_model:PolymarketFeeModel",
        config_path=("nautilus_trader.adapters.polymarket.fee_model:PolymarketFeeModelConfig"),
        config={},
    )

    # Act
    model = FeeModelFactory.create(cfg)

    # Assert
    instrument = _make_instrument(Decimal("0.072"), info={"category": "crypto"})
    order = _make_order(LiquiditySide.MAKER)
    commission = model.get_commission(
        order,
        Quantity.from_str("100"),
        Price.from_str("0.50"),
        instrument,
    )
    assert commission.as_decimal() == Decimal("-0.36")


@pytest.mark.parametrize(
    ("info", "fee_rate", "expected"),
    [
        ({"category": "crypto"}, Decimal("0.072"), Decimal("0.20")),
        ({"category": "politics"}, Decimal("0.030"), Decimal("0.25")),
        ({"tags": ["sports"]}, Decimal("0.030"), Decimal("0.25")),
        # Labels still win even when the rate is outside the known set
        ({"tags": [{"label": "crypto"}]}, Decimal("0.0072"), Decimal("0.20")),
        ({"events": [{"category": "crypto"}]}, Decimal("0.072"), Decimal("0.20")),
        # No labels, fee rate fallback by documented Polymarket rates
        ({}, Decimal("0.072"), Decimal("0.20")),
        ({}, Decimal("0.040"), Decimal("0.25")),
        # Unknown rate, no labels -> no rebate
        ({}, Decimal("0.999"), Decimal(0)),
        ({}, Decimal("0.0072"), Decimal(0)),
        # None info -> falls through to fee-rate fallback (0.072 -> crypto)
        (None, Decimal("0.072"), Decimal("0.20")),
        # Zero fee rate -> short-circuits to zero regardless of labels
        ({"category": "crypto"}, Decimal(0), Decimal(0)),
    ],
)
def test_infer_maker_rebate_rate(info, fee_rate, expected):
    # Act
    rate = infer_maker_rebate_rate(market_info=info, fee_rate=fee_rate)

    # Assert
    assert rate == expected


def test_calculate_maker_rebate_rounds_to_five_decimals():
    # Arrange - calculate_commission rounds to 5dp; the rebate is
    # rebate_rate * that rounded value, then itself rounded to 5dp.
    # qty*px*(1-px)*rate = 33 * 0.27 * 0.73 * 0.072 -> 0.46831 (5dp)
    # rebate = 0.46831 * 0.20 = 0.093662 -> 0.09366 (5dp, half-up)
    rebate = calculate_maker_rebate(
        quantity=Decimal(33),
        price=Decimal("0.27"),
        fee_rate=Decimal("0.072"),
        maker_rebate_rate=Decimal("0.20"),
    )

    # Assert
    assert rebate == 0.09366


def test_calculate_maker_rebate_zero_for_nonpositive_inputs():
    # Arrange
    common_kwargs = {
        "quantity": Decimal(100),
        "price": Decimal("0.50"),
    }

    # Act + Assert
    assert (
        calculate_maker_rebate(
            fee_rate=Decimal(0),
            maker_rebate_rate=Decimal("0.20"),
            **common_kwargs,
        )
        == 0.0
    )
    assert (
        calculate_maker_rebate(
            fee_rate=Decimal("0.072"),
            maker_rebate_rate=Decimal(0),
            **common_kwargs,
        )
        == 0.0
    )


def test_polymarket_fee_model_config_default_maker_rebates_enabled():
    # Act
    cfg = PolymarketFeeModelConfig()

    # Assert
    assert cfg.maker_rebates_enabled is True
