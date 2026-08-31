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
Test models behavior.
"""

from decimal import Decimal

import pytest

from nautilus_trader.core import UUID4
from nautilus_trader.execution import BestPriceFillModel
from nautilus_trader.execution import CappedOptionFeeModel
from nautilus_trader.execution import CompetitionAwareFillModel
from nautilus_trader.execution import DefaultFillModel
from nautilus_trader.execution import FeeModel
from nautilus_trader.execution import FixedFeeModel
from nautilus_trader.execution import LimitOrderPartialFillModel
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.execution import MarketHoursFillModel
from nautilus_trader.execution import OneTickSlippageFillModel
from nautilus_trader.execution import PerContractFeeModel
from nautilus_trader.execution import ProbabilisticFillModel
from nautilus_trader.execution import ProbabilityPriceFeeModel
from nautilus_trader.execution import SizeAwareFillModel
from nautilus_trader.execution import StaticLatencyModel
from nautilus_trader.execution import ThreeTierFillModel
from nautilus_trader.execution import TieredNotionalOptionFeeModel
from nautilus_trader.execution import TwoTierFillModel
from nautilus_trader.execution import VolumeSensitiveFillModel
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import Money
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from tests.providers import TestInstrumentProvider


def test_default_fill_model() -> None:
    """
    Test default fill model.
    """
    model = DefaultFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_default_fill_model_with_seed() -> None:
    """
    Test default fill model with seed.
    """
    model = DefaultFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1, random_seed=42)

    assert model is not None


def test_best_price_fill_model() -> None:
    """
    Test best price fill model.
    """
    model = BestPriceFillModel(prob_fill_on_limit=0.9, prob_slippage=0.05)

    assert model is not None


def test_best_price_fill_model_with_seed() -> None:
    """
    Test best price fill model with seed.
    """
    model = BestPriceFillModel(prob_fill_on_limit=0.9, prob_slippage=0.05, random_seed=42)

    assert model is not None


def test_competition_aware_fill_model() -> None:
    """
    Test competition aware fill model.
    """
    model = CompetitionAwareFillModel()

    assert model is not None


def test_competition_aware_fill_model_with_params() -> None:
    """
    Test competition aware fill model with params.
    """
    model = CompetitionAwareFillModel(
        prob_fill_on_limit=0.9,
        prob_slippage=0.1,
        random_seed=42,
        liquidity_factor=0.5,
    )

    assert model is not None


def test_limit_order_partial_fill_model() -> None:
    """
    Test limit order partial fill model.
    """
    model = LimitOrderPartialFillModel(prob_fill_on_limit=0.7, prob_slippage=0.2)

    assert model is not None


def test_market_hours_fill_model() -> None:
    """
    Test market hours fill model.
    """
    model = MarketHoursFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_one_tick_slippage_fill_model() -> None:
    """
    Test one tick slippage fill model.
    """
    model = OneTickSlippageFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_probabilistic_fill_model() -> None:
    """
    Test probabilistic fill model.
    """
    model = ProbabilisticFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_size_aware_fill_model() -> None:
    """
    Test size aware fill model.
    """
    model = SizeAwareFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_three_tier_fill_model() -> None:
    """
    Test three tier fill model.
    """
    model = ThreeTierFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_two_tier_fill_model() -> None:
    """
    Test two tier fill model.
    """
    model = TwoTierFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_volume_sensitive_fill_model() -> None:
    """
    Test volume sensitive fill model.
    """
    model = VolumeSensitiveFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_fixed_fee_model() -> None:
    """
    Test fixed fee model.
    """
    model = FixedFeeModel(commission=Money.from_str("5.00 USD"))

    assert model is not None


@pytest.mark.parametrize(
    "keyword",
    [
        "charge_commission_once",
        "change_commission_once",
    ],
)
def test_fixed_fee_model_charge_once_keyword_routes_false(keyword: object) -> None:
    """
    Test fixed fee model charge once keyword routes false.
    """
    model = FixedFeeModel(commission=Money.from_str("5.00 USD"), **{keyword: False})

    assert "charge_commission_once: false" in repr(model)


def test_fixed_fee_model_rejects_both_charge_once_keywords() -> None:
    """
    Test fixed fee model rejects both charge once keywords.
    """
    with pytest.raises(TypeError, match="Provide only one"):
        FixedFeeModel(
            commission=Money.from_str("5.00 USD"),
            charge_commission_once=True,
            change_commission_once=True,
        )


def test_maker_taker_fee_model() -> None:
    """
    Test maker taker fee model.
    """
    model = MakerTakerFeeModel()

    assert model is not None


def test_per_contract_fee_model() -> None:
    """
    Test per contract fee model.
    """
    model = PerContractFeeModel(commission=Money.from_str("1.25 USD"))

    assert model is not None


def test_probability_price_fee_model() -> None:
    """
    Test probability price fee model.
    """
    model = ProbabilityPriceFeeModel()

    assert model is not None


def test_capped_option_fee_model() -> None:
    """
    Test capped option fee model.
    """
    model = CappedOptionFeeModel(
        maker_rate=Decimal("0.0003"),
        taker_rate=Decimal("0.0003"),
    )

    assert model is not None
    expected = (
        "CappedOptionFeeModel { maker_rate: Some(0.0003), "
        "taker_rate: Some(0.0003), cap_rate: 0.125 }"
    )
    assert repr(model) == expected


def test_tiered_notional_option_fee_model() -> None:
    """
    Test tiered notional option fee model.
    """
    model = TieredNotionalOptionFeeModel(
        maker_rate=Decimal("0.0002"),
        taker_rate=Decimal("0.0005"),
    )

    assert model is not None
    expected = "TieredNotionalOptionFeeModel { maker_rate: Some(0.0002), taker_rate: Some(0.0005) }"
    assert repr(model) == expected


def test_fee_model_is_instantiable() -> None:
    """
    Test fee model is instantiable.
    """
    assert isinstance(FeeModel(), FeeModel)


def test_concrete_fee_models_inherit_fee_model() -> None:
    """
    Test concrete fee models inherit fee model.
    """
    fixed = FixedFeeModel(commission=Money.from_str("5.00 USD"))
    maker_taker = MakerTakerFeeModel()
    per_contract = PerContractFeeModel(commission=Money.from_str("1.25 USD"))
    probability = ProbabilityPriceFeeModel()
    capped = CappedOptionFeeModel(maker_rate=Decimal("0.0003"), taker_rate=Decimal("0.0003"))
    tiered = TieredNotionalOptionFeeModel(
        maker_rate=Decimal("0.0002"),
        taker_rate=Decimal("0.0005"),
    )

    assert isinstance(fixed, FeeModel)
    assert isinstance(maker_taker, FeeModel)
    assert isinstance(per_contract, FeeModel)
    assert isinstance(probability, FeeModel)
    assert isinstance(capped, FeeModel)
    assert isinstance(tiered, FeeModel)


def test_fee_model_subclass_with_init_args() -> None:
    """
    Test fee model subclass with init args.
    """

    class PercentFee(FeeModel):
        """
        Collect percent fee tests.
        """

        def __init__(self, rate: object) -> None:
            """
            Initialize the helper.
            """
            self.rate = rate

    assert PercentFee(Decimal("0.0005")).rate == Decimal("0.0005")


def test_fee_model_subclass_get_commission_dispatches_to_override() -> None:
    """
    Test fee model subclass get commission dispatches to override.
    """

    class FixedOverride(FeeModel):
        """
        Collect fixed override tests.
        """

        def __init__(self, commission: object) -> None:
            """
            Initialize the helper.
            """
            self.commission = commission

        def get_commission(
            self,
            _order: object,
            _fill_quantity: object,
            _fill_px: object,
            _instrument: object,
        ) -> object:
            """
            Get commission.
            """
            return self.commission

    model = FixedOverride(Money.from_str("5.00 USD"))

    assert isinstance(model, FeeModel)
    assert model.get_commission(None, None, None, None) == Money.from_str("5.00 USD")


def test_fee_model_base_get_commission_raises_not_implemented() -> None:
    """
    Test fee model base get commission raises not implemented.
    """
    with pytest.raises(NotImplementedError):
        FeeModel().get_commission(None, Quantity.from_str("1"), Price.from_str("1.0"), None)


def test_fee_model_get_commission_with_context_rejects_non_instrument() -> None:
    """
    Test fee model get commission with context rejects non instrument.
    """
    model = MakerTakerFeeModel()

    with pytest.raises(TypeError, match="instrument"):
        model.get_commission_with_context(
            None,
            Quantity.from_str("1"),
            Price.from_str("1.0"),
            "not-an-instrument",
        )


def test_fixed_fee_model_get_commission_direct_call() -> None:
    """
    Test fixed fee model get commission direct call.
    """
    commission = Money.from_str("5.00 USD")
    model = FixedFeeModel(commission=commission, charge_commission_once=False)
    instrument = TestInstrumentProvider.audusd_sim()
    order = MarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        contingency_type=None,
    )

    result = model.get_commission(
        order,
        Quantity.from_int(100_000),
        Price.from_str("1.00000"),
        instrument,
    )

    assert result == commission


def _make_market_order(instrument: object) -> object:
    return MarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
        contingency_type=None,
    )


def test_per_contract_fee_model_get_commission_direct_call() -> None:
    """
    Test per contract fee model get commission direct call.
    """
    commission = Money.from_str("1.25 USD")
    model = PerContractFeeModel(commission=commission)
    instrument = TestInstrumentProvider.audusd_sim()
    order = _make_market_order(instrument)

    result = model.get_commission(
        order,
        Quantity.from_int(2),
        Price.from_str("1.00000"),
        instrument,
    )

    assert result == Money.from_str("2.50 USD")


def test_maker_taker_fee_model_get_commission_direct_call() -> None:
    """
    Test maker taker fee model get commission direct call.
    """
    model = MakerTakerFeeModel()
    instrument = TestInstrumentProvider.audusd_sim()
    order = _make_market_order(instrument)

    with pytest.raises(RuntimeError, match="Liquidity side"):
        model.get_commission(
            order,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            instrument,
        )


def test_probability_price_fee_model_get_commission_direct_call() -> None:
    """
    Test probability price fee model get commission direct call.
    """
    model = ProbabilityPriceFeeModel()
    instrument = TestInstrumentProvider.audusd_sim()
    order = _make_market_order(instrument)

    with pytest.raises(RuntimeError, match="binary option instrument"):
        model.get_commission(order, Quantity.from_int(100), Price.from_str("0.50"), instrument)


def test_capped_option_fee_model_get_commission_direct_call() -> None:
    """
    Test capped option fee model get commission direct call.
    """
    model = CappedOptionFeeModel()
    instrument = TestInstrumentProvider.audusd_sim()
    order = _make_market_order(instrument)

    with pytest.raises(RuntimeError, match="CappedOptionFeeModel requires an option instrument"):
        model.get_commission(order, Quantity.from_int(2), Price.from_str("100.00"), instrument)


def test_tiered_notional_option_fee_model_get_commission_direct_call() -> None:
    """
    Test tiered notional option fee model get commission direct call.
    """
    model = TieredNotionalOptionFeeModel()
    instrument = TestInstrumentProvider.audusd_sim()
    order = _make_market_order(instrument)

    with pytest.raises(
        RuntimeError,
        match="TieredNotionalOptionFeeModel requires an option instrument",
    ):
        model.get_commission(order, Quantity.from_int(2), Price.from_str("100.00"), instrument)


def test_static_latency_model_defaults() -> None:
    """
    Test static latency model defaults.
    """
    model = StaticLatencyModel()

    assert repr(model) == (
        "StaticLatencyModel { base_latency_nanos: UnixNanos(0), "
        "insert_latency_nanos: UnixNanos(0), update_latency_nanos: UnixNanos(0), "
        "delete_latency_nanos: UnixNanos(0) }"
    )


def test_static_latency_model_with_params() -> None:
    """
    Test static latency model with params.
    """
    model = StaticLatencyModel(
        base_latency_nanos=1_000_000,
        insert_latency_nanos=2_000_000,
        update_latency_nanos=1_500_000,
        cancel_latency_nanos=500_000,
    )

    assert repr(model) == (
        "StaticLatencyModel { base_latency_nanos: UnixNanos(1000000), "
        "insert_latency_nanos: UnixNanos(3000000), "
        "update_latency_nanos: UnixNanos(2500000), "
        "delete_latency_nanos: UnixNanos(1500000) }"
    )
