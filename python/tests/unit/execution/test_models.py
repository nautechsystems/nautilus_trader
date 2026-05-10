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

from nautilus_trader.execution import BestPriceFillModel
from nautilus_trader.execution import CompetitionAwareFillModel
from nautilus_trader.execution import DefaultFillModel
from nautilus_trader.execution import FixedFeeModel
from nautilus_trader.execution import LimitOrderPartialFillModel
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.execution import MarketHoursFillModel
from nautilus_trader.execution import OneTickSlippageFillModel
from nautilus_trader.execution import PerContractFeeModel
from nautilus_trader.execution import ProbabilisticFillModel
from nautilus_trader.execution import SizeAwareFillModel
from nautilus_trader.execution import StaticLatencyModel
from nautilus_trader.execution import ThreeTierFillModel
from nautilus_trader.execution import TwoTierFillModel
from nautilus_trader.execution import VolumeSensitiveFillModel
from nautilus_trader.execution import calculate_reconciliation_price
from nautilus_trader.model import Money


def test_default_fill_model():
    model = DefaultFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_default_fill_model_with_seed():
    model = DefaultFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1, random_seed=42)

    assert model is not None


def test_best_price_fill_model():
    model = BestPriceFillModel(prob_fill_on_limit=0.9, prob_slippage=0.05)

    assert model is not None


def test_best_price_fill_model_with_seed():
    model = BestPriceFillModel(prob_fill_on_limit=0.9, prob_slippage=0.05, random_seed=42)

    assert model is not None


def test_competition_aware_fill_model():
    model = CompetitionAwareFillModel()

    assert model is not None


def test_competition_aware_fill_model_with_params():
    model = CompetitionAwareFillModel(
        prob_fill_on_limit=0.9,
        prob_slippage=0.1,
        random_seed=42,
        liquidity_factor=0.5,
    )

    assert model is not None


def test_limit_order_partial_fill_model():
    model = LimitOrderPartialFillModel(prob_fill_on_limit=0.7, prob_slippage=0.2)

    assert model is not None


def test_market_hours_fill_model():
    model = MarketHoursFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_one_tick_slippage_fill_model():
    model = OneTickSlippageFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_probabilistic_fill_model():
    model = ProbabilisticFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_size_aware_fill_model():
    model = SizeAwareFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_three_tier_fill_model():
    model = ThreeTierFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_two_tier_fill_model():
    model = TwoTierFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_volume_sensitive_fill_model():
    model = VolumeSensitiveFillModel(prob_fill_on_limit=0.8, prob_slippage=0.1)

    assert model is not None


def test_fixed_fee_model():
    model = FixedFeeModel(commission=Money.from_str("5.00 USD"))

    assert model is not None


def test_fixed_fee_model_with_charge_once():
    model = FixedFeeModel(commission=Money.from_str("5.00 USD"), change_commission_once=True)

    assert model is not None


def test_maker_taker_fee_model():
    model = MakerTakerFeeModel()

    assert model is not None


def test_per_contract_fee_model():
    model = PerContractFeeModel(commission=Money.from_str("1.25 USD"))

    assert model is not None


def test_static_latency_model_defaults():
    model = StaticLatencyModel()

    assert model is not None


def test_static_latency_model_with_params():
    model = StaticLatencyModel(
        base_latency_nanos=1_000_000,
        insert_latency_nanos=2_000_000,
        update_latency_nanos=1_500_000,
        cancel_latency_nanos=500_000,
    )

    assert model is not None


def test_calculate_reconciliation_price_open_position():
    result = calculate_reconciliation_price(
        current_position_qty=Decimal(10),
        current_position_avg_px=Decimal("100.50"),
        target_position_qty=Decimal(15),
        target_position_avg_px=Decimal("105.00"),
    )

    assert isinstance(result, Decimal)


def test_calculate_reconciliation_price_flat_to_open():
    result = calculate_reconciliation_price(
        current_position_qty=Decimal(0),
        current_position_avg_px=None,
        target_position_qty=Decimal(10),
        target_position_avg_px=Decimal("100.00"),
    )

    assert isinstance(result, Decimal)


def test_calculate_reconciliation_price_close_position():
    result = calculate_reconciliation_price(
        current_position_qty=Decimal(10),
        current_position_avg_px=Decimal("100.00"),
        target_position_qty=Decimal(0),
        target_position_avg_px=Decimal(0),
    )

    assert result is None or isinstance(result, Decimal)
