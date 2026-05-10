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

import pickle
from datetime import timedelta

import pytest

from nautilus_trader.model import AggregationSource
from nautilus_trader.model import Bar
from nautilus_trader.model import BarAggregation
from nautilus_trader.model import BarSpecification
from nautilus_trader.model import BarType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue


@pytest.fixture
def one_min_bid():
    return BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)


@pytest.fixture
def audusd_1_min_bid(audusd_id, one_min_bid):
    return BarType(audusd_id, one_min_bid)


def test_bar_spec_equality():
    spec1 = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
    spec2 = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
    spec3 = BarSpecification(1, BarAggregation.MINUTE, PriceType.ASK)

    assert spec1 == spec2
    assert spec1 != spec3


def test_bar_spec_hash_and_str():
    spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)

    assert isinstance(hash(spec), int)
    assert str(spec) == "1-MINUTE-BID"


def test_bar_spec_properties():
    spec = BarSpecification(1, BarAggregation.HOUR, PriceType.BID)

    assert spec.step == 1
    assert spec.aggregation == BarAggregation.HOUR
    assert spec.price_type == PriceType.BID


@pytest.mark.parametrize(
    ("step", "aggregation", "expected_str"),
    [
        (1, BarAggregation.MINUTE, "1-MINUTE-BID"),
        (5, BarAggregation.MINUTE, "5-MINUTE-BID"),
        (100, BarAggregation.TICK, "100-TICK-BID"),
        (1, BarAggregation.HOUR, "1-HOUR-BID"),
        (1, BarAggregation.DAY, "1-DAY-BID"),
    ],
)
def test_bar_spec_str_with_various_aggregations(step, aggregation, expected_str):
    spec = BarSpecification(step, aggregation, PriceType.BID)
    assert str(spec) == expected_str


@pytest.mark.parametrize(
    ("step", "aggregation", "expected"),
    [
        (500, BarAggregation.MILLISECOND, timedelta(milliseconds=500)),
        (10, BarAggregation.SECOND, timedelta(seconds=10)),
        (5, BarAggregation.MINUTE, timedelta(minutes=5)),
        (1, BarAggregation.HOUR, timedelta(hours=1)),
        (1, BarAggregation.DAY, timedelta(days=1)),
    ],
)
def test_bar_spec_timedelta(step, aggregation, expected):
    spec = BarSpecification(step, aggregation, PriceType.LAST)

    assert spec.timedelta == expected


def test_bar_type_equality(audusd_id, one_min_bid):
    bt1 = BarType(audusd_id, one_min_bid)
    bt2 = BarType(audusd_id, one_min_bid)
    bt3 = BarType(InstrumentId(Symbol("GBP/USD"), Venue("SIM")), one_min_bid)

    assert bt1 == bt2
    assert bt1 != bt3


def test_bar_type_hash(audusd_id, one_min_bid):
    bt = BarType(audusd_id, one_min_bid)
    assert isinstance(hash(bt), int)


def test_bar_type_str(audusd_id, one_min_bid):
    bt = BarType(audusd_id, one_min_bid)

    assert str(bt) == "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"


def test_bar_type_from_str(audusd_id):
    bar_type = BarType.from_str("AUD/USD.SIM-1-MINUTE-BID-INTERNAL")

    assert bar_type.spec.step == 1
    assert bar_type.spec.aggregation == BarAggregation.MINUTE
    assert bar_type.spec.price_type == PriceType.BID


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (
            "AUD/USD.IDEALPRO-1-MINUTE-BID-EXTERNAL",
            BarType(
                InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO")),
                BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
            ),
        ),
        (
            "GBP/USD.SIM-1000-TICK-MID-INTERNAL",
            BarType(
                InstrumentId(Symbol("GBP/USD"), Venue("SIM")),
                BarSpecification(1000, BarAggregation.TICK, PriceType.MID),
                AggregationSource.INTERNAL,
            ),
        ),
        (
            "AAPL.NYSE-1-HOUR-MID-INTERNAL",
            BarType(
                InstrumentId(Symbol("AAPL"), Venue("NYSE")),
                BarSpecification(1, BarAggregation.HOUR, PriceType.MID),
                AggregationSource.INTERNAL,
            ),
        ),
        (
            "ETHUSDT-PERP.BINANCE-100-TICK-LAST-INTERNAL",
            BarType(
                InstrumentId(Symbol("ETHUSDT-PERP"), Venue("BINANCE")),
                BarSpecification(100, BarAggregation.TICK, PriceType.LAST),
                AggregationSource.INTERNAL,
            ),
        ),
    ],
)
def test_bar_type_from_str_valid(value, expected):
    assert BarType.from_str(value) == expected


