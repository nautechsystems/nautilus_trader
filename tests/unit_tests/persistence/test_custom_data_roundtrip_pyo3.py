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


import numpy as np

from nautilus_trader.core.nautilus_pyo3.model import CustomData


def _assert_custom_data_json_roundtrip(result, inner_type, assert_fields):
    """
    Assert each CustomData in result roundtrips via to_json_bytes/from_json_bytes.
    """
    for item in result:
        json_bytes = item.to_json_bytes()
        roundtripped = CustomData.from_json_bytes(bytes(json_bytes))
        assert roundtripped.data_type.type_name == item.data_type.type_name
        assert roundtripped.data_type.metadata == item.data_type.metadata
        assert roundtripped.data_type.identifier == item.data_type.identifier
        inner = roundtripped.data
        assert isinstance(inner, inner_type)
        assert_fields(item.data, inner)


def test_python_custom_data_roundtrip(tmp_path):
    """Test PyO3 custom data roundtrip via catalog - same as Rust test but from Python."""
    from nautilus_trader.core.nautilus_pyo3 import InstrumentId
    from nautilus_trader.core.nautilus_pyo3 import ParquetDataCatalog
    from nautilus_trader.core.nautilus_pyo3.model import CustomData
    from nautilus_trader.core.nautilus_pyo3.model import DataType
    from nautilus_trader.core.nautilus_pyo3.model import custom_data_backend_kind
    from nautilus_trader.core.nautilus_pyo3.model import register_custom_data_class
    from nautilus_trader.core.nautilus_pyo3.persistence import RustTestCustomData

    register_custom_data_class(RustTestCustomData)
    catalog_path = tmp_path / "catalog_file"
    catalog_path.mkdir(parents=True, exist_ok=True)
    pyo3_catalog = ParquetDataCatalog(str(catalog_path))

    instrument_id = InstrumentId.from_str("RUST.TEST")
    metadata = {"venue": "TEST", "instrument_id": str(instrument_id)}
    data_type = DataType("RustTestCustomData", metadata, str(instrument_id))

    # Create RustTestCustomData instances (PyO3 class from Rust) and wrap with CustomData
    original_data = [
        RustTestCustomData(instrument_id, 1.23, True, 1, 1),
        RustTestCustomData(instrument_id, 4.56, False, 2, 2),
    ]
    wrapped = [CustomData(data_type, item) for item in original_data]
    assert [custom_data_backend_kind(item) for item in wrapped] == ["native", "native"]

    print(f"Writing {len(wrapped)} items...")

    # Write via pyo3 catalog - requires CustomData wrappers
    pyo3_catalog.write_custom_data(wrapped)

    print("Write successful!")

    # Read back via pyo3 catalog query
    print("Reading back data...")
    result = pyo3_catalog.query(
        "RustTestCustomData",
        [str(instrument_id)],
        None,
        None,
        None,
        None,
        True,
    )

    print(f"Read {len(result)} items")

    # Result should be CustomData wrappers with RustTestCustomData in .data
    roundtripped = []

    for item in result:
        assert isinstance(item, CustomData), f"Expected CustomData, found {type(item)}"
        assert custom_data_backend_kind(item) == "native"
        inner = item.data
        assert isinstance(inner, RustTestCustomData), (
            f"Expected RustTestCustomData in .data, found {type(inner)}"
        )
        roundtripped.append(inner)

    print(f"Original items: {original_data}")
    print(f"Roundtripped items: {roundtripped}")

    # Compare - RustTestCustomData should have equality implemented
    assert len(roundtripped) == len(original_data), (
        f"Expected {len(original_data)} items, found {len(roundtripped)}"
    )

    for expected, actual in zip(original_data, roundtripped, strict=True):
        assert expected.instrument_id == actual.instrument_id
        assert expected.value == actual.value
        assert expected.flag == actual.flag
        assert expected.ts_event == actual.ts_event
        assert expected.ts_init == actual.ts_init

    # DataType (type_name, metadata, identifier) should be restored on catalog decode
    for item in result:
        assert item.data_type.type_name == "RustTestCustomData"
        assert item.data_type.metadata == metadata
        assert item.data_type.identifier == str(instrument_id)

    # JSON roundtrip: CustomData -> JSON bytes -> CustomData.from_json_bytes
    def assert_rust_test(orig, rt):
        assert orig.instrument_id == rt.instrument_id
        assert orig.value == rt.value
        assert orig.flag == rt.flag
        assert orig.ts_event == rt.ts_event
        assert orig.ts_init == rt.ts_init

    _assert_custom_data_json_roundtrip(result, RustTestCustomData, assert_rust_test)

    # Regression: serialized CustomData must use canonical envelope { type, data_type, payload }
    # so payload with a field named "value" is not confused with wrapper metadata
    import json as json_module

    first = result[0]
    raw = json_module.loads(first.to_json_bytes().decode())
    assert "type" in raw
    assert "data_type" in raw
    assert "payload" in raw
    assert raw["payload"].get("value") == 1.23


