# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestStubs.audusd_id()
GBPUSD_SIM = TestStubs.gbpusd_id()
ONE_MIN_BID = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
AUDUSD_1_MIN_BID = BarType(AUDUSD_SIM, ONE_MIN_BID)
GBPUSD_1_MIN_BID = BarType(GBPUSD_SIM, ONE_MIN_BID)


class TestBarSpecification:
    def test_bar_spec_equality(self):
        # Arrange
        bar_spec1 = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_spec2 = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_spec3 = BarSpecification(1, BarAggregation.MINUTE, PriceType.ASK)

        # Act
        # Assert
        assert bar_spec1 == bar_spec1
        assert bar_spec1 == bar_spec2
        assert bar_spec1 != bar_spec3

    def test_bar_spec_hash_str_and_repr(self):
        # Arrange
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)

        # Act
        # Assert
        assert isinstance(hash(bar_spec), int)
        assert str(bar_spec) == "1-MINUTE-BID"
        assert repr(bar_spec) == "BarSpecification(1-MINUTE-BID)"

    @pytest.mark.parametrize(
        "value",
        ["", "1", "-1-TICK-MID", "1-TICK_MID"],
    )
    def test_from_str_given_various_invalid_strings_raises_value_error(self, value):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            BarSpecification.from_str(value)

    @pytest.mark.parametrize(
        "value, expected",
        [
            ["1-MINUTE-BID", BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)],
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
        self, value, expected
    ):
        # Arrange
        # Act
        spec = BarSpecification.from_str(value)

        # Assert
        assert spec == expected

    @pytest.mark.parametrize(
        "bar_spec, is_time_aggregated, is_threshold_aggregated, is_information_aggregated",
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
        # Arrange
        # Act
        # Assert
        assert is_time_aggregated == bar_spec.is_time_aggregated()
        assert is_threshold_aggregated == bar_spec.is_threshold_aggregated()
        assert is_information_aggregated == bar_spec.is_information_aggregated()


class TestBarType:
    def test_bar_type_equality(self):
        # Arrange
        instrument_id1 = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        instrument_id2 = InstrumentId(Symbol("GBP/USD"), Venue("SIM"))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type1 = BarType(instrument_id1, bar_spec)
        bar_type2 = BarType(instrument_id1, bar_spec)
        bar_type3 = BarType(instrument_id2, bar_spec)

        # Act
        # Assert
        assert bar_type1 == bar_type1
        assert bar_type1 == bar_type2
        assert bar_type1 != bar_type3

    def test_bar_type_hash_str_and_repr(self):
        # Arrange
        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type = BarType(instrument_id, bar_spec)

        # Act
        # Assert
        assert isinstance(hash(bar_type), int)
        assert str(bar_type) == "AUD/USD.SIM-1-MINUTE-BID"
        assert repr(bar_type) == "BarType(AUD/USD.SIM-1-MINUTE-BID, internal_aggregation=True)"

    @pytest.mark.parametrize(
        "value",
        ["", "AUD/USD", "AUD/USD.IDEALPRO-1-MILLISECOND-BID"],
    )
    def test_from_str_given_various_invalid_strings_raises_value_error(self, value):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            BarType.from_str(value)

    @pytest.mark.parametrize(
        "value, expected",
        [
            [
                "AUD/USD.IDEALPRO-1-MINUTE-BID",
                BarType(
                    InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO")),
                    BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
                ),
            ],  # noqa
            [
                "GBP/USD.SIM-1000-TICK-MID",
                BarType(
                    InstrumentId(Symbol("GBP/USD"), Venue("SIM")),
                    BarSpecification(1000, BarAggregation.TICK, PriceType.MID),
                ),
            ],  # noqa
            [
                "AAPL.NYSE-1-HOUR-MID",
                BarType(
                    InstrumentId(Symbol("AAPL"), Venue("NYSE")),
                    BarSpecification(1, BarAggregation.HOUR, PriceType.MID),
                ),
            ],  # noqa
            [
                "BTC/USDT.BINANCE-100-TICK-LAST",
                BarType(
                    InstrumentId(Symbol("BTC/USDT"), Venue("BINANCE")),
                    BarSpecification(100, BarAggregation.TICK, PriceType.LAST),
                ),
            ],
        ],  # noqa
    )
    def test_from_str_given_various_valid_string_returns_expected_specification(
        self, value, expected
    ):
        # Arrange
        # Act
        bar_type = BarType.from_str(value, internal_aggregation=True)

        # Assert
        assert expected == bar_type


class TestBar:
    def test_check_when_high_below_low_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            Bar(
                AUDUSD_1_MIN_BID,
                Price.from_str("1.00001"),
                Price.from_str("1.00000"),  # High below low
                Price.from_str("1.00002"),
                Price.from_str("1.00003"),
                Quantity.from_int(100000),
                0,
                0,
                True,
            )

    def test_check_when_high_below_close_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            Bar(
                AUDUSD_1_MIN_BID,
                Price.from_str("1.00000"),
                Price.from_str("1.00000"),  # High below close
                Price.from_str("1.00000"),
                Price.from_str("1.00005"),
                Quantity.from_int(100000),
                0,
                0,
                True,
            )

    def test_check_when_low_above_close_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            Bar(
                AUDUSD_1_MIN_BID,
                Price.from_str("1.00000"),
                Price.from_str("1.00005"),
                Price.from_str("1.00000"),
                Price.from_str("0.99999"),  # Close below low
                Quantity.from_int(100000),
                0,
                0,
                True,
            )

    def test_equality(self):
        # Arrange
        bar1 = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Quantity.from_int(100000),
            0,
            0,
        )

        bar2 = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00000"),
            Price.from_str("1.00004"),
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Quantity.from_int(100000),
            0,
            0,
        )

        # Act
        # Assert
        assert bar1 == bar1
        assert bar1 != bar2

    def test_hash_str_repr(self):
        # Arrange
        bar = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Quantity.from_int(100000),
            0,
            0,
        )

        # Act
        # Assert
        assert isinstance(hash(bar), int)
        assert str(bar) == "AUD/USD.SIM-1-MINUTE-BID,1.00001,1.00004,1.00002,1.00003,100000,0"
        assert repr(bar) == "Bar(AUD/USD.SIM-1-MINUTE-BID,1.00001,1.00004,1.00002,1.00003,100000,0)"

    def test_to_dict(self):
        # Arrange
        bar = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Quantity.from_int(100000),
            0,
            0,
        )

        # Act
        values = Bar.to_dict(bar)

        # Assert
        assert values == {
            "type": "Bar",
            "bar_type": "AUD/USD.SIM-1-MINUTE-BID",
            "open": "1.00001",
            "high": "1.00004",
            "low": "1.00002",
            "close": "1.00003",
            "volume": "100000",
            "ts_event_ns": 0,
            "ts_recv_ns": 0,
        }

    def test_from_dict_returns_expected_bar(self):
        # Arrange
        bar = TestStubs.bar_5decimal()

        # Act
        result = Bar.from_dict(Bar.to_dict(bar))

        # Assert
        assert result == bar
