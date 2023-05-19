# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from datetime import timedelta

import pytest

from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
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
        assert bar_spec3 < bar_spec1
        assert bar_spec1 > bar_spec3
        assert bar_spec1 >= bar_spec3

    def test_bar_spec_pickle(self):
        # Arrange
        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.LAST)

        # Act
        pickled = pickle.dumps(bar_spec)
        unpickled = pickle.loads(pickled)  # noqa S301 (pickle is safe here)

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
        "aggregation",
        [
            BarAggregation.TICK,
            BarAggregation.MONTH,
        ],
    )
    def test_timedelta_for_unsupported_aggregations_raises_value_error(self, aggregation):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            spec = BarSpecification(1, aggregation, price_type=PriceType.LAST)
            _ = spec.timedelta

    @pytest.mark.parametrize(
        ("step", "aggregation", "expected"),
        [
            [
                500,
                BarAggregation.MILLISECOND,
                timedelta(milliseconds=500),
            ],
            [
                10,
                BarAggregation.SECOND,
                timedelta(seconds=10),
            ],
            [
                5,
                BarAggregation.MINUTE,
                timedelta(minutes=5),
            ],
            [
                1,
                BarAggregation.HOUR,
                timedelta(hours=1),
            ],
            [
                1,
                BarAggregation.DAY,
                timedelta(days=1),
            ],
            [
                1,
                BarAggregation.WEEK,
                timedelta(days=7),
            ],
        ],
    )
    def test_timedelta_given_various_values_returns_expected(
        self,
        step,
        aggregation,
        expected,
    ):
        # Arrange, Act
        spec = BarSpecification(
            step=step,
            aggregation=aggregation,
            price_type=PriceType.LAST,
        )

        # Assert
        assert spec.timedelta == expected

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
                "300-MILLISECOND-LAST",
                BarSpecification(300, BarAggregation.MILLISECOND, PriceType.LAST),
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
        unpickled = pickle.loads(pickled)  # noqa S301 (pickle is safe here)

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
        "value",
        ["", "AUD/USD", "AUD/USD.IDEALPRO-1-MILLISECOND-BID"],
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
        assert Bar.fully_qualified_name() == "nautilus_trader.model.data.bar:Bar"

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

    def test_from_dict_returns_expected_bar(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()

        # Act
        result = Bar.from_dict(Bar.to_dict(bar))

        # Assert
        assert result == bar

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
        unpickled = pickle.loads(pickled)  # noqa S301 (pickle is safe here)

        # Assert
        assert unpickled == bar
