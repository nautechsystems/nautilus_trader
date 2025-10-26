# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pickle

import pandas as pd
import pytest

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model import convert_to_raw_int
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestIdStubs.audusd_id()
GBPUSD_SIM = TestIdStubs.gbpusd_id()

ONE_MIN_BID = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
AUDUSD_1_MIN_BID = BarType(AUDUSD_SIM, ONE_MIN_BID)
GBPUSD_1_MIN_BID = BarType(GBPUSD_SIM, ONE_MIN_BID)


class TestBarSpecification:
    def test_bar_spec_equality(self):
        # Arrange
        bar_spec1 = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_spec2 = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_spec3 = BarSpecification(1, BarAggregation.MINUTE, PriceType.ASK)

        # Act, Assert
        assert bar_spec1 == bar_spec1
        assert bar_spec1 == bar_spec2
        assert bar_spec1 != bar_spec3

    def test_bar_spec_comparison(self):
        # Arrange
        bar_spec1 = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_spec2 = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_spec3 = BarSpecification(1, BarAggregation.MINUTE, PriceType.ASK)

        # Act, Assert
        assert bar_spec1 <= bar_spec2
        assert bar_spec3 > bar_spec1
        assert bar_spec1 < bar_spec3
        assert bar_spec3 >= bar_spec1

    def test_bar_spec_pickle(self):
        # Arrange
        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.LAST)

        # Act
        pickled = pickle.dumps(bar_spec)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert unpickled == bar_spec

    def test_bar_spec_hash_str_and_repr(self):
        # Arrange
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)

        # Act, Assert
        assert isinstance(hash(bar_spec), int)
        assert str(bar_spec) == "1-MINUTE-BID"
        assert repr(bar_spec) == "BarSpecification(1-MINUTE-BID)"

    @pytest.mark.parametrize(
        ("bar_aggregation", "step", "expected_msg"),
        [
            # MILLISECOND
            [BarAggregation.MILLISECOND, 0, "'step' not a positive integer, was 0"],
            [BarAggregation.MILLISECOND, -1, "'step' not a positive integer, was -1"],
            [
                BarAggregation.MILLISECOND,
                12,
                "Invalid step in bar_type.spec.step: 12 for aggregation=10. step must evenly divide 1000",
            ],
            [
                BarAggregation.MILLISECOND,
                2000,
                "Invalid step in bar_type.spec.step: 2000 for aggregation=10. step must evenly divide 1000",
            ],
            [
                BarAggregation.MILLISECOND,
                1000,
                "Invalid step in bar_type.spec.step: 1000 for aggregation=10. step must not be 1000",
            ],
            # SECOND
            [
                BarAggregation.SECOND,
                50,
                "Invalid step in bar_type.spec.step: 50 for aggregation=11. step must evenly divide 60",
            ],
            [
                BarAggregation.SECOND,
                120,
                "Invalid step in bar_type.spec.step: 120 for aggregation=11. step must evenly divide 60",
            ],
            [
                BarAggregation.SECOND,
                60,
                "Invalid step in bar_type.spec.step: 60 for aggregation=11. step must not be 60",
            ],
            # MINUTE
            [
                BarAggregation.MINUTE,
                40,
                "Invalid step in bar_type.spec.step: 40 for aggregation=12. step must evenly divide 60",
            ],
            [
                BarAggregation.MINUTE,
                120,
                "Invalid step in bar_type.spec.step: 120 for aggregation=12. step must evenly divide 60",
            ],
            [
                BarAggregation.MINUTE,
                60,
                "Invalid step in bar_type.spec.step: 60 for aggregation=12. step must not be 60",
            ],
            # HOUR
            [
                BarAggregation.HOUR,
                13,
                "Invalid step in bar_type.spec.step: 13 for aggregation=13. step must evenly divide 24",
            ],
            [
                BarAggregation.HOUR,
                48,
                "Invalid step in bar_type.spec.step: 48 for aggregation=13. step must evenly divide 24",
            ],
            [
                BarAggregation.HOUR,
                24,
                "Invalid step in bar_type.spec.step: 24 for aggregation=13. step must not be 24",
            ],
            # DAY
            [
                BarAggregation.DAY,
                2,
                "Invalid step in bar_type.spec.step: 2 for aggregation=14. step must evenly divide 1",
            ],
            # WEEK
            [
                BarAggregation.WEEK,
                2,
                "Invalid step in bar_type.spec.step: 2 for aggregation=15. step must evenly divide 1",
            ],
            [
                BarAggregation.WEEK,
                3,
                "Invalid step in bar_type.spec.step: 3 for aggregation=15. step must evenly divide 1",
            ],
            # MONTH
            [
                BarAggregation.MONTH,
                5,
                "Invalid step in bar_type.spec.step: 5 for aggregation=16. step must evenly divide 12",
            ],
            [
                BarAggregation.MONTH,
                24,
                "Invalid step in bar_type.spec.step: 24 for aggregation=16. step must evenly divide 12",
            ],
            [
                BarAggregation.MONTH,
                12,
                "Invalid step in bar_type.spec.step: 12 for aggregation=16. step must not be 12",
            ],
        ],
    )
    def test_instantiate_given_invalid_step_raises_value_error(
        self,
        bar_aggregation: BarAggregation,
        step: int,
        expected_msg: str,
    ):
        with pytest.raises(ValueError, match=expected_msg):
            BarSpecification(step, bar_aggregation, PriceType.BID)

    @pytest.mark.parametrize(
        ("step", "aggregation"),
        [
            # Millisecond valid steps
            (1, BarAggregation.MILLISECOND),
            (10, BarAggregation.MILLISECOND),
            (100, BarAggregation.MILLISECOND),
            (200, BarAggregation.MILLISECOND),
            (250, BarAggregation.MILLISECOND),
            (500, BarAggregation.MILLISECOND),
            # Second valid steps
            (1, BarAggregation.SECOND),
            (5, BarAggregation.SECOND),
            (10, BarAggregation.SECOND),
            (15, BarAggregation.SECOND),
            (20, BarAggregation.SECOND),
            (30, BarAggregation.SECOND),
            # Minute valid steps
            (1, BarAggregation.MINUTE),
            (5, BarAggregation.MINUTE),
            (10, BarAggregation.MINUTE),
            (15, BarAggregation.MINUTE),
            (20, BarAggregation.MINUTE),
            (30, BarAggregation.MINUTE),
            # Hour valid steps
            (1, BarAggregation.HOUR),
            (2, BarAggregation.HOUR),
            (3, BarAggregation.HOUR),
            (4, BarAggregation.HOUR),
            (6, BarAggregation.HOUR),
            (8, BarAggregation.HOUR),
            (12, BarAggregation.HOUR),
            # Day and Week - only step=1 is valid
            (1, BarAggregation.DAY),
            (1, BarAggregation.WEEK),
            # Month valid steps
            (1, BarAggregation.MONTH),
            (2, BarAggregation.MONTH),
            (3, BarAggregation.MONTH),
            (4, BarAggregation.MONTH),
            (6, BarAggregation.MONTH),
            # Year valid steps
            (1, BarAggregation.YEAR),
            (2, BarAggregation.YEAR),
            (3, BarAggregation.YEAR),
            (4, BarAggregation.YEAR),
            (13, BarAggregation.YEAR),
            # Non-time aggregations
            (1, BarAggregation.TICK),
            (100, BarAggregation.TICK),
            (1000, BarAggregation.TICK),
            (100, BarAggregation.VOLUME),
            (1000, BarAggregation.VOLUME),
            (10000, BarAggregation.VALUE),
        ],
    )
    def test_instantiate_given_correct_step_passes(self, step, aggregation):
        # Arrange, Act
        spec = BarSpecification(step, aggregation, PriceType.LAST)

        # Assert
        assert spec.step == step
        assert spec.aggregation == aggregation
        assert spec.price_type == PriceType.LAST

    @pytest.mark.parametrize(
        "value",
        ["", "1", "-1-TICK-MID", "1-TICK_MID"],
    )
    def test_from_str_given_various_invalid_strings_raises_value_error(self, value):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            BarSpecification.from_str(value)

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [
                "200-MILLISECOND-LAST",
                BarSpecification(200, BarAggregation.MILLISECOND, PriceType.LAST),
            ],
            [
                "1-MINUTE-BID",
                BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
            ],
            [
                "15-MINUTE-MID",
                BarSpecification(15, BarAggregation.MINUTE, PriceType.MID),
            ],
            [
                "100-TICK-LAST",
                BarSpecification(100, BarAggregation.TICK, PriceType.LAST),
            ],
            [
                "10000-VALUE_IMBALANCE-MID",
                BarSpecification(10000, BarAggregation.VALUE_IMBALANCE, PriceType.MID),
            ],
        ],
    )
    def test_from_str_given_various_valid_string_returns_expected_specification(
        self,
        value,
        expected,
    ):
        # Arrange, Act
        spec = BarSpecification.from_str(value)

        # Assert
        assert spec == expected

    @pytest.mark.parametrize(
        ("bar_spec", "is_time_aggregated", "is_threshold_aggregated", "is_information_aggregated"),
        [
            [
                BarSpecification(1, BarAggregation.SECOND, PriceType.BID),
                True,
                False,
                False,
            ],
            [
                BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
                True,
                False,
                False,
            ],
            [
                BarSpecification(1000, BarAggregation.TICK, PriceType.MID),
                False,
                True,
                False,
            ],
            [
                BarSpecification(10000, BarAggregation.VALUE_RUNS, PriceType.MID),
                False,
                False,
                True,
            ],
        ],
    )
    def test_aggregation_queries(
        self,
        bar_spec,
        is_time_aggregated,
        is_threshold_aggregated,
        is_information_aggregated,
    ):
        # Arrange, Act, Assert
        assert bar_spec.is_time_aggregated() == is_time_aggregated
        assert bar_spec.is_threshold_aggregated() == is_threshold_aggregated
        assert bar_spec.is_information_aggregated() == is_information_aggregated
        assert BarSpecification.check_time_aggregated(bar_spec.aggregation) == is_time_aggregated
        assert (
            BarSpecification.check_threshold_aggregated(bar_spec.aggregation)
            == is_threshold_aggregated
        )
        assert (
            BarSpecification.check_information_aggregated(bar_spec.aggregation)
            == is_information_aggregated
        )

    @pytest.mark.parametrize(
        ("step", "aggregation", "expected_timedelta"),
        [
            # MILLISECOND aggregations
            (1, BarAggregation.MILLISECOND, pd.Timedelta(milliseconds=1)),
            (100, BarAggregation.MILLISECOND, pd.Timedelta(milliseconds=100)),
            (500, BarAggregation.MILLISECOND, pd.Timedelta(milliseconds=500)),
            (250, BarAggregation.MILLISECOND, pd.Timedelta(milliseconds=250)),
            # SECOND aggregations
            (1, BarAggregation.SECOND, pd.Timedelta(seconds=1)),
            (5, BarAggregation.SECOND, pd.Timedelta(seconds=5)),
            (30, BarAggregation.SECOND, pd.Timedelta(seconds=30)),
            (15, BarAggregation.SECOND, pd.Timedelta(seconds=15)),
            # MINUTE aggregations
            (1, BarAggregation.MINUTE, pd.Timedelta(minutes=1)),
            (5, BarAggregation.MINUTE, pd.Timedelta(minutes=5)),
            (15, BarAggregation.MINUTE, pd.Timedelta(minutes=15)),
            (30, BarAggregation.MINUTE, pd.Timedelta(minutes=30)),
            # HOUR aggregations
            (1, BarAggregation.HOUR, pd.Timedelta(hours=1)),
            (2, BarAggregation.HOUR, pd.Timedelta(hours=2)),
            (4, BarAggregation.HOUR, pd.Timedelta(hours=4)),
            (12, BarAggregation.HOUR, pd.Timedelta(hours=12)),
            # DAY aggregations
            (1, BarAggregation.DAY, pd.Timedelta(days=1)),
            # WEEK aggregations
            (1, BarAggregation.WEEK, pd.Timedelta(weeks=1)),
        ],
    )
    def test_get_interval_ns_and_timedelta_valid_aggregations(
        self,
        step: int,
        aggregation: BarAggregation,
        expected_timedelta: pd.Timedelta,
    ):
        # Arrange
        spec = BarSpecification(step, aggregation, PriceType.LAST)

        # Act
        actual_ns = spec.get_interval_ns()
        actual_timedelta = spec.timedelta

        # Assert
        assert actual_ns == expected_timedelta.value
        assert actual_timedelta == expected_timedelta
        # Verify consistency between methods
        assert actual_timedelta == pd.Timedelta(nanoseconds=actual_ns)

    @pytest.mark.parametrize(
        "aggregation",
        [
            BarAggregation.TICK,
            BarAggregation.VOLUME,
            BarAggregation.VALUE,
            BarAggregation.TICK_IMBALANCE,
            BarAggregation.VOLUME_IMBALANCE,
            BarAggregation.VALUE_IMBALANCE,
            BarAggregation.TICK_RUNS,
            BarAggregation.VOLUME_RUNS,
            BarAggregation.VALUE_RUNS,
            BarAggregation.MONTH,
            BarAggregation.YEAR,
        ],
    )
    def test_get_interval_ns_and_timedelta_non_time_aggregations_raise_error(
        self,
        aggregation: BarAggregation,
    ):
        # Arrange
        spec = BarSpecification(1, aggregation, PriceType.LAST)

        # Act & Assert
        if aggregation in [BarAggregation.MONTH, BarAggregation.YEAR]:
            match = f"get_interval_ns not supported for the `BarAggregation.{aggregation.name}` aggregation"
        else:
            match = "Aggregation not time based"

        with pytest.raises(ValueError, match=match):
            spec.get_interval_ns()
        with pytest.raises(ValueError, match=match):
            spec.timedelta

    def test_properties(self):
        # Arrange, Act
        bar_spec = BarSpecification(1, BarAggregation.HOUR, PriceType.BID)

        # Assert
        assert bar_spec.step == 1
        assert bar_spec.aggregation == BarAggregation.HOUR
        assert bar_spec.price_type == PriceType.BID