def test_macro_yield_curve_data_roundtrip(tmp_path):
    """Test MacroYieldCurveData roundtrip via catalog - tests Vec<f64> and numpy interop."""
    from nautilus_trader.core.nautilus_pyo3 import ParquetDataCatalog
    from nautilus_trader.core.nautilus_pyo3.model import CustomData
    from nautilus_trader.core.nautilus_pyo3.model import DataType
    from nautilus_trader.core.nautilus_pyo3.model import register_custom_data_class
    from nautilus_trader.core.nautilus_pyo3.persistence import MacroYieldCurveData

    register_custom_data_class(MacroYieldCurveData)
    catalog_path = tmp_path / "catalog_file"
    catalog_path.mkdir(parents=True, exist_ok=True)
    pyo3_catalog = ParquetDataCatalog(str(catalog_path))

    metadata = {"currency": "USD", "curve": "govt"}
    identifier = "USD-GOVT"
    data_type = DataType("MacroYieldCurveData", metadata, identifier)

    tenors = np.array([0.25, 0.5, 1.0, 2.0, 5.0], dtype=np.float64)
    interest_rates = np.array([0.025, 0.03, 0.035, 0.04, 0.045], dtype=np.float64)

    original_data = [
        MacroYieldCurveData("USD", tenors.tolist(), interest_rates.tolist(), 1, 1),
        MacroYieldCurveData("EUR", [1.0, 2.0], [0.02, 0.025], 2, 2),
    ]
    wrapped = [CustomData(data_type, item) for item in original_data]

    pyo3_catalog.write_custom_data(wrapped)

    result = pyo3_catalog.query(
        "MacroYieldCurveData",
        None,
        None,
        None,
        None,
        None,
        True,
    )

    roundtripped = []

    for item in result:
        assert isinstance(item, CustomData), f"Expected CustomData, found {type(item)}"
        inner = item.data
        assert isinstance(inner, MacroYieldCurveData), (
            f"Expected MacroYieldCurveData in .data, found {type(inner)}"
        )
        roundtripped.append(inner)

    assert len(roundtripped) == len(original_data)

    # DataType (type_name, metadata, identifier) should be restored on catalog decode
    for item in result:
        assert item.data_type.type_name == "MacroYieldCurveData"
        assert item.data_type.metadata == metadata
        assert item.data_type.identifier == identifier

    for expected, actual in zip(original_data, roundtripped, strict=True):
        assert expected.curve_name == actual.curve_name
        np.testing.assert_array_almost_equal(expected.tenors, actual.tenors)
        np.testing.assert_array_almost_equal(expected.interest_rates, actual.interest_rates)
        assert expected.ts_event == actual.ts_event
        assert expected.ts_init == actual.ts_init

    # JSON roundtrip: CustomData -> JSON bytes -> CustomData.from_json_bytes
    def assert_yield_curve(orig, rt):
        assert orig.curve_name == rt.curve_name
        np.testing.assert_array_almost_equal(orig.tenors, rt.tenors)
        np.testing.assert_array_almost_equal(orig.interest_rates, rt.interest_rates)
        assert orig.ts_event == rt.ts_event
        assert orig.ts_init == rt.ts_init

    _assert_custom_data_json_roundtrip(result, MacroYieldCurveData, assert_yield_curve)


