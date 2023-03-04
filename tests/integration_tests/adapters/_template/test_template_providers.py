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

from nautilus_trader.adapters._template.core import TEMPLATE_VENUE  # noqa
from nautilus_trader.adapters._template.providers import TemplateInstrumentProvider  # noqa
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger


@pytest.fixture(scope="function")
def instrument_provider():
    clock = TestClock()
    return TemplateInstrumentProvider(
        venue=TEMPLATE_VENUE,
        logger=Logger(clock),
    )


@pytest.mark.skip(reason="example")
def test_load_all_async(instrument_provider):
    pass


@pytest.mark.skip(reason="example")
def test_load_all(instrument_provider):
    pass


@pytest.mark.skip(reason="example")
def test_load(instrument_provider):
    pass