class TestBarType:
    def test_bar_type_equality(self):
        # Arrange
        instrument_id1 = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        instrument_id2 = InstrumentId(Symbol("GBP/USD"), Venue("SIM"))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type1 = BarType(instrument_id1, bar_spec)
        bar_type2 = BarType(instrument_id1, bar_spec)
        bar_type3 = BarType(instrument_id2, bar_spec)

        # Act, Assert
        assert bar_type1 == bar_type1
        assert bar_type1 == bar_type2
        assert bar_type1 != bar_type3

    def test_bar_type_comparison(self):
        # Arrange
        instrument_id1 = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        instrument_id2 = InstrumentId(Symbol("GBP/USD"), Venue("SIM"))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type1 = BarType(instrument_id1, bar_spec)
        bar_type2 = BarType(instrument_id1, bar_spec)
        bar_type3 = BarType(instrument_id2, bar_spec)

        # Act, Assert
        assert bar_type1 <= bar_type2
        assert bar_type1 < bar_type3
        assert bar_type3 > bar_type1
        assert bar_type3 >= bar_type1

    def test_bar_type_pickle(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type = BarType(instrument_id, bar_spec)

        # Act
        pickled = pickle.dumps(bar_type)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert unpickled == bar_type

    def test_bar_type_hash_str_and_repr(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type = BarType(instrument_id, bar_spec)

        # Act, Assert
        assert isinstance(hash(bar_type), int)
        assert str(bar_type) == "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"
        assert repr(bar_type) == "BarType(AUD/USD.SIM-1-MINUTE-BID-EXTERNAL)"

    @pytest.mark.parametrize(
        ("input", "expected_err"),
        [
            [
                "AUD/USD.-0-0-0-0",
                "Error parsing `BarType` from 'AUD/USD.-0-0-0-0', invalid token: 'AUD/USD.' at position 0",
            ],
            [
                "AUD/USD.SIM-a-0-0-0",
                "Error parsing `BarType` from 'AUD/USD.SIM-a-0-0-0', invalid token: 'a' at position 1",
            ],
            [
                "AUD/USD.SIM-1000-a-0-0",
                "Error parsing `BarType` from 'AUD/USD.SIM-1000-a-0-0', invalid token: 'a' at position 2",
            ],
            [
                "AUD/USD.SIM-1000-TICK-a-0",
                "Error parsing `BarType` from 'AUD/USD.SIM-1000-TICK-a-0', invalid token: 'a' at position 3",
            ],
            [
                "AUD/USD.SIM-1000-TICK-LAST-a",
                "Error parsing `BarType` from 'AUD/USD.SIM-1000-TICK-LAST-a', invalid token: 'a' at position 4",
            ],
        ],
    )
    def test_bar_type_from_str_with_invalid_values(self, input: str, expected_err: str) -> None:
        # Arrange, Act
        with pytest.raises(ValueError) as exc_info:
            BarType.from_str(input)

        assert str(exc_info.value) == expected_err

    @pytest.mark.parametrize(
        "value",
        [
            "",
            "AUD/USD",
            "AUD/USD.IDEALPRO-1-MILLISECOND-BID",
        ],
    )
    def test_from_str_given_various_invalid_strings_raises_value_error(self, value):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            BarType.from_str(value)

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [
                "AUD/USD.IDEALPRO-1-MINUTE-BID-EXTERNAL",
                BarType(
                    InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO")),
                    BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
                ),
            ],
            [
                "GBP/USD.SIM-1000-TICK-MID-INTERNAL",
                BarType(
                    InstrumentId(Symbol("GBP/USD"), Venue("SIM")),
                    BarSpecification(1000, BarAggregation.TICK, PriceType.MID),
                    AggregationSource.INTERNAL,
                ),
            ],
            [
                "AAPL.NYSE-1-HOUR-MID-INTERNAL",
                BarType(
                    InstrumentId(Symbol("AAPL"), Venue("NYSE")),
                    BarSpecification(1, BarAggregation.HOUR, PriceType.MID),
                    AggregationSource.INTERNAL,
                ),
            ],
            [
                "BTCUSDT.BINANCE-100-TICK-LAST-INTERNAL",
                BarType(
                    InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
                    BarSpecification(100, BarAggregation.TICK, PriceType.LAST),
                    AggregationSource.INTERNAL,
                ),
            ],
            [
                "ETHUSDT-PERP.BINANCE-100-TICK-LAST-INTERNAL",
                BarType(
                    InstrumentId(Symbol("ETHUSDT-PERP"), Venue("BINANCE")),
                    BarSpecification(100, BarAggregation.TICK, PriceType.LAST),
                    AggregationSource.INTERNAL,
                ),
            ],
            [
                "TOTAL-INDEX.TRADINGVIEW-2-HOUR-LAST-EXTERNAL",
                BarType(
                    InstrumentId(Symbol("TOTAL-INDEX"), Venue("TRADINGVIEW")),
                    BarSpecification(2, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
        ],
    )
    def test_from_str_given_various_valid_string_returns_expected_specification(
        self,
        value,
        expected,
    ):
        # Arrange, Act
        bar_type = BarType.from_str(value)

        # Assert
        assert expected == bar_type

    def test_bar_type_from_str_with_utf8_symbol(self):
        # Arrange
        non_ascii_instrument = "TËST-PÉRP.BINANCE"
        non_ascii_bar_type = "TËST-PÉRP.BINANCE-1-MINUTE-LAST-EXTERNAL"

        # Act
        bar_type = BarType.from_str(non_ascii_bar_type)

        # Assert
        assert bar_type.instrument_id == InstrumentId.from_str(non_ascii_instrument)
        assert bar_type.spec == BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
        assert bar_type.aggregation_source == AggregationSource.EXTERNAL
        assert str(bar_type) == non_ascii_bar_type

    def test_properties(self):
        # Arrange, Act
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type = BarType(instrument_id, bar_spec, AggregationSource.EXTERNAL)

        # Assert
        assert bar_type.instrument_id == instrument_id
        assert bar_type.spec == bar_spec
        assert bar_type.aggregation_source == AggregationSource.EXTERNAL


class TestBar:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert Bar.fully_qualified_name() == "nautilus_trader.model.data:Bar"

    def test_validation_when_high_below_open_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Bar(
                AUDUSD_1_MIN_BID,
                Price.from_str("1.00001"),
                Price.from_str("1.00000"),  # <-- High below open
                Price.from_str("1.00000"),
                Price.from_str("1.00000"),
                Quantity.from_int(100_000),
                0,
                0,
            )

    def test_validation_when_high_below_low_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Bar(
                AUDUSD_1_MIN_BID,
                Price.from_str("1.00001"),
                Price.from_str("1.00000"),  # <-- High below low
                Price.from_str("1.00002"),
                Price.from_str("1.00003"),
                Quantity.from_int(100_000),
                0,
                0,
            )

    def test_validation_when_high_below_close_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Bar(
                AUDUSD_1_MIN_BID,
                Price.from_str("1.00000"),
                Price.from_str("1.00000"),  # <-- High below close
                Price.from_str("1.00000"),
                Price.from_str("1.00001"),
                Quantity.from_int(100_000),
                0,
                0,
            )

    def test_validation_when_low_above_close_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Bar(
                AUDUSD_1_MIN_BID,
                Price.from_str("1.00000"),
                Price.from_str("1.00005"),
                Price.from_str("1.00000"),
                Price.from_str("0.99999"),  # <-- Close below low
                Quantity.from_int(100_000),
                0,
                0,
            )

    def test_validation_when_low_above_open_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Bar(
                AUDUSD_1_MIN_BID,
                Price.from_str("0.99999"),  # <-- Open below low
                Price.from_str("1.00000"),
                Price.from_str("1.00000"),
                Price.from_str("1.00000"),
                Quantity.from_int(100_000),
                0,
                0,
            )

    def test_equality(self):
        # Arrange
        bar1 = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00001"),
            Price.from_str("1.00001"),
            Quantity.from_int(100_000),
            0,
            0,
        )

        bar2 = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00000"),
            Price.from_str("1.00004"),
            Price.from_str("1.00000"),
            Price.from_str("1.00003"),
            Quantity.from_int(100_000),
            0,
            0,
        )

        # Act, Assert
        assert bar1 == bar1
        assert bar1 != bar2

    def test_hash_str_repr(self):
        # Arrange
        bar = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00000"),
            Price.from_str("1.00003"),
            Quantity.from_int(100_000),
            0,
            0,
        )

        # Act, Assert
        assert isinstance(hash(bar), int)
        assert (
            str(bar) == "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL,1.00001,1.00004,1.00000,1.00003,100000,0"
        )
        assert (
            repr(bar)
            == "Bar(AUD/USD.SIM-1-MINUTE-BID-EXTERNAL,1.00001,1.00004,1.00000,1.00003,100000,0)"
        )

    def test_is_single_price(self):
        # Arrange
        bar1 = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Quantity.from_int(100_000),
            0,
            0,
        )

        bar2 = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00000"),
            Price.from_str("1.00004"),
            Price.from_str("1.00000"),
            Price.from_str("1.00003"),
            Quantity.from_int(100_000),
            0,
            0,
        )

        # Act, Assert
        assert bar1.is_single_price()
        assert not bar2.is_single_price()

    def test_to_dict(self):
        # Arrange
        bar = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00000"),
            Price.from_str("1.00003"),
            Quantity.from_int(100_000),
            0,
            0,
        )

        # Act
        values = Bar.to_dict(bar)

        # Assert
        assert values == {
            "type": "Bar",
            "bar_type": "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL",
            "open": "1.00001",
            "high": "1.00004",
            "low": "1.00000",
            "close": "1.00003",
            "volume": "100000",
            "ts_event": 0,
            "ts_init": 0,
        }

    def test_from_raw_returns_expected_bar(self):
        # Arrange
        bar_type = BarType.from_str("EUR/USD.IDEALPRO-5-MINUTE-MID-EXTERNAL")
        open_price = 1.06210
        high_price = 1.06355
        low_price = 1.06205
        close_price = 1.06320
        precision = 5

        raw_bar = [
            bar_type,
            convert_to_raw_int(open_price, precision),
            convert_to_raw_int(high_price, precision),
            convert_to_raw_int(low_price, precision),
            convert_to_raw_int(close_price, precision),
            precision,
            convert_to_raw_int(100_000, 0),
            0,
            1672012800000000000,
            1672013100300000000,
        ]

        # Act
        result = Bar.from_raw(*raw_bar)

        # Assert
        assert result.bar_type == bar_type
        assert result.volume == Quantity.from_int(100_000)
        assert result.open.raw == raw_bar[1]
        assert result.high.raw == raw_bar[2]
        assert result.low.raw == raw_bar[3]
        assert result.close.raw == raw_bar[4]
        assert result.ts_event == 1672012800000000000
        assert result.ts_init == 1672013100300000000

    def test_from_dict_returns_expected_bar(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()

        # Act
        result = Bar.from_dict(Bar.to_dict(bar))

        # Assert
        assert result == bar

    def test_from_pyo3(self):
        # Arrange
        pyo3_bar = TestDataProviderPyo3.bar_5decimal()

        # Act
        bar = Bar.from_pyo3(pyo3_bar)

        # Assert
        assert isinstance(bar, Bar)

    def test_to_pyo3(self):
        # Arrange
        bar = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Quantity.from_int(100_000),
            1,
            2,
        )

        # Act
        pyo3_bar = bar.to_pyo3()

        # Assert
        assert isinstance(pyo3_bar, nautilus_pyo3.Bar)
        assert pyo3_bar.open == nautilus_pyo3.Price.from_str("1.00000")
        assert pyo3_bar.high == nautilus_pyo3.Price.from_str("1.00000")
        assert pyo3_bar.low == nautilus_pyo3.Price.from_str("1.00000")
        assert pyo3_bar.close == nautilus_pyo3.Price.from_str("1.00000")
        assert pyo3_bar.volume == nautilus_pyo3.Quantity.from_int(100_000)
        assert pyo3_bar.ts_event == 1
        assert pyo3_bar.ts_init == 2

    def test_from_pyo3_list(self):
        # Arrange
        pyo3_bars = [TestDataProviderPyo3.bar_5decimal()] * 1024

        # Act
        bars = Bar.from_pyo3_list(pyo3_bars)

        # Assert
        assert len(bars) == 1024
        assert isinstance(bars[0], Bar)

    def test_pickle_bar(self):
        # Arrange
        bar = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00000"),
            Price.from_str("1.00003"),
            Quantity.from_int(100_000),
            0,
            0,
        )

        # Act
        pickled = pickle.dumps(bar)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert unpickled == bar

    def test_bar_type_composite_parse_valid(self):
        input_str = "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL"
        bar_type = BarType.from_str(input_str)
        standard = bar_type.standard()
        composite = bar_type.composite()
        composite_input = "BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL"

        assert bar_type.instrument_id == InstrumentId.from_str("BTCUSDT-PERP.BINANCE")
        assert bar_type.spec == BarSpecification(
            step=2,
            aggregation=BarAggregation.MINUTE,
            price_type=PriceType.LAST,
        )
        assert bar_type.aggregation_source == AggregationSource.INTERNAL
        assert bar_type == BarType.from_str(input_str)
        assert bar_type.is_composite()

        assert standard.instrument_id == InstrumentId.from_str("BTCUSDT-PERP.BINANCE")
        assert standard.spec == BarSpecification(
            step=2,
            aggregation=BarAggregation.MINUTE,
            price_type=PriceType.LAST,
        )
        assert standard.aggregation_source == AggregationSource.INTERNAL
        assert standard.is_standard()

        assert composite.instrument_id == InstrumentId.from_str("BTCUSDT-PERP.BINANCE")
        assert composite.spec == BarSpecification(
            step=1,
            aggregation=BarAggregation.MINUTE,
            price_type=PriceType.LAST,
        )
        assert composite.aggregation_source == AggregationSource.EXTERNAL
        assert composite == BarType.from_str(composite_input)
        assert composite.is_standard()
