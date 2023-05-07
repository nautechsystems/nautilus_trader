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

import pytest

from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


@pytest.fixture(name="audusd_instrument")
def fixture_audusd_instrument() -> CurrencyPair:
    return TestInstrumentProvider.default_fx_ccy("AUD/USD", Venue("SIM"))


@pytest.fixture(name="data_provider", scope="session")
def fixture_data_provider() -> TestDataProvider:
    return TestDataProvider()


@pytest.fixture(name="audusd_quote_ticks", scope="session")
def fixture_audusd_quote_ticks(
    data_provider: TestDataProvider,
    audusd_instrument: CurrencyPair,
) -> list[QuoteTick]:
    wrangler = QuoteTickDataWrangler(instrument=audusd_instrument)
    return wrangler.process(data_provider.read_csv_ticks("truefx-audusd-ticks.csv"))
