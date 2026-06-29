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

import pytest

from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXEnvironment
from nautilus_trader.adapters.okx import OKXExecClientConfig
from nautilus_trader.adapters.okx import OKXRegion
from nautilus_trader.adapters.okx import get_okx_http_base_url
from nautilus_trader.adapters.okx import get_okx_ws_url_business
from nautilus_trader.adapters.okx import get_okx_ws_url_private
from nautilus_trader.adapters.okx import get_okx_ws_url_public
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


@pytest.mark.parametrize(
    ("region", "expected"),
    [
        (OKXRegion.GLOBAL, "https://www.okx.com"),
        (OKXRegion.EEA, "https://eea.okx.com"),
        (OKXRegion.US, "https://us.okx.com"),
    ],
)
def test_http_base_url_by_region(region: OKXRegion, expected: str) -> None:
    assert get_okx_http_base_url(region) == expected


def test_http_base_url_defaults_to_global() -> None:
    assert get_okx_http_base_url() == "https://www.okx.com"


@pytest.mark.parametrize(
    ("region", "public", "private", "business"),
    [
        (
            OKXRegion.GLOBAL,
            "wss://ws.okx.com:8443/ws/v5/public",
            "wss://ws.okx.com:8443/ws/v5/private",
            "wss://ws.okx.com:8443/ws/v5/business",
        ),
        (
            OKXRegion.EEA,
            "wss://wseea.okx.com:8443/ws/v5/public",
            "wss://wseea.okx.com:8443/ws/v5/private",
            "wss://wseea.okx.com:8443/ws/v5/business",
        ),
        (
            OKXRegion.US,
            "wss://wsus.okx.com:8443/ws/v5/public",
            "wss://wsus.okx.com:8443/ws/v5/private",
            "wss://wsus.okx.com:8443/ws/v5/business",
        ),
    ],
)
def test_ws_urls_by_region_live(
    region: OKXRegion,
    public: str,
    private: str,
    business: str,
) -> None:
    assert get_okx_ws_url_public(OKXEnvironment.LIVE, region) == public
    assert get_okx_ws_url_private(OKXEnvironment.LIVE, region) == private
    assert get_okx_ws_url_business(OKXEnvironment.LIVE, region) == business


def test_ws_urls_eea_demo() -> None:
    assert (
        get_okx_ws_url_public(OKXEnvironment.DEMO, OKXRegion.EEA)
        == "wss://wseeapap.okx.com:8443/ws/v5/public"
    )


def test_data_config_defaults_to_global_region() -> None:
    config = OKXDataClientConfig()

    # The pyo3 config exposes no field getters; inspect via repr.
    assert "region: Global" in repr(config)


def test_exec_config_accepts_region() -> None:
    config = OKXExecClientConfig(
        trader_id=TraderId("TRADER-001"),
        account_id=AccountId("OKX-001"),
        region=OKXRegion.EEA,
    )

    assert "region: Eea" in repr(config)


def test_okx_region_enum_surface() -> None:
    # OKXRegion must mirror OKXEnvironment's surface so frozen configs with a region
    # field stay hashable, and string/TOML values round-trip.
    assert len({OKXRegion.GLOBAL, OKXRegion.EEA, OKXRegion.US}) == 3  # hashable + distinct
    assert OKXRegion.from_str("eea") == OKXRegion.EEA
    assert OKXRegion.from_str("EEA") == OKXRegion.EEA  # case-insensitive
    assert set(OKXRegion.variants()) == {"global", "eea", "us"}
