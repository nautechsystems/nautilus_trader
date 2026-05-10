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
        inner = item.data
        assert isinstance(inner, RustTestParamsCustomData), (
            f"Expected RustTestParamsCustomData in .data, found {type(inner)}"
        )
        assert isinstance(inner.params, dict)
        roundtripped.append(inner)

    assert len(roundtripped) == len(original_data)

    for item in result:
        assert item.data_type.type_name == "RustTestParamsCustomData"
        assert item.data_type.metadata == metadata
        assert item.data_type.identifier is None

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