def test_rust_params_custom_data_roundtrip(tmp_path):
    """Test RustTestParamsCustomData roundtrip via catalog - exercises Params <-> dict interop."""
    from nautilus_trader.core.nautilus_pyo3 import ParquetDataCatalog
    from nautilus_trader.core.nautilus_pyo3.model import CustomData
    from nautilus_trader.core.nautilus_pyo3.model import DataType
    from nautilus_trader.core.nautilus_pyo3.model import register_custom_data_class
    from nautilus_trader.core.nautilus_pyo3.persistence import RustTestParamsCustomData

    register_custom_data_class(RustTestParamsCustomData)
    catalog_path = tmp_path / "catalog_params"
    catalog_path.mkdir(parents=True, exist_ok=True)
    pyo3_catalog = ParquetDataCatalog(str(catalog_path))

    metadata = {"source": "unit-test", "kind": "params"}
    data_type = DataType("RustTestParamsCustomData", metadata, None)

    original_params = [
        {
            "key": "alpha",
            "count": 1,
            "enabled": True,
        },
        {
            "key": "beta",
            "nested": {"level": 2, "tags": ["x", "y"]},
            "ratio": 1.25,
        },
    ]
    original_data = [
        RustTestParamsCustomData("first", original_params[0], 10, 10),
        RustTestParamsCustomData("second", original_params[1], 20, 20),
    ]
    wrapped = [CustomData(data_type, item) for item in original_data]

    pyo3_catalog.write_custom_data(wrapped)

    result = pyo3_catalog.query(
        "RustTestParamsCustomData",
        None,
        None,
        None,
        None,
        None,
        True,
    )

    roundtripped = []

    for item in result:
        assert isinstance(item, CustomData), f"Expected CustomData, found {type(item)}"
        assert item.data_type.type_name == "RustTestParamsCustomData"
        assert item.data_type.metadata == metadata
        assert item.data_type.identifier is None
        inner = item.data
        assert isinstance(inner, RustTestParamsCustomData), (
            f"Expected RustTestParamsCustomData in .data, found {type(inner)}"
        )
        assert isinstance(inner.params, dict)
        roundtripped.append(inner)

    assert len(roundtripped) == len(original_data)

    for expected, expected_params, actual in zip(
        original_data,
        original_params,
        roundtripped,
        strict=True,
    ):
        assert expected.name == actual.name
        assert actual.params == expected_params
        assert expected.ts_event == actual.ts_event
        assert expected.ts_init == actual.ts_init

    def assert_params_data(orig, rt):
        assert orig.name == rt.name
        assert rt.params == orig.params
        assert orig.ts_event == rt.ts_event
        assert orig.ts_init == rt.ts_init

    _assert_custom_data_json_roundtrip(
        result,
        RustTestParamsCustomData,
        assert_params_data,
    )


