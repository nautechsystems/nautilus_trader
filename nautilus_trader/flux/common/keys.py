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

from __future__ import annotations

from typing import Final

from nautilus_trader.flux.common.config import FLUX_SCHEMA_VERSION
from nautilus_trader.flux.common.config import validate_identifier_part


class FluxRedisKeys:
    """
    Builder for Flux Redis keys under the versioned namespace.
    """

    PREFIX: Final[str] = f"flux:{FLUX_SCHEMA_VERSION}"

    def __init__(self, strategy_id: str) -> None:
        self._strategy_id = validate_identifier_part(strategy_id, "strategy_id")

    @property
    def strategy_id(self) -> str:
        return self._strategy_id

    def state(self) -> str:
        return f"{self.PREFIX}:state:{self._strategy_id}"

    def events(self) -> str:
        return f"{self.PREFIX}:events:{self._strategy_id}"

    def trades_stream(self) -> str:
        return f"{self.PREFIX}:trades:stream:{self._strategy_id}"

    def alerts(self) -> str:
        return f"{self.PREFIX}:alerts:{self._strategy_id}"

    def params_hash(self) -> str:
        return f"{self.PREFIX}:params:{self._strategy_id}"

    def params_channel(self) -> str:
        return f"{self.PREFIX}:params:{self._strategy_id}"

    @classmethod
    def global_params_channel(cls) -> str:
        return f"{cls.PREFIX}:params:global"

    def inbound_stream(self, environment: str, topic: str) -> str:
        safe_environment = validate_identifier_part(environment, "environment")
        safe_topic = validate_identifier_part(topic, "topic")
        return f"{self.PREFIX}:in:stream:{safe_environment}:{self._strategy_id}:{safe_topic}"
