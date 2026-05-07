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
from nautilus_trader.adapters.hyperliquid.paper import is_outcome_instrument_id
from nautilus_trader.adapters.hyperliquid.paper import select_outcome_instrument_id
from nautilus_trader.adapters.hyperliquid.paper import validate_outcome_price
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId


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