@pytest.mark.parametrize(
    "value",
    ["", "AUD/USD", "AUD/USD.IDEALPRO-1-MILLISECOND-BID"],
)
def test_bar_type_from_str_invalid(value):
    with pytest.raises(ValueError, match="Error parsing"):
        BarType.from_str(value)


def test_bar_type_from_str_with_utf8():
    bar_type = BarType.from_str("TËST-PÉRP.BINANCE-1-MINUTE-LAST-EXTERNAL")

    assert bar_type.spec == BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
    assert str(bar_type) == "TËST-PÉRP.BINANCE-1-MINUTE-LAST-EXTERNAL"


def test_bar_type_composite():
    bt = BarType.from_str("BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL")

    assert bt.is_composite()
    assert str(bt) == "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL"

    std = bt.standard()
    assert std.is_standard()
    assert str(std) == "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL"

    comp = bt.composite()
    assert comp.is_standard()
    assert str(comp) == "BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL"


def test_bar_fully_qualified_name():
    module_name, _, type_name = Bar.fully_qualified_name().partition(":")

    assert module_name
    assert type_name == "Bar"
    assert Bar.__module__ == "nautilus_trader.model"


def test_bar_construction(audusd_1_min_bid):
    bar = Bar(
        bar_type=audusd_1_min_bid,
        open=Price.from_str("1.00001"),
        high=Price.from_str("1.00010"),
        low=Price.from_str("1.00000"),
        close=Price.from_str("1.00002"),
        volume=Quantity.from_int(100_000),
        ts_event=1,
        ts_init=2,
    )

    assert bar.bar_type == audusd_1_min_bid
    assert bar.open == Price.from_str("1.00001")
    assert bar.high == Price.from_str("1.00010")
    assert bar.low == Price.from_str("1.00000")
    assert bar.close == Price.from_str("1.00002")
    assert bar.volume == Quantity.from_int(100_000)
    assert bar.ts_event == 1
    assert bar.ts_init == 2


def test_bar_equality(audusd_1_min_bid):
    bar1 = Bar(
        audusd_1_min_bid,
        Price.from_str("1.00001"),
        Price.from_str("1.00004"),
        Price.from_str("1.00001"),
        Price.from_str("1.00001"),
        Quantity.from_int(100_000),
        0,
        0,
    )
    bar2 = Bar(
        audusd_1_min_bid,
        Price.from_str("1.00000"),
        Price.from_str("1.00004"),
        Price.from_str("1.00000"),
        Price.from_str("1.00003"),
        Quantity.from_int(100_000),
        0,
        0,
    )

    assert bar1 == bar1
    assert bar1 != bar2


def test_bar_hash(audusd_1_min_bid):
    bar = Bar(
        audusd_1_min_bid,
        Price.from_str("1.00001"),
        Price.from_str("1.00010"),
        Price.from_str("1.00000"),
        Price.from_str("1.00002"),
        Quantity.from_int(100_000),
        0,
        0,
    )

    assert isinstance(hash(bar), int)


def test_bar_str(audusd_1_min_bid):
    bar = Bar(
        audusd_1_min_bid,
        Price.from_str("1.00001"),
        Price.from_str("1.00004"),
        Price.from_str("1.00000"),
        Price.from_str("1.00003"),
        Quantity.from_int(100_000),
        0,
        0,
    )

    assert str(bar) == "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL,1.00001,1.00004,1.00000,1.00003,100000,0"