def test_rust_price_map_custom_data_roundtrip(tmp_path):
    """
    Test RustTestPriceMapCustomData roundtrip via catalog.
    """
    from nautilus_trader.core.nautilus_pyo3 import ParquetDataCatalog
    from nautilus_trader.core.nautilus_pyo3.model import CustomData
    from nautilus_trader.core.nautilus_pyo3.model import DataType
    from nautilus_trader.core.nautilus_pyo3.model import InstrumentId
    from nautilus_trader.core.nautilus_pyo3.model import Price
    from nautilus_trader.core.nautilus_pyo3.model import register_custom_data_class
    from nautilus_trader.core.nautilus_pyo3.persistence import RustTestPriceMapCustomData

    register_custom_data_class(RustTestPriceMapCustomData)
    catalog_path = tmp_path / "catalog_price_map"
    catalog_path.mkdir(parents=True, exist_ok=True)
    pyo3_catalog = ParquetDataCatalog(str(catalog_path))

    metadata = {"source": "unit-test", "kind": "price-map"}
    data_type = DataType("RustTestPriceMapCustomData", metadata, None)
    original_prices = [
        {
            "AUD/USD.SIM": "1.23456",
            "BTCUSDT.BINANCE": "65432.10",
        },
        {
            "BTCUSDT.BINANCE": "65433.20",
            "AUD/USD.SIM": "1.23457",
        },
    ]
    typed_prices = [
        {
            InstrumentId.from_str("AUD/USD.SIM"): Price.from_str("1.23456"),
            InstrumentId.from_str("BTCUSDT.BINANCE"): Price.from_str("65432.10"),
        },
        {
            InstrumentId.from_str("BTCUSDT.BINANCE"): Price.from_str("65433.20"),
            InstrumentId.from_str("AUD/USD.SIM"): Price.from_str("1.23457"),
        },
    ]
    original_data = [
        RustTestPriceMapCustomData("first", typed_prices[0], 10, 10),
        RustTestPriceMapCustomData("second", original_prices[1], 20, 20),
    ]
    wrapped = [CustomData(data_type, item) for item in original_data]

    for expected, item in zip(original_prices, original_data, strict=True):
        _assert_typed_price_map(item.prices, expected, InstrumentId, Price)

    pyo3_catalog.write_custom_data(wrapped)

    result = pyo3_catalog.query(
        "RustTestPriceMapCustomData",
        None,
        None,
        None,
        None,
        None,
        True,
    )

    roundtripped = []

    for item in result:
        assert isinstance(item, CustomData), f"Expected CustomData, found {type(item)}"
        assert item.data_type.type_name == "RustTestPriceMapCustomData"
        assert item.data_type.metadata == metadata
        assert item.data_type.identifier is None
        inner = item.data
        assert isinstance(inner, RustTestPriceMapCustomData), (
            f"Expected RustTestPriceMapCustomData in .data, found {type(inner)}"
        )
        assert isinstance(inner.prices, dict)
        _assert_typed_price_map(
            inner.prices,
            original_prices[len(roundtripped)],
            InstrumentId,
            Price,
        )
        roundtripped.append(inner)

    assert len(roundtripped) == len(original_data)

    for expected, expected_prices, actual in zip(
        original_data,
        original_prices,
        roundtripped,
        strict=True,
    ):
        assert expected.name == actual.name
        _assert_typed_price_map(actual.prices, expected_prices, InstrumentId, Price)
        assert expected.ts_event == actual.ts_event
        assert expected.ts_init == actual.ts_init

    def assert_price_map_data(orig, rt):
        assert orig.name == rt.name
        assert rt.prices == orig.prices
        assert orig.ts_event == rt.ts_event
        assert orig.ts_init == rt.ts_init

    _assert_custom_data_json_roundtrip(
        result,
        RustTestPriceMapCustomData,
        assert_price_map_data,
    )


