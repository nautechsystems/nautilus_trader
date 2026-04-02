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
Minimal test fixtures for LiveNode from_config registration tests.

PyO3 #[new] maps to __new__, not __init__. Subclasses inherit the base constructor
automatically and should not define __init__.

"""

from nautilus_trader.common import DataActor
from nautilus_trader.common import DataActorConfig
from nautilus_trader.trading import Strategy


class TestActorConfig(DataActorConfig):
    pass


class TestActor(DataActor):
    pass


class TestStrategy(Strategy):
    pass


class TestExecAlgorithmConfig(DataActorConfig):
    pass


class TestExecAlgorithm(DataActor):
    pass
