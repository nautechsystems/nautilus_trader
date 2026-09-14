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
"""
Verify owned response payloads and exact request metadata.
"""

import pytest

from nautilus_trader.core import UUID4
from nautilus_trader.live import CustomDataResponse
from nautilus_trader.live import InstrumentResponse
from nautilus_trader.live import InstrumentsResponse
from nautilus_trader.live import OptionChainReferencePriceResponse
from nautilus_trader.model import ClientId
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import OptionSeriesId
from nautilus_trader.model import Price
from nautilus_trader.model import Venue
from nautilus_trader.testkit.providers import TestInstrumentProvider


@pytest.mark.parametrize("kind", ["instrument", "instruments", "custom"])
def test_response_payload_and_metadata_survive_owned_conversion(kind) -> None:
    """
    Preserve payload identity, correlation, bounds, and nested params across conversion.
    """
    instrument = TestInstrumentProvider.audusd_sim()
    client_id = ClientId("RESPONSE")
    correlation_id = UUID4()
    params = {"tag": "response", "nested": {"sequence": 47}}
    data_type = DataType("Example", metadata={"source": "external"})
    custom = CustomData(data_type, instrument)
    shared = {
        "client_id": client_id,
        "correlation_id": correlation_id,
        "ts_init": 53,
        "start": 59,
        "end": 61,
        "params": params,
    }

    if kind == "instrument":
        response = InstrumentResponse(instrument_id=instrument.id, data=instrument, **shared)
        expected = instrument
    elif kind == "instruments":
        response = InstrumentsResponse(venue=Venue("SIM"), data=[instrument], **shared)
        expected = [instrument]
    else:
        response = CustomDataResponse(
            data_type=data_type,
            venue=Venue("SIM"),
            data=[custom],
            **shared,
        )
        expected = [custom]
    params["nested"]["sequence"] = 71
    returned = response.params
    returned["nested"]["sequence"] = 73

    assert response.client_id == client_id
    assert response.correlation_id == correlation_id
    assert response.ts_init == 53
    assert response.start == 59
    assert response.end == 61
    assert response.params == {"tag": "response", "nested": {"sequence": 47}}
    if kind == "custom":
        assert response.data_type == data_type
        assert response.venue == Venue("SIM")
        assert [item.data for item in response.data] == [instrument]
        assert [item.data_type for item in response.data] == [data_type]
    else:
        assert response.data == expected
        if kind == "instrument":
            assert response.instrument_id == instrument.id
        else:
            assert response.venue == Venue("SIM")
    with pytest.raises(AttributeError):
        response.ts_init = 79


@pytest.mark.parametrize("price", [None, Price.from_str("123.456")])
def test_option_reference_response_preserves_absent_and_exact_price(price) -> None:
    """
    Keep absent reference prices distinct from concrete prices and retain metadata.
    """
    client_id = ClientId("OPTIONS")
    series = OptionSeriesId("SIM", "BTC", "USD", 1780000000000000000)
    correlation = UUID4()
    params = {"source": "index", "sequence": 83}
    response = OptionChainReferencePriceResponse(client_id, series, price, correlation, 89, params)
    params["sequence"] = 97

    assert response.client_id == client_id
    assert response.series_id == series
    assert response.price == price
    assert response.correlation_id == correlation
    assert response.ts_init == 89
    assert response.params == {"source": "index", "sequence": 83}