def test_rust_typed_map_custom_data_roundtrip(tmp_path):
    """
    Test typed JSON map values roundtrip through PyO3, catalog, and JSON.
    """
    from nautilus_trader.core.nautilus_pyo3 import ParquetDataCatalog
    from nautilus_trader.core.nautilus_pyo3.model import AccountId
    from nautilus_trader.core.nautilus_pyo3.model import BarType
    from nautilus_trader.core.nautilus_pyo3.model import Currency
    from nautilus_trader.core.nautilus_pyo3.model import CustomData
    from nautilus_trader.core.nautilus_pyo3.model import DataType
    from nautilus_trader.core.nautilus_pyo3.model import InstrumentId
    from nautilus_trader.core.nautilus_pyo3.model import Money
    from nautilus_trader.core.nautilus_pyo3.model import Price
    from nautilus_trader.core.nautilus_pyo3.model import Quantity
    from nautilus_trader.core.nautilus_pyo3.model import register_custom_data_class
    from nautilus_trader.core.nautilus_pyo3.persistence import RustTestTypedMapCustomData

    register_custom_data_class(RustTestTypedMapCustomData)
    catalog_path = tmp_path / "catalog_typed_map"
    catalog_path.mkdir(parents=True, exist_ok=True)
    pyo3_catalog = ParquetDataCatalog(str(catalog_path))

    metadata = {"source": "unit-test", "kind": "typed-map"}
    data_type = DataType("RustTestTypedMapCustomData", metadata, None)
    bar_type = BarType.from_str("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL")
    original = RustTestTypedMapCustomData(
        "typed",
        {"primary": InstrumentId.from_str("AUD/USD.SIM")},
        {"primary": AccountId.from_str("SIM-001")},
        {"settlement": Currency.from_str("USD")},
        {"bar": bar_type},
        {"bid": Price.from_str("1.23456")},
        {"size": Quantity.from_str("10.500")},
        {"notional": Money.from_str("123.45 USD")},
        {InstrumentId.from_str("AUD/USD.SIM"): Price.from_str("1.23456")},
        {AccountId.from_str("SIM-001"): Quantity.from_str("10.500")},
        {Currency.from_str("USD"): Money.from_str("123.45 USD")},
        {bar_type: Price.from_str("1.23456")},
        {InstrumentId.from_str("AUD/USD.SIM"): Price.from_str("1.23456")},
        {"label": "alpha"},
        {"ratio": 1.5},
        {"ratio": 1.5},
        {"enabled": True},
        {"count": 7},
        {"delta": -7},
        {"count": 5},
        {"delta": -5},
        10,
        10,
    )
    wrapped = [CustomData(data_type, original)]

    _assert_typed_value_map(original.instrument_ids, {"primary": "AUD/USD.SIM"}, InstrumentId)
    _assert_typed_value_map(original.account_ids, {"primary": "SIM-001"}, AccountId)
    _assert_typed_value_map(original.currencies, {"settlement": "USD"}, Currency)
    _assert_typed_value_map(original.bar_types, {"bar": str(bar_type)}, BarType)
    _assert_typed_value_map(original.prices, {"bid": "1.23456"}, Price)
    _assert_typed_value_map(original.quantities, {"size": "10.500"}, Quantity)
    _assert_typed_value_map(original.monies, {"notional": "123.45 USD"}, Money)
    _assert_typed_map(
        original.prices_by_instrument,
        {"AUD/USD.SIM": "1.23456"},
        InstrumentId,
        Price,
    )
    _assert_typed_map(
        original.quantities_by_account,
        {"SIM-001": "10.500"},
        AccountId,
        Quantity,
    )
    _assert_typed_map(original.monies_by_currency, {"USD": "123.45 USD"}, Currency, Money)
    _assert_typed_map(
        original.prices_by_bar_type,
        {str(bar_type): "1.23456"},
        BarType,
        Price,
    )
    _assert_typed_map(
        original.hash_prices_by_instrument,
        {"AUD/USD.SIM": "1.23456"},
        InstrumentId,
        Price,
    )
    assert original.strings == {"label": "alpha"}
    assert original.floats_64 == {"ratio": 1.5}
    assert original.floats_32 == {"ratio": 1.5}
    assert original.booleans == {"enabled": True}
    assert original.integers_u64 == {"count": 7}
    assert original.integers_i64 == {"delta": -7}
    assert original.integers_u32 == {"count": 5}
    assert original.integers_i32 == {"delta": -5}

    pyo3_catalog.write_custom_data(wrapped)

    result = pyo3_catalog.query(
        "RustTestTypedMapCustomData",
        None,
        None,
        None,
        None,
        None,
        True,
    )

    assert len(result) == 1
    item = result[0]
    assert isinstance(item, CustomData), f"Expected CustomData, found {type(item)}"
    assert item.data_type.type_name == "RustTestTypedMapCustomData"
    assert item.data_type.metadata == metadata
    assert item.data_type.identifier is None

    roundtripped = item.data
    assert isinstance(roundtripped, RustTestTypedMapCustomData), (
        f"Expected RustTestTypedMapCustomData in .data, found {type(roundtripped)}"
    )
    assert roundtripped.name == original.name
    _assert_typed_value_map(roundtripped.instrument_ids, {"primary": "AUD/USD.SIM"}, InstrumentId)
    _assert_typed_value_map(roundtripped.account_ids, {"primary": "SIM-001"}, AccountId)
    _assert_typed_value_map(roundtripped.currencies, {"settlement": "USD"}, Currency)
    _assert_typed_value_map(roundtripped.bar_types, {"bar": str(bar_type)}, BarType)
    _assert_typed_value_map(roundtripped.prices, {"bid": "1.23456"}, Price)
    _assert_typed_value_map(roundtripped.quantities, {"size": "10.500"}, Quantity)
    _assert_typed_value_map(roundtripped.monies, {"notional": "123.45 USD"}, Money)
    _assert_typed_map(
        roundtripped.prices_by_instrument,
        {"AUD/USD.SIM": "1.23456"},
        InstrumentId,
        Price,
    )
    _assert_typed_map(
        roundtripped.quantities_by_account,
        {"SIM-001": "10.500"},
        AccountId,
        Quantity,
    )
    _assert_typed_map(roundtripped.monies_by_currency, {"USD": "123.45 USD"}, Currency, Money)
    _assert_typed_map(
        roundtripped.prices_by_bar_type,
        {str(bar_type): "1.23456"},
        BarType,
        Price,
    )
    _assert_typed_map(
        roundtripped.hash_prices_by_instrument,
        {"AUD/USD.SIM": "1.23456"},
        InstrumentId,
        Price,
    )
    assert roundtripped.strings == original.strings
    assert roundtripped.floats_64 == original.floats_64
    assert roundtripped.floats_32 == original.floats_32
    assert roundtripped.booleans == original.booleans
    assert roundtripped.integers_u64 == original.integers_u64
    assert roundtripped.integers_i64 == original.integers_i64
    assert roundtripped.integers_u32 == original.integers_u32
    assert roundtripped.integers_i32 == original.integers_i32
    assert roundtripped.ts_event == original.ts_event
    assert roundtripped.ts_init == original.ts_init

    def assert_typed_map_data(orig, rt):
        assert rt.name == orig.name
        _assert_typed_value_map(rt.instrument_ids, {"primary": "AUD/USD.SIM"}, InstrumentId)
        _assert_typed_value_map(rt.account_ids, {"primary": "SIM-001"}, AccountId)
        _assert_typed_value_map(rt.currencies, {"settlement": "USD"}, Currency)
        _assert_typed_value_map(rt.bar_types, {"bar": str(bar_type)}, BarType)
        _assert_typed_value_map(rt.prices, {"bid": "1.23456"}, Price)
        _assert_typed_value_map(rt.quantities, {"size": "10.500"}, Quantity)
        _assert_typed_value_map(rt.monies, {"notional": "123.45 USD"}, Money)
        _assert_typed_map(rt.prices_by_instrument, {"AUD/USD.SIM": "1.23456"}, InstrumentId, Price)
        _assert_typed_map(
            rt.quantities_by_account,
            {"SIM-001": "10.500"},
            AccountId,
            Quantity,
        )
        _assert_typed_map(rt.monies_by_currency, {"USD": "123.45 USD"}, Currency, Money)
        _assert_typed_map(rt.prices_by_bar_type, {str(bar_type): "1.23456"}, BarType, Price)
        _assert_typed_map(
            rt.hash_prices_by_instrument,
            {"AUD/USD.SIM": "1.23456"},
            InstrumentId,
            Price,
        )
        assert rt.strings == orig.strings
        assert rt.floats_64 == orig.floats_64
        assert rt.floats_32 == orig.floats_32
        assert rt.booleans == orig.booleans
        assert rt.integers_u64 == orig.integers_u64
        assert rt.integers_i64 == orig.integers_i64
        assert rt.integers_u32 == orig.integers_u32
        assert rt.integers_i32 == orig.integers_i32
        assert rt.ts_event == orig.ts_event
        assert rt.ts_init == orig.ts_init

    _assert_custom_data_json_roundtrip(
        result,
        RustTestTypedMapCustomData,
        assert_typed_map_data,
    )