def test_bar_validation_high_below_open(audusd_1_min_bid):
    with pytest.raises(ValueError, match="high >= open"):
        Bar(
            audusd_1_min_bid,
            Price.from_str("1.00001"),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Quantity.from_int(100_000),
            0,
            0,
        )


def test_bar_validation_high_below_low(audusd_1_min_bid):
    with pytest.raises(ValueError, match="high >= open"):
        Bar(
            audusd_1_min_bid,
            Price.from_str("1.00001"),
            Price.from_str("1.00000"),
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Quantity.from_int(100_000),
            0,
            0,
        )


def test_bar_validation_high_below_close(audusd_1_min_bid):
    with pytest.raises(ValueError, match="high >= close"):
        Bar(
            audusd_1_min_bid,
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(100_000),
            0,
            0,
        )


def test_bar_validation_low_above_open(audusd_1_min_bid):
    with pytest.raises(ValueError, match="low <= open"):
        Bar(
            audusd_1_min_bid,
            Price.from_str("0.99999"),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
            Quantity.from_int(100_000),
            0,
            0,
        )


def test_bar_validation_low_above_close(audusd_1_min_bid):
    with pytest.raises(ValueError, match="low <= close"):
        Bar(
            audusd_1_min_bid,
            Price.from_str("1.00000"),
            Price.from_str("1.00005"),
            Price.from_str("1.00000"),
            Price.from_str("0.99999"),
            Quantity.from_int(100_000),
            0,
            0,
        )


