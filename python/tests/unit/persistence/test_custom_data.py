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

from nautilus_trader.model import AccountId
from nautilus_trader.model import BarType
from nautilus_trader.model import Currency
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.persistence import MacroYieldCurveData
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import RustTestParamsCustomData
from nautilus_trader.persistence import RustTestPriceMapCustomData
from nautilus_trader.persistence import RustTestTypedMapCustomData


def test_yield_curve_custom_data_catalog_and_json_roundtrip(tmp_path):
    data_type = DataType(
        "MacroYieldCurveData",
        {"currency": "USD", "curve": "government"},
        "USD-GOVERNMENT",
    )
    original = MacroYieldCurveData(
        "USD",
        [0.25, 0.5, 1.0, 2.0, 5.0],
        [0.025, 0.03, 0.035, 0.04, 0.045],
        11,
        12,
    )

    item = _roundtrip(tmp_path, data_type, original)
    restored = CustomData.from_json_bytes(item.to_json_bytes())

    assert item.data_type == data_type
    assert isinstance(item.data, MacroYieldCurveData)
    assert item.data.curve_name == "USD"
    assert item.data.tenors == [0.25, 0.5, 1.0, 2.0, 5.0]
    assert item.data.interest_rates == [0.025, 0.03, 0.035, 0.04, 0.045]
    assert item.ts_event == 11
    assert item.ts_init == 12
    assert isinstance(restored.data, MacroYieldCurveData)
    assert restored.data.curve_name == "USD"
    assert restored.data.tenors == item.data.tenors
    assert restored.data.interest_rates == item.data.interest_rates
    assert restored.ts_event == 11
    assert restored.ts_init == 12


def test_params_custom_data_catalog_and_json_roundtrip(tmp_path):
    data_type = DataType(
        "RustTestParamsCustomData",
        {"source": "python", "kind": "params"},
    )
    params = {
        "name": "alpha",
        "enabled": True,
        "count": 7,
        "ratio": 1.25,
        "nested": {"level": 2, "tags": ["x", "y"]},
    }
    original = RustTestParamsCustomData("first", params, 21, 22)

    item = _roundtrip(tmp_path, data_type, original)
    restored = CustomData.from_json_bytes(item.to_json_bytes())

    assert item.data_type == data_type
    assert isinstance(item.data, RustTestParamsCustomData)
    assert item.data.name == "first"
    assert item.data.params == params
    assert item.ts_event == 21
    assert item.ts_init == 22
    assert isinstance(restored.data, RustTestParamsCustomData)
    assert restored.data.name == "first"
    assert restored.data.params == params
    assert restored.ts_event == 21
    assert restored.ts_init == 22


def test_price_map_custom_data_catalog_and_json_roundtrip(tmp_path):
    audusd_id = InstrumentId.from_str("AUD/USD.SIM")
    btcusdt_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    prices = {
        audusd_id: Price.from_str("1.23456"),
        btcusdt_id: Price.from_str("65432.10"),
    }
    data_type = DataType(
        "RustTestPriceMapCustomData",
        {"source": "python", "kind": "price-map"},
    )
    original = RustTestPriceMapCustomData("prices", prices, 31, 32)

    item = _roundtrip(tmp_path, data_type, original)
    restored = CustomData.from_json_bytes(item.to_json_bytes())

    assert item.data_type == data_type
    assert isinstance(item.data, RustTestPriceMapCustomData)
    assert item.data.name == "prices"
    assert item.data.prices == prices
    assert item.ts_event == 31
    assert item.ts_init == 32
    assert isinstance(restored.data, RustTestPriceMapCustomData)
    assert restored.data.name == "prices"
    assert restored.data.prices == prices
    assert restored.ts_event == 31
    assert restored.ts_init == 32


def test_typed_map_custom_data_catalog_and_json_roundtrip(tmp_path):
    instrument_id = InstrumentId.from_str("AUD/USD.SIM")
    account_id = AccountId.from_str("SIM-001")
    currency = Currency.from_str("USD")
    bar_type = BarType.from_str("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL")
    price = Price.from_str("1.23456")
    quantity = Quantity.from_str("10.500")
    money = Money.from_str("123.45 USD")
    data_type = DataType(
        "RustTestTypedMapCustomData",
        {"source": "python", "kind": "typed-map"},
    )
    original = RustTestTypedMapCustomData(
        "typed",
        {"primary": instrument_id},
        {"primary": account_id},
        {"settlement": currency},
        {"bar": bar_type},
        {"bid": price},
        {"size": quantity},
        {"notional": money},
        {instrument_id: price},
        {account_id: quantity},
        {currency: money},
        {bar_type: price},
        {instrument_id: price},
        {"label": "alpha"},
        {"ratio": 1.5},
        {"ratio": 1.5},
        {"enabled": True},
        {"count": 7},
        {"delta": -7},
        {"count": 5},
        {"delta": -5},
        41,
        42,
    )

    item = _roundtrip(tmp_path, data_type, original)
    restored = CustomData.from_json_bytes(item.to_json_bytes())

    assert item.data_type == data_type
    _assert_typed_maps(
        item.data,
        instrument_id,
        account_id,
        currency,
        bar_type,
        price,
        quantity,
        money,
    )
    assert item.ts_event == 41
    assert item.ts_init == 42
    _assert_typed_maps(
        restored.data,
        instrument_id,
        account_id,
        currency,
        bar_type,
        price,
        quantity,
        money,
    )
    assert restored.ts_event == 41
    assert restored.ts_init == 42


def _roundtrip(tmp_path, data_type, original):
    register_custom_data_class(type(original))
    catalog_path = tmp_path / data_type.type_name
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    catalog.write_custom_data([CustomData(data_type, original)])

    result = catalog.query_custom_data(
        data_type.type_name,
        identifiers=[data_type.identifier] if data_type.identifier else None,
    )

    assert len(result) == 1
    assert isinstance(result[0], CustomData)
    return result[0]


def _assert_typed_maps(
    value,
    instrument_id,
    account_id,
    currency,
    bar_type,
    price,
    quantity,
    money,
):
    assert isinstance(value, RustTestTypedMapCustomData)
    assert value.name == "typed"
    assert value.instrument_ids == {"primary": instrument_id}
    assert value.account_ids == {"primary": account_id}
    assert value.currencies == {"settlement": currency}
    assert value.bar_types == {"bar": bar_type}
    assert value.prices == {"bid": price}
    assert value.quantities == {"size": quantity}
    assert value.monies == {"notional": money}
    assert value.prices_by_instrument == {instrument_id: price}
    assert value.quantities_by_account == {account_id: quantity}
    assert value.monies_by_currency == {currency: money}
    assert value.prices_by_bar_type == {bar_type: price}
    assert value.hash_prices_by_instrument == {instrument_id: price}
    assert value.strings == {"label": "alpha"}
    assert value.floats_64 == {"ratio": 1.5}
    assert value.floats_32 == {"ratio": 1.5}
    assert value.booleans == {"enabled": True}
    assert value.integers_u64 == {"count": 7}
    assert value.integers_i64 == {"delta": -7}
    assert value.integers_u32 == {"count": 5}
    assert value.integers_i32 == {"delta": -5}