def _assert_typed_price_map(actual, expected, instrument_id_type, price_type):
    _assert_typed_map(actual, expected, instrument_id_type, price_type)


def _assert_typed_map(actual, expected, key_type, value_type):
    assert {str(key): str(value) for key, value in actual.items()} == expected
    for key, value in actual.items():
        assert isinstance(key, key_type)
        assert isinstance(value, value_type)


def _assert_typed_value_map(actual, expected, value_type):
    assert {key: str(value) for key, value in actual.items()} == expected
    for key, value in actual.items():
        assert isinstance(key, str)
        assert isinstance(value, value_type)


def test_python_only_customdataclass_pyo3_roundtrip(tmp_path):
    """
    Test Python-only custom data via customdataclass_pyo3 and
    register_custom_data_class.
    """
    from nautilus_trader.core.nautilus_pyo3 import ParquetDataCatalog
    from nautilus_trader.core.nautilus_pyo3.model import CustomData
    from nautilus_trader.core.nautilus_pyo3.model import DataType
    from nautilus_trader.core.nautilus_pyo3.model import custom_data_backend_kind
    from nautilus_trader.core.nautilus_pyo3.model import register_custom_data_class
    from nautilus_trader.model.custom import customdataclass_pyo3

    @customdataclass_pyo3()
    class MarketTickPython:
        symbol: str = ""
        price: float = 0.0
        volume: int = 0

    register_custom_data_class(MarketTickPython)
    catalog_path = tmp_path / "catalog_python_only"
    catalog_path.mkdir(parents=True, exist_ok=True)
    pyo3_catalog = ParquetDataCatalog(str(catalog_path))

    metadata = {"exchange": "NASDAQ", "asset_class": "equity"}
    identifier = "NASDAQ-EQUITY"
    data_type = DataType("MarketTickPython", metadata, identifier)
    original_data = [
        MarketTickPython(1, 1, "AAPL", 150.5, 1000),
        MarketTickPython(2, 2, "GOOGL", 2800.0, 500),
    ]
    wrapped = [CustomData(data_type, item) for item in original_data]
    assert [custom_data_backend_kind(item) for item in wrapped] == ["python", "python"]
    pyo3_catalog.write_custom_data(wrapped)
    result = pyo3_catalog.query(
        "MarketTickPython",
        None,
        None,
        None,
        None,
        None,
        True,
    )
    roundtripped = []

    for item in result:
        assert isinstance(item, CustomData), f"Expected CustomData, found {type(item)}"
        assert custom_data_backend_kind(item) == "python"
        inner = item.data
        assert isinstance(inner, MarketTickPython), (
            f"Expected MarketTickPython in .data, found {type(inner)}"
        )
        roundtripped.append(inner)

    assert len(roundtripped) == len(original_data)

    # DataType (type_name, metadata, identifier) should be restored on catalog decode
    for item in result:
        assert item.data_type.type_name == "MarketTickPython"
        assert item.data_type.metadata == metadata
        assert item.data_type.identifier == identifier

    for expected, actual in zip(original_data, roundtripped, strict=True):
        assert expected.symbol == actual.symbol
        assert expected.price == actual.price
        assert expected.volume == actual.volume
        assert expected.ts_event == actual.ts_event
        assert expected.ts_init == actual.ts_init

    # JSON roundtrip: CustomData -> JSON bytes -> CustomData.from_json_bytes
    def assert_market_tick(orig, rt):
        assert orig.symbol == rt.symbol
        assert orig.price == rt.price
        assert orig.volume == rt.volume
        assert orig.ts_event == rt.ts_event
        assert orig.ts_init == rt.ts_init

    _assert_custom_data_json_roundtrip(result, MarketTickPython, assert_market_tick)