def test_bar_to_dict(audusd_1_min_bid):
    bar = Bar(
        audusd_1_min_bid,
        Price.from_str("1.00001"),
        Price.from_str("1.00004"),
        Price.from_str("1.00000"),
        Price.from_str("1.00003"),
        Quantity.from_int(100_000),
        0,
        0,
    )

    assert bar.to_dict() == {
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


def test_bar_from_dict_roundtrip(audusd_1_min_bid):
    bar = Bar(
        audusd_1_min_bid,
        Price.from_str("1.00001"),
        Price.from_str("1.00010"),
        Price.from_str("1.00000"),
        Price.from_str("1.00002"),
        Quantity.from_int(100_000),
        1,
        2,
    )

    restored = Bar.from_dict(bar.to_dict())

    assert restored == bar


def test_bar_spec_pickle_roundtrip():
    spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
    restored = pickle.loads(pickle.dumps(spec))  # noqa: S301

    assert restored == spec
    assert restored.step == 1
    assert restored.aggregation == BarAggregation.MINUTE
    assert restored.price_type == PriceType.BID


@pytest.mark.parametrize(
    ("aggregation", "expected"),
    [
        (BarAggregation.MILLISECOND, True),
        (BarAggregation.SECOND, True),
        (BarAggregation.MINUTE, True),
        (BarAggregation.HOUR, True),
        (BarAggregation.DAY, True),
        (BarAggregation.WEEK, True),
        (BarAggregation.MONTH, True),
        (BarAggregation.TICK, False),
        (BarAggregation.VOLUME, False),
        (BarAggregation.VALUE, False),
        (BarAggregation.TICK_IMBALANCE, False),
        (BarAggregation.TICK_RUNS, False),
    ],
)
def test_bar_spec_is_time_aggregated(aggregation, expected):
    spec = BarSpecification(1, aggregation, PriceType.LAST)
    assert spec.is_time_aggregated() == expected


@pytest.mark.parametrize(
    ("aggregation", "expected"),
    [
        (BarAggregation.TICK, True),
        (BarAggregation.TICK_IMBALANCE, True),
        (BarAggregation.VOLUME, True),
        (BarAggregation.VOLUME_IMBALANCE, True),
        (BarAggregation.VALUE, True),
        (BarAggregation.VALUE_IMBALANCE, True),
        (BarAggregation.MINUTE, False),
        (BarAggregation.TICK_RUNS, False),
    ],
)
def test_bar_spec_is_threshold_aggregated(aggregation, expected):
    spec = BarSpecification(1, aggregation, PriceType.LAST)
    assert spec.is_threshold_aggregated() == expected


@pytest.mark.parametrize(
    ("aggregation", "expected"),
    [
        (BarAggregation.TICK_RUNS, True),
        (BarAggregation.VOLUME_RUNS, True),
        (BarAggregation.VALUE_RUNS, True),
        (BarAggregation.MINUTE, False),
        (BarAggregation.TICK, False),
        (BarAggregation.VOLUME, False),
        (BarAggregation.TICK_IMBALANCE, False),
    ],
)
def test_bar_spec_is_information_aggregated(aggregation, expected):
    spec = BarSpecification(1, aggregation, PriceType.LAST)
    assert spec.is_information_aggregated() == expected


@pytest.mark.parametrize(
    ("step", "aggregation", "expected_ns"),
    [
        (500, BarAggregation.MILLISECOND, 500_000_000),
        (10, BarAggregation.SECOND, 10_000_000_000),
        (5, BarAggregation.MINUTE, 300_000_000_000),
        (1, BarAggregation.HOUR, 3_600_000_000_000),
        (1, BarAggregation.DAY, 86_400_000_000_000),
    ],
)
def test_bar_spec_get_interval_ns(step, aggregation, expected_ns):
    spec = BarSpecification(step, aggregation, PriceType.LAST)
    assert spec.get_interval_ns() == expected_ns


def test_bar_spec_from_timedelta():
    spec = BarSpecification.from_timedelta(timedelta(minutes=5), PriceType.MID)

    assert spec.step == 5
    assert spec.aggregation == BarAggregation.MINUTE
    assert spec.price_type == PriceType.MID


@pytest.mark.parametrize(
    ("duration", "expected_step", "expected_agg"),
    [
        (timedelta(milliseconds=250), 250, BarAggregation.MILLISECOND),
        (timedelta(seconds=30), 30, BarAggregation.SECOND),
        (timedelta(hours=4), 4, BarAggregation.HOUR),
        (timedelta(days=1), 1, BarAggregation.DAY),
    ],
)
def test_bar_spec_from_timedelta_various(duration, expected_step, expected_agg):
    spec = BarSpecification.from_timedelta(duration, PriceType.LAST)

    assert spec.step == expected_step
    assert spec.aggregation == expected_agg


@pytest.mark.parametrize(
    ("aggregation", "expected"),
    [
        (BarAggregation.MILLISECOND, True),
        (BarAggregation.MINUTE, True),
        (BarAggregation.TICK, False),
        (BarAggregation.VOLUME, False),
    ],
)
def test_bar_spec_check_time_aggregated(aggregation, expected):
    assert BarSpecification.check_time_aggregated(aggregation) == expected


@pytest.mark.parametrize(
    ("aggregation", "expected"),
    [
        (BarAggregation.TICK, True),
        (BarAggregation.VOLUME, True),
        (BarAggregation.VALUE, True),
        (BarAggregation.MINUTE, False),
    ],
)
def test_bar_spec_check_threshold_aggregated(aggregation, expected):
    assert BarSpecification.check_threshold_aggregated(aggregation) == expected


@pytest.mark.parametrize(
    ("aggregation", "expected"),
    [
        (BarAggregation.TICK_RUNS, True),
        (BarAggregation.VOLUME_RUNS, True),
        (BarAggregation.VALUE_RUNS, True),
        (BarAggregation.TICK, False),
        (BarAggregation.MINUTE, False),
    ],
)
def test_bar_spec_check_information_aggregated(aggregation, expected):
    assert BarSpecification.check_information_aggregated(aggregation) == expected


def test_bar_type_pickle_roundtrip(audusd_id, one_min_bid):
    bar_type = BarType(audusd_id, one_min_bid)
    restored = pickle.loads(pickle.dumps(bar_type))  # noqa: S301

    assert restored == bar_type
    assert str(restored) == str(bar_type)
    assert restored.instrument_id == audusd_id
    assert restored.spec == one_min_bid


def test_bar_type_composite_pickle_roundtrip():
    bar_type = BarType.from_str(
        "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL",
    )
    restored = pickle.loads(pickle.dumps(bar_type))  # noqa: S301

    assert restored == bar_type
    assert restored.is_composite()
    assert str(restored) == str(bar_type)


def test_bar_type_is_standard(audusd_id, one_min_bid):
    bar_type = BarType(audusd_id, one_min_bid)
    assert bar_type.is_standard() is True
    assert bar_type.is_composite() is False


def test_bar_type_is_composite_from_str():
    bar_type = BarType.from_str(
        "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL",
    )
    assert bar_type.is_composite() is True
    assert bar_type.is_standard() is False


def test_bar_type_is_externally_aggregated(audusd_id, one_min_bid):
    external = BarType(audusd_id, one_min_bid, AggregationSource.EXTERNAL)
    internal = BarType(audusd_id, one_min_bid, AggregationSource.INTERNAL)

    assert external.is_externally_aggregated() is True
    assert external.is_internally_aggregated() is False
    assert internal.is_internally_aggregated() is True
    assert internal.is_externally_aggregated() is False


def test_bar_type_standard_and_composite_accessors():
    bar_type = BarType.from_str(
        "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL",
    )
    std = bar_type.standard()
    comp = bar_type.composite()

    assert std.is_standard() is True
    assert std.spec.step == 2
    assert std.spec.aggregation == BarAggregation.MINUTE
    assert comp.is_standard() is True
    assert comp.spec.step == 1
    assert comp.spec.aggregation == BarAggregation.MINUTE
    assert comp.aggregation_source == AggregationSource.EXTERNAL


def test_bar_type_id_spec_key(audusd_id, one_min_bid):
    bt_ext = BarType(audusd_id, one_min_bid, AggregationSource.EXTERNAL)
    bt_int = BarType(audusd_id, one_min_bid, AggregationSource.INTERNAL)

    key_ext = bt_ext.id_spec_key()
    key_int = bt_int.id_spec_key()

    assert key_ext == (audusd_id, one_min_bid)
    assert key_ext == key_int


def test_bar_type_new_composite(audusd_id):
    bar_type = BarType.new_composite(
        instrument_id=audusd_id,
        spec=BarSpecification(5, BarAggregation.MINUTE, PriceType.BID),
        aggregation_source=AggregationSource.INTERNAL,
        composite_step=1,
        composite_aggregation=BarAggregation.MINUTE,
        composite_aggregation_source=AggregationSource.EXTERNAL,
    )

    assert bar_type.is_composite() is True
    assert bar_type.standard().spec.step == 5
    assert bar_type.composite().spec.step == 1


def test_bar_pickle_roundtrip(audusd_1_min_bid):
    bar = Bar(
        bar_type=audusd_1_min_bid,
        open=Price.from_str("1.00001"),
        high=Price.from_str("1.00010"),
        low=Price.from_str("1.00000"),
        close=Price.from_str("1.00002"),
        volume=Quantity.from_str("100000"),
        ts_event=1,
        ts_init=2,
    )

    restored = pickle.loads(pickle.dumps(bar))  # noqa: S301

    assert restored == bar
    assert restored.bar_type == bar.bar_type
    assert restored.open == bar.open
    assert restored.high == bar.high
    assert restored.low == bar.low
    assert restored.close == bar.close
    assert restored.volume == bar.volume
    assert restored.ts_event == 1
    assert restored.ts_init == 2


def test_bar_pickle_composite_bar_type():
    bar_type = BarType.from_str(
        "BTCUSDT-PERP.BINANCE-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL",
    )
    bar = Bar(
        bar_type=bar_type,
        open=Price.from_str("50000.0"),
        high=Price.from_str("50100.0"),
        low=Price.from_str("49900.0"),
        close=Price.from_str("50050.0"),
        volume=Quantity.from_str("10.5"),
        ts_event=1_000_000_000,
        ts_init=1_000_000_001,
    )

    restored = pickle.loads(pickle.dumps(bar))  # noqa: S301

    assert restored == bar
    assert restored.bar_type.is_composite()
    assert str(restored.bar_type) == str(bar.bar_type)
