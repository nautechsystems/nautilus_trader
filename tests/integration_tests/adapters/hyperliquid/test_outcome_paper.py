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

import pytest

from nautilus_trader.adapters.hyperliquid.enums import HyperliquidProductType
from nautilus_trader.adapters.hyperliquid.paper import get_price_bucket_index
from nautilus_trader.adapters.hyperliquid.paper import get_price_bucket_thresholds
from nautilus_trader.adapters.hyperliquid.paper import is_outcome_instrument_id
from nautilus_trader.adapters.hyperliquid.paper import select_active_price_bucket_instrument
from nautilus_trader.adapters.hyperliquid.paper import select_outcome_instrument_id
from nautilus_trader.adapters.hyperliquid.paper import validate_outcome_price
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def test_is_outcome_instrument_id():
    assert is_outcome_instrument_id(
        InstrumentId.from_str("OUTCOME-2-YES-OUTCOME.HYPERLIQUID"),
    )
    assert not is_outcome_instrument_id(
        InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID"),
    )


def test_validate_outcome_price():
    validate_outcome_price(Decimal("0.001"))
    validate_outcome_price(Decimal("0.500"))
    validate_outcome_price(Decimal("0.999"))

    with pytest.raises(ValueError):
        validate_outcome_price(Decimal("0.0009"))
    with pytest.raises(ValueError):
        validate_outcome_price(Decimal("1.0001"))


def test_select_outcome_instrument_id_prefers_requested():
    available = [
        InstrumentId.from_str("OUTCOME-2-YES-OUTCOME.HYPERLIQUID"),
        InstrumentId.from_str("OUTCOME-2-NO-OUTCOME.HYPERLIQUID"),
    ]
    preferred = "OUTCOME-2-NO-OUTCOME.HYPERLIQUID"

    selected = select_outcome_instrument_id(available, preferred=preferred)
    assert selected == InstrumentId.from_str(preferred)


def test_price_bucket_helpers_parse_thresholds_and_index():
    inst_id = InstrumentId.from_str("OUTCOME-7130-YES-OUTCOME.HYPERLIQUID")
    raw_symbol = Symbol("#71300")
    price_inc = Price.from_str("0.000001")
    size_inc = Quantity.from_str("0.000001")
    usd_h = Currency.from_str("USDH")

    inst = BinaryOption(
        instrument_id=inst_id,
        raw_symbol=raw_symbol,
        outcome="YES",
        description="test",
        asset_class=AssetClass.ALTERNATIVE,
        currency=usd_h,
        price_precision=price_inc.precision,
        size_precision=size_inc.precision,
        price_increment=price_inc,
        size_increment=size_inc,
        activation_ns=0,
        expiration_ns=1_000_000_000,
        ts_event=0,
        ts_init=0,
        info={
            "hyperliquid": {
                "bucket_index": 0,
                "price_bucket": {
                    "underlying": "BTC",
                    "period": "15m",
                    "price_thresholds": ["81010", "81253"],
                },
            },
        },
    )

    low, high = get_price_bucket_thresholds(inst)
    assert low == Decimal(81010)
    assert high == Decimal(81253)
    assert get_price_bucket_index(inst) == 0


def test_select_active_price_bucket_instrument_filters_and_prefers_yes():
    venue = Venue("HYPERLIQUID")
    raw_symbol = Symbol("#71300")
    price_inc = Price.from_str("0.000001")
    size_inc = Quantity.from_str("0.000001")
    usd_h = Currency.from_str("USDH")

    def make_inst(symbol: str, bucket_index: int, expiry_ns: int, outcome: str) -> BinaryOption:
        inst_id = InstrumentId(symbol=Symbol(symbol), venue=venue)
        return BinaryOption(
            instrument_id=inst_id,
            raw_symbol=raw_symbol,
            outcome=outcome,
            description="test",
            asset_class=AssetClass.ALTERNATIVE,
            currency=usd_h,
            price_precision=price_inc.precision,
            size_precision=size_inc.precision,
            price_increment=price_inc,
            size_increment=size_inc,
            activation_ns=0,
            expiration_ns=expiry_ns,
            ts_event=0,
            ts_init=0,
            info={
                "hyperliquid": {
                    "bucket_index": bucket_index,
                    "price_bucket": {
                        "underlying": "BTC",
                        "period": "15m",
                        "price_thresholds": ["81010", "81253"],
                    },
                },
            },
        )

    now_ns = 1_000_000_000
    insts = [
        make_inst("OUTCOME-7130-NO-OUTCOME", 0, now_ns + 10, "NO"),
        make_inst("OUTCOME-7130-YES-OUTCOME", 0, now_ns + 10, "YES"),
        make_inst("OUTCOME-7131-YES-OUTCOME", 1, now_ns + 10, "YES"),
    ]

    selected = select_active_price_bucket_instrument(
        insts,
        underlying="BTC",
        period="15m",
        bucket_index=0,
        side="YES",
        now_ns=now_ns,
    )
    assert selected.symbol.value == "OUTCOME-7130-YES-OUTCOME"


@pytest.mark.asyncio
async def test_provider_requests_outcomes_when_configured(mock_http_client):
    provider = HyperliquidInstrumentProvider(
        client=mock_http_client,
        config=InstrumentProviderConfig(),
        product_types=[HyperliquidProductType.OUTCOME],
    )

    await provider.load_all_async()

    mock_http_client.load_instrument_definitions.assert_called_once_with(
        include_spot=False,
        include_perps=False,
        include_perps_hip3=False,
        include_outcomes=True,
    )


@pytest.mark.asyncio
async def test_provider_falls_back_for_older_http_client_signature(mock_http_client):
    mock_http_client.load_instrument_definitions.side_effect = [
        TypeError("unexpected keyword include_outcomes"),
        [],
    ]

    provider = HyperliquidInstrumentProvider(
        client=mock_http_client,
        config=InstrumentProviderConfig(),
        product_types=[HyperliquidProductType.SPOT],
    )

    await provider.load_all_async()

    assert mock_http_client.load_instrument_definitions.call_count == 2
    assert (
        "include_outcomes" in mock_http_client.load_instrument_definitions.call_args_list[0].kwargs
    )
    assert (
        "include_outcomes"
        not in mock_http_client.load_instrument_definitions.call_args_list[1].kwargs
    )


@pytest.mark.asyncio
async def test_provider_outcomes_require_updated_http_client_signature(mock_http_client):
    mock_http_client.load_instrument_definitions.side_effect = TypeError(
        "unexpected keyword include_outcomes",
    )

    provider = HyperliquidInstrumentProvider(
        client=mock_http_client,
        config=InstrumentProviderConfig(),
        product_types=[HyperliquidProductType.OUTCOME],
    )

    with pytest.raises(RuntimeError, match="include_outcomes support"):
        await provider.load_all_async()