def test_python_only_customdataclass_pyo3_dict_roundtrip(tmp_path):
    """
    Test Python-only custom data dict fields roundtrip via JSON-backed Arrow strings.
    """
    import json as json_module
    from dataclasses import field

    import pyarrow as pa

    from nautilus_trader.core.nautilus_pyo3 import ParquetDataCatalog
    from nautilus_trader.core.nautilus_pyo3.model import CustomData
    from nautilus_trader.core.nautilus_pyo3.model import DataType
    from nautilus_trader.core.nautilus_pyo3.model import custom_data_backend_kind
    from nautilus_trader.core.nautilus_pyo3.model import register_custom_data_class
    from nautilus_trader.model.custom import customdataclass_pyo3

    @customdataclass_pyo3()
    class JsonBlobPython:
        name: str = ""
        payload: dict[str, object] = field(default_factory=dict)

    register_custom_data_class(JsonBlobPython)
    catalog_path = tmp_path / "catalog_python_dict"
    catalog_path.mkdir(parents=True, exist_ok=True)
    pyo3_catalog = ParquetDataCatalog(str(catalog_path))

    metadata = {"source": "python", "format": "json-dict"}
    identifier = "PYTHON-DICT"
    data_type = DataType("JsonBlobPython", metadata, identifier)
    original_payloads = [
        {"symbol": "AAPL", "nested": {"count": 2, "enabled": True}},
        {"symbol": "MSFT", "values": [1, 2, 3], "ratio": 1.25},
    ]
    original_data = [
        JsonBlobPython(1, 1, "first", original_payloads[0]),
        JsonBlobPython(2, 2, "second", original_payloads[1]),
    ]
    wrapped = [CustomData(data_type, item) for item in original_data]

    assert JsonBlobPython._schema.field("payload").type == pa.string()
    assert [custom_data_backend_kind(item) for item in wrapped] == ["python", "python"]

    arrow_row = original_data[0].to_arrow().to_pylist()[0]
    assert arrow_row["payload"] == json_module.dumps(original_payloads[0], sort_keys=True)

    pyo3_catalog.write_custom_data(wrapped)
    result = pyo3_catalog.query(
        "JsonBlobPython",
        None,
        None,
        None,
        None,
        None,
        True,
    )

    roundtripped = []

    for item in result:
        assert isinstance(item, CustomData), f"Expected CustomData, found {type(item)}"
        assert custom_data_backend_kind(item) == "python"
        inner = item.data
        assert isinstance(inner, JsonBlobPython), (
            f"Expected JsonBlobPython in .data, found {type(inner)}"
        )
        assert isinstance(inner.payload, dict)
        roundtripped.append(inner)

    assert len(roundtripped) == len(original_data)

    for item in result:
        assert item.data_type.type_name == "JsonBlobPython"
        assert item.data_type.metadata == metadata
        assert item.data_type.identifier == identifier

    for expected, expected_payload, actual in zip(
        original_data,
        original_payloads,
        roundtripped,
        strict=True,
    ):
        assert expected.name == actual.name
        assert actual.payload == expected_payload
        assert expected.ts_event == actual.ts_event
        assert expected.ts_init == actual.ts_init

    def assert_json_blob(orig, rt):
        assert orig.name == rt.name
        assert rt.payload == orig.payload
        assert orig.ts_event == rt.ts_event
        assert orig.ts_init == rt.ts_init

    _assert_custom_data_json_roundtrip(result, JsonBlobPython, assert_json_blob)


def test_python_custom_data_equality_by_identity():
    """
    Two CustomData wrappers around different Python objects (same type and timestamps)
    must not be equal: equality is by Python object identity, not by field values.
    """
    from nautilus_trader.core.nautilus_pyo3.model import CustomData
    from nautilus_trader.core.nautilus_pyo3.model import DataType
    from nautilus_trader.core.nautilus_pyo3.model import register_custom_data_class
    from nautilus_trader.model.custom import customdataclass_pyo3

    @customdataclass_pyo3()
    class SameTsType:
        value: int = 0

    register_custom_data_class(SameTsType)
    data_type = DataType("SameTsType", None, None)

    # Same ts_event/ts_init and type, but different payloads (different Python objects).
    a = SameTsType(1, 1, 100)
    b = SameTsType(1, 1, 200)
    wrap_a = CustomData(data_type, a)
    wrap_b = CustomData(data_type, b)

    assert wrap_a != wrap_b, "Different Python objects must not compare equal (identity semantics)"
